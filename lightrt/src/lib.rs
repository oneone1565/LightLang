//! lightrt —— Light 语言的内存安全运行时
//!
//! 这是 Light 语言中「需要安全」的部分，全部用 Rust 编写，
//! 通过 C ABI 暴露给编译器生成的汇编代码调用。
//!
//! 安全策略：
//!   - 所有数组访问都经过边界检查，越界立即终止进程（安全崩溃而非 UB）
//!   - 所有内存分配/释放由 Rust 管理，禁止用户直接操作裸指针
//!   - 字符串以 (ptr, len) 表示，由 Rust 负责 UTF-8 与生命周期
//!   - 空指针/无效指针在入口处被拦截
//!
//! 容器元素统一使用「标签值」表示，标签存放在与数据并行的字节数组中，
//! 使 打印/字典查找 等操作能够在运行时正确区分整数、浮点、字符串与嵌套容器。

use std::io::Write;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

// ---------------------------------------------------------------------------
// 内存追踪：用于检测泄漏（调试期安全网）
// ---------------------------------------------------------------------------
static LIVE_ARRAY: AtomicUsize = AtomicUsize::new(0);
static LIVE_STR: AtomicUsize = AtomicUsize::new(0);
static LIVE_DICT: AtomicUsize = AtomicUsize::new(0);
static LAST_ERROR: Mutex<Option<Vec<u8>>> = Mutex::new(None);
static REPL_SELECTED: AtomicI64 = AtomicI64::new(-1);
static REPL_CURRENT: AtomicI64 = AtomicI64::new(-1);

thread_local! {
    /// 线程归属的 REPL 语句序号：主线程为 -2 表示跟随 light_repl_begin
    static OWN_SLOT: std::cell::Cell<i64> = const { std::cell::Cell::new(-2) };
}

fn current_slot() -> i64 {
    OWN_SLOT.with(|slot| {
        let value = slot.get();
        if value == -2 {
            REPL_CURRENT.load(Ordering::SeqCst)
        } else {
            value
        }
    })
}

fn repl_visible() -> bool {
    let selected = REPL_SELECTED.load(Ordering::SeqCst);
    selected < 0 || current_slot() == selected
}

#[no_mangle]
pub extern "C" fn light_repl_select(line: i64) {
    REPL_SELECTED.store(line, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn light_repl_begin(line: i64) {
    REPL_CURRENT.store(line, Ordering::SeqCst);
}

// 元素标签
pub const TAG_INT: u8 = 0;
pub const TAG_FLOAT: u8 = 1;
pub const TAG_STR: u8 = 2;
pub const TAG_BOOL: u8 = 3;
pub const TAG_NONE: u8 = 4;
pub const TAG_ARRAY: u8 = 5;
pub const TAG_DICT: u8 = 6;
pub const TAG_TUPLE: u8 = 7;

/// 数组/列表/元组运行时表示：长度 + 堆分配缓冲区 + 并行元素标签。
/// 对汇编端透明，汇编只持有 `*mut LightArray` 不透明指针。
#[repr(C)]
pub struct LightArray {
    len: usize,
    data: Box<[i64]>,
    tags: Box<[u8]>,
}

/// 字符串运行时表示：Rust 拥有的字节缓冲区。
#[repr(C)]
pub struct LightStr {
    ptr: *const u8,
    len: usize,
    // 实际数据存储在 Box<[u8]> 中，ptr 指向它；用 `data` 持有所有权
    _data: Box<[u8]>,
}

/// 字典运行时表示：插入有序的 (键, 值) 对，键值均为标签值。
/// 字符串键/值由字典持有（深拷贝）。
#[repr(C)]
pub struct LightDict {
    keys: Vec<(u8, i64)>,
    vals: Vec<(u8, i64)>,
}

struct LightThread {
    handle: Option<JoinHandle<i64>>,
}

type LightThreadTask = unsafe extern "C" fn(i64) -> i64;

static THREADS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// 内部工具：安全失败
// ---------------------------------------------------------------------------
unsafe fn abort(msg: &str) -> ! {
    let _ = writeln!(std::io::stderr(), "[lightrt 内存安全错误] {}", msg);
    std::process::abort();
}

fn make_str(bytes: Vec<u8>) -> *mut LightStr {
    let owned: Box<[u8]> = bytes.into_boxed_slice();
    let s = Box::new(LightStr {
        ptr: owned.as_ptr(),
        len: owned.len(),
        _data: owned,
    });
    LIVE_STR.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(s)
}

fn make_array(data: Vec<i64>, tags: Vec<u8>) -> *mut LightArray {
    debug_assert_eq!(data.len(), tags.len());
    let len = data.len();
    let arr = Box::new(LightArray {
        len,
        data: data.into_boxed_slice(),
        tags: tags.into_boxed_slice(),
    });
    LIVE_ARRAY.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(arr)
}

fn str_clone_raw(s: &LightStr) -> *mut LightStr {
    let owned: Box<[u8]> = s._data.to_vec().into_boxed_slice();
    let c = Box::new(LightStr {
        ptr: owned.as_ptr(),
        len: owned.len(),
        _data: owned,
    });
    LIVE_STR.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(c)
}

/// 深拷贝一个标签值（字符串/容器被复制，调用方获得独立所有权）。
fn clone_val(tag: u8, v: i64) -> i64 {
    match tag {
        TAG_STR => {
            let s = v as *const LightStr;
            if s.is_null() {
                v
            } else {
                let c = unsafe { str_clone_raw(&*s) };
                c as i64
            }
        }
        TAG_ARRAY | TAG_TUPLE => {
            let a = v as *const LightArray;
            if a.is_null() {
                v
            } else {
                array_clone_raw(a) as i64
            }
        }
        TAG_DICT => {
            let d = v as *const LightDict;
            if d.is_null() {
                v
            } else {
                dict_clone_raw(d) as i64
            }
        }
        _ => v,
    }
}

/// 释放一个标签值持有的堆资源（字符串/容器），标量无操作。
fn free_val(tag: u8, v: i64) {
    match tag {
        TAG_STR => {
            let s = v as *mut LightStr;
            if !s.is_null() {
                unsafe { drop(Box::from_raw(s)); }
                LIVE_STR.fetch_sub(1, Ordering::SeqCst);
            }
        }
        TAG_ARRAY | TAG_TUPLE => {
            let a = v as *mut LightArray;
            if !a.is_null() {
                array_free_raw(a);
            }
        }
        TAG_DICT => {
            let d = v as *mut LightDict;
            if !d.is_null() {
                dict_free_raw(d);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 打印
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_print_int(value: i64) {
    if !repl_visible() {
        return;
    }
    print!("{}", value);
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn light_print_float(value: f64) {
    if !repl_visible() {
        return;
    }
    let mut s = format!("{}", value);
    // 保证至少有一个小数点，与 Python 风格一致
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        s.push_str(".0");
    }
    print!("{}", s);
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn light_print_str(ptr: *const u8, len: usize) {
    if !repl_visible() {
        return;
    }
    if ptr.is_null() {
        unsafe { abort("打印空字符串指针"); }
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    // 以 UTF-8 容错方式输出，保证不 panic
    let s = String::from_utf8_lossy(bytes);
    print!("{}", s);
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn light_print_newline() {
    if !repl_visible() {
        return;
    }
    println!();
}

#[no_mangle]
pub extern "C" fn light_print_bool(value: bool) {
    if !repl_visible() {
        return;
    }
    print!("{}", if value { "真" } else { "假" });
    let _ = std::io::stdout().flush();
}

// ---- 标签值格式化 ----

pub fn str_parts(s: *const std::os::raw::c_void) -> (*const u8, usize) {
    if s.is_null() {
        return (std::ptr::null(), 0);
    }
    let s = unsafe { &*(s as *const LightStr) };
    (s.ptr, s.len)
}

#[no_mangle]
pub extern "C" fn light_throw(message: *const LightStr) {
    let bytes = if message.is_null() {
        Vec::new()
    } else {
        unsafe { (*message)._data.to_vec() }
    };
    if let Ok(mut slot) = LAST_ERROR.lock() {
        *slot = Some(bytes);
    }
}

#[no_mangle]
pub extern "C" fn light_take_error() -> *mut LightStr {
    match LAST_ERROR.lock() {
        Ok(mut slot) => slot.take().map(make_str).unwrap_or(std::ptr::null_mut()),
        Err(_) => std::ptr::null_mut(),
    }
}

fn join_thread(address: usize) -> i64 {
    if address == 0 {
        return -1;
    }
    let task = unsafe { Box::from_raw(address as *mut LightThread) };
    match task.handle {
        Some(handle) => match handle.join() {
            Ok(value) => value,
            Err(_) => -1,
        },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn light_thread_spawn(task: LightThreadTask, argument: i64) -> *mut std::os::raw::c_void {
    if task as usize == 0 {
        return std::ptr::null_mut();
    }
    // 记录创建线程时所处的 REPL 语句序号，使线程输出归属正确
    let owner = REPL_CURRENT.load(Ordering::SeqCst);
    let spawned = thread::Builder::new().spawn(move || {
        OWN_SLOT.with(|slot| slot.set(owner));
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { task(argument) })) {
            Ok(value) => value,
            Err(_) => -1,
        }
    });
    let handle = match spawned {
        Ok(handle) => handle,
        Err(_) => return std::ptr::null_mut(),
    };
    let thread = Box::into_raw(Box::new(LightThread { handle: Some(handle) }));
    if let Ok(mut threads) = THREADS.lock() {
        threads.push(thread as usize);
    }
    thread as *mut std::os::raw::c_void
}

#[no_mangle]
pub extern "C" fn light_thread_join(thread: *mut std::os::raw::c_void) -> i64 {
    if thread.is_null() {
        return -1;
    }
    let address = thread as usize;
    let registered = if let Ok(mut threads) = THREADS.lock() {
        let before = threads.len();
        threads.retain(|value| *value != address);
        threads.len() != before
    } else {
        false
    };
    if !registered {
        return -1;
    }
    join_thread(address)
}

#[no_mangle]
pub extern "C" fn light_thread_join_all() -> i64 {
    let addresses = match THREADS.lock() {
        Ok(mut threads) => std::mem::take(&mut *threads),
        Err(_) => return -1,
    };
    let mut failed = 0;
    for address in addresses {
        if join_thread(address) < 0 {
            failed += 1;
        }
    }
    if failed == 0 { 0 } else { -failed }
}

fn str_content(s: *const LightStr) -> String {
    if s.is_null() {
        String::new()
    } else {
        unsafe { String::from_utf8_lossy(&(*s)._data).to_string() }
    }
}

fn fmt_float_value(v: f64) -> String {
    let mut s = format!("{}", v);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        s.push_str(".0");
    }
    s
}

fn fmt_value(tag: u8, v: i64) -> String {
    match tag {
        TAG_INT => v.to_string(),
        TAG_FLOAT => fmt_float_value(f64::from_bits(v as u64)),
        TAG_STR => str_content(v as *const LightStr),
        TAG_BOOL => if v != 0 { "真".to_string() } else { "假".to_string() },
        TAG_NONE => "空".to_string(),
        TAG_ARRAY => fmt_array(v as *const LightArray, false),
        TAG_TUPLE => fmt_array(v as *const LightArray, true),
        TAG_DICT => fmt_dict(v as *const LightDict),
        _ => v.to_string(),
    }
}

fn fmt_array(arr: *const LightArray, tuple: bool) -> String {
    if arr.is_null() {
        return if tuple { "()".to_string() } else { "[]".to_string() };
    }
    let a = unsafe { &*arr };
    let open = if tuple { "(" } else { "[" };
    let close = if tuple { ")" } else { "]" };
    let mut s = String::from(open);
    for i in 0..a.len {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&fmt_value(a.tags[i], a.data[i]));
    }
    s.push_str(close);
    s
}

fn fmt_dict(d: *const LightDict) -> String {
    if d.is_null() {
        return "{}".to_string();
    }
    let dd = unsafe { &*d };
    let mut s = String::from("{");
    for i in 0..dd.keys.len() {
        if i > 0 {
            s.push_str(", ");
        }
        let (kt, kv) = dd.keys[i];
        if kt == TAG_STR {
            s.push('"');
            s.push_str(&fmt_value(kt, kv));
            s.push('"');
        } else {
            s.push_str(&fmt_value(kt, kv));
        }
        s.push_str(": ");
        s.push_str(&fmt_value(dd.vals[i].0, dd.vals[i].1));
    }
    s.push('}');
    s
}

#[no_mangle]
pub extern "C" fn light_print_value(v: i64, tag: i32) {
    if !repl_visible() {
        return;
    }
    print!("{}", fmt_value(tag as u8, v));
    let _ = std::io::stdout().flush();
}

// ---------------------------------------------------------------------------
// 格式化：把格式串中的占位符替换为运行时的值
//   名字列表：NUL 分隔的 UTF-8 片段，与 vals 数组前 name_count 项一一对应
//   位置参数：vals 数组中 name_count 之后的部分，可用 {} 或 {0} 引用
// ---------------------------------------------------------------------------

/// 按格式串生成新字符串；`{{` 与 `}}` 分别输出字面量 `{` 与 `}`。
#[no_mangle]
pub extern "C" fn light_format_v1(
    fmt_ptr: *const u8,
    fmt_len: usize,
    names_ptr: *const u8,
    names_len: usize,
    vals: *const LightArray,
    name_count: usize,
) -> *mut LightStr {
    let format = if fmt_ptr.is_null() {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(fmt_ptr, fmt_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    let names: Vec<String> = if names_ptr.is_null() {
        Vec::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(names_ptr, names_len) };
        bytes
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect()
    };
    let array = if vals.is_null() {
        None
    } else {
        Some(unsafe { &*vals })
    };
    let fetch = |tag: u8, value: i64| -> String { fmt_value(tag, value) };
    let named = |name: &str| -> Option<String> {
        let index = names.iter().position(|item| item == name)?;
        let array = array?;
        if index >= array.len {
            return None;
        }
        Some(fetch(array.tags[index], array.data[index]))
    };
    let positional = |index: usize| -> Option<String> {
        let array = array?;
        let real = name_count + index;
        if real >= array.len {
            return None;
        }
        Some(fetch(array.tags[real], array.data[real]))
    };

    let mut out = String::with_capacity(format.len() + 16);
    let chars: Vec<char> = format.chars().collect();
    let mut index = 0usize;
    let mut auto = 0usize;
    while index < chars.len() {
        let current = chars[index];
        if current == '{' {
            if index + 1 < chars.len() && chars[index + 1] == '{' {
                out.push('{');
                index += 2;
                continue;
            }
            match chars[index..].iter().position(|c| *c == '}') {
                Some(offset) => {
                    let key: String = chars[index + 1..index + offset].iter().collect();
                    let key = key.trim().to_string();
                    let replacement = if key.is_empty() {
                        let value = positional(auto);
                        auto += 1;
                        value
                    } else if key.chars().all(|c| c.is_ascii_digit()) {
                        match key.parse::<usize>() {
                            Ok(position) => positional(position),
                            Err(_) => None,
                        }
                    } else {
                        named(&key)
                    };
                    match replacement {
                        Some(text) => out.push_str(&text),
                        None => {
                            // 找不到对应值时保留占位符原样
                            out.push('{');
                            out.push_str(&key);
                            out.push('}');
                        }
                    }
                    index += offset + 1;
                    continue;
                }
                None => {
                    out.push('{');
                    index += 1;
                    continue;
                }
            }
        }
        if current == '}' && index + 1 < chars.len() && chars[index + 1] == '}' {
            out.push('}');
            index += 2;
            continue;
        }
        out.push(current);
        index += 1;
    }
    make_str(out.into_bytes())
}

#[no_mangle]
pub extern "C" fn light_print_array(arr: *const LightArray) {
    if !repl_visible() {
        return;
    }
    print!("{}", fmt_array(arr, false));
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn light_print_tuple(arr: *const LightArray) {
    if !repl_visible() {
        return;
    }
    print!("{}", fmt_array(arr, true));
    let _ = std::io::stdout().flush();
}

#[no_mangle]
pub extern "C" fn light_print_dict(d: *const LightDict) {
    if !repl_visible() {
        return;
    }
    print!("{}", fmt_dict(d));
    let _ = std::io::stdout().flush();
}

// ---------------------------------------------------------------------------
// 数组：所有访问均带边界检查
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_array_new(len: usize) -> *mut LightArray {
    make_array(vec![0i64; len], vec![TAG_INT; len])
}

#[no_mangle]
pub extern "C" fn light_array_len(arr: *const LightArray) -> usize {
    if arr.is_null() {
        unsafe { abort("取长度：数组指针为空"); }
    }
    unsafe { (*arr).len }
}

#[no_mangle]
pub extern "C" fn light_array_get(arr: *mut LightArray, index: usize) -> i64 {
    if arr.is_null() {
        unsafe { abort("读取：数组指针为空"); }
    }
    let a = unsafe { &*arr };
    if index >= a.len {
        unsafe {
            abort(&format!(
                "数组越界读取：索引 {} >= 长度 {}",
                index, a.len
            ));
        }
    }
    a.data[index]
}

#[no_mangle]
pub extern "C" fn light_array_get_tag(arr: *mut LightArray, index: usize) -> i32 {
    if arr.is_null() {
        unsafe { abort("读取：数组指针为空"); }
    }
    let a = unsafe { &*arr };
    if index >= a.len {
        unsafe {
            abort(&format!(
                "数组越界读取：索引 {} >= 长度 {}",
                index, a.len
            ));
        }
    }
    a.tags[index] as i32
}

#[no_mangle]
pub extern "C" fn light_array_set(arr: *mut LightArray, index: usize, value: i64) {
    light_array_set_tag(arr, index, TAG_INT as i32, value);
}

#[no_mangle]
pub extern "C" fn light_array_set_tag(arr: *mut LightArray, index: usize, tag: i32, value: i64) {
    if arr.is_null() {
        unsafe { abort("写入：数组指针为空"); }
    }
    let a = unsafe { &mut *arr };
    if index >= a.len {
        unsafe {
            abort(&format!(
                "数组越界写入：索引 {} >= 长度 {}",
                index, a.len
            ));
        }
    }
    // 覆盖旧元素：释放其持有的堆资源
    let old_tag = a.tags[index];
    let old_val = a.data[index];
    if old_tag != tag as u8 || old_tag != TAG_INT {
        free_val(old_tag, old_val);
    }
    a.data[index] = value;
    a.tags[index] = tag as u8;
}

#[no_mangle]
pub extern "C" fn light_array_set_copy_tag(arr: *mut LightArray, index: usize, tag: i32, value: i64) {
    light_array_set_tag(arr, index, tag, clone_val(tag as u8, value));
}

#[no_mangle]
pub extern "C" fn light_array_push(arr: *mut LightArray, value: i64) {
    light_array_push_tag(arr, TAG_INT as i32, value);
}

#[no_mangle]
pub extern "C" fn light_array_push_tag(arr: *mut LightArray, tag: i32, value: i64) {
    if arr.is_null() {
        unsafe { abort("追加：数组指针为空"); }
    }
    let a = unsafe { &mut *arr };
    let mut v: Vec<i64> = std::mem::take(&mut a.data).into_vec();
    let mut t: Vec<u8> = std::mem::take(&mut a.tags).into_vec();
    v.push(value);
    t.push(tag as u8);
    a.data = v.into_boxed_slice();
    a.tags = t.into_boxed_slice();
    a.len = a.data.len();
}

#[no_mangle]
pub extern "C" fn light_array_pop(arr: *mut LightArray) -> i64 {
    // 弹出的元素所有权转移给调用方（不释放）
    if arr.is_null() {
        unsafe { abort("弹出：数组指针为空"); }
    }
    let a = unsafe { &mut *arr };
    let mut v: Vec<i64> = std::mem::take(&mut a.data).into_vec();
    let mut t: Vec<u8> = std::mem::take(&mut a.tags).into_vec();
    let result = match v.pop() {
        Some(val) => {
            t.pop();
            val
        }
        None => unsafe { abort("弹出：数组为空"); },
    };
    a.data = v.into_boxed_slice();
    a.tags = t.into_boxed_slice();
    a.len = a.data.len();
    result
}

fn array_free_raw(arr: *mut LightArray) {
    if arr.is_null() {
        return;
    }
    let a = unsafe { Box::from_raw(arr) };
    for i in 0..a.len {
        free_val(a.tags[i], a.data[i]);
    }
    LIVE_ARRAY.fetch_sub(1, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn light_array_free(arr: *mut LightArray) {
    array_free_raw(arr);
}

/// 只释放数组容器，不释放元素：元素为借用引用时使用（如格式化参数表）
#[no_mangle]
pub extern "C" fn light_array_free_shallow(arr: *mut LightArray) {
    if arr.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(arr) });
    LIVE_ARRAY.fetch_sub(1, Ordering::SeqCst);
}

fn array_clone_raw(arr: *const LightArray) -> *mut LightArray {
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    let a = unsafe { &*arr };
    let mut data = Vec::with_capacity(a.len);
    for i in 0..a.len {
        data.push(clone_val(a.tags[i], a.data[i]));
    }
    make_array(data, a.tags.to_vec())
}

#[no_mangle]
pub extern "C" fn light_array_clone(arr: *const LightArray) -> *mut LightArray {
    array_clone_raw(arr)
}

// ---------------------------------------------------------------------------
// 字符串：由 Rust 管理内存，汇编端只持有不透明句柄
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_str_from_utf8(ptr: *const u8, len: usize) -> *mut LightStr {
    if ptr.is_null() {
        unsafe { abort("创建字符串：源指针为空"); }
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    // 复制一份由 Rust 持有，汇编端的原始数据可安全释放
    let owned: Box<[u8]> = bytes.to_vec().into_boxed_slice();
    let s = Box::new(LightStr {
        ptr: owned.as_ptr(),
        len: owned.len(),
        _data: owned,
    });
    LIVE_STR.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(s)
}

#[no_mangle]
pub extern "C" fn light_str_clone(s: *const LightStr) -> *mut LightStr {
    if s.is_null() {
        unsafe { abort("复制字符串：指针为空"); }
    }
    unsafe { str_clone_raw(&*s) }
}

#[no_mangle]
pub extern "C" fn light_str_ptr(s: *const LightStr) -> *const u8 {
    if s.is_null() {
        return std::ptr::null();
    }
    unsafe { (*s).ptr }
}

#[no_mangle]
pub extern "C" fn light_str_len(s: *const LightStr) -> usize {
    if s.is_null() {
        return 0;
    }
    unsafe { (*s).len }
}

#[no_mangle]
pub extern "C" fn light_str_concat(a: *const LightStr, b: *const LightStr) -> *mut LightStr {
    if a.is_null() || b.is_null() {
        unsafe { abort("字符串拼接：参数为空"); }
    }
    let (sa, sb) = unsafe { (&*a, &*b) };
    let mut out = Vec::with_capacity(sa.len + sb.len);
    out.extend_from_slice(&sa._data);
    out.extend_from_slice(&sb._data);
    let owned = out.into_boxed_slice();
    let s = Box::new(LightStr {
        ptr: owned.as_ptr(),
        len: owned.len(),
        _data: owned,
    });
    LIVE_STR.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(s)
}

#[no_mangle]
pub extern "C" fn light_str_free(s: *mut LightStr) {
    if s.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(s);
    }
    LIVE_STR.fetch_sub(1, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// 数组扩展：追加、弹出、范围
// ---------------------------------------------------------------------------

/// 创建 [0, 1, ..., n-1] 的数组
#[no_mangle]
pub extern "C" fn light_range(n: i64) -> *mut LightArray {
    if n < 0 {
        return light_array_new(0);
    }
    let len = n as usize;
    let data: Vec<i64> = (0..n).collect();
    make_array(data, vec![TAG_INT; len])
}

// ---------------------------------------------------------------------------
// 字符串扩展：索引、类型转换
// ---------------------------------------------------------------------------
/// 返回字符串第 i 个字节的值（0-255），越界安全终止
#[no_mangle]
pub extern "C" fn light_str_index(s: *const LightStr, index: i64) -> i64 {
    if s.is_null() {
        unsafe { abort("字符串索引：指针为空"); }
    }
    let s = unsafe { &*s };
    if index < 0 || (index as usize) >= s._data.len() {
        unsafe {
            abort(&format!(
                "字符串索引越界：索引 {} >= 长度 {}",
                index,
                s._data.len()
            ));
        }
    }
    s._data[index as usize] as i64
}

#[no_mangle]
pub extern "C" fn light_str_to_int(s: *const LightStr) -> i64 {
    if s.is_null() {
        unsafe { abort("转整数：字符串指针为空"); }
    }
    let s = unsafe { &*s };
    let text = String::from_utf8_lossy(&s._data);
    let trimmed = text.trim();
    match trimmed.parse::<i64>() {
        Ok(v) => v,
        Err(_) => trimmed.parse::<f64>().map(|f| f as i64).unwrap_or(0),
    }
}

#[no_mangle]
pub extern "C" fn light_str_to_float(s: *const LightStr) -> f64 {
    if s.is_null() {
        unsafe { abort("转浮点：字符串指针为空"); }
    }
    let s = unsafe { &*s };
    let text = String::from_utf8_lossy(&s._data);
    text.trim().parse::<f64>().unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn light_int_to_str(value: i64) -> *mut LightStr {
    make_str(value.to_string().into_bytes())
}

#[no_mangle]
pub extern "C" fn light_float_to_str(value: f64) -> *mut LightStr {
    // 格式化浮点，去掉多余的零但保留可读性
    let s = if value.fract() == 0.0 {
        format!("{:.1}", value)
    } else {
        format!("{}", value)
    };
    make_str(s.into_bytes())
}

#[no_mangle]
pub extern "C" fn light_bool_to_str(value: bool) -> *mut LightStr {
    make_str(if value { "真" } else { "假" }.to_string().into_bytes())
}

// ---------------------------------------------------------------------------
// 输入
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_input() -> *mut LightStr {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            // 去掉末尾换行
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            make_str(line.into_bytes())
        }
        Err(_) => make_str(Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// 数值辅助
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_pow_int(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

#[no_mangle]
pub extern "C" fn light_floor_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        unsafe { abort("整除：除数为零"); }
    }
    // Python 风格向下取整
    let q = a / b;
    let r = a % b;
    if r != 0 && ((r > 0) != (b > 0)) {
        q - 1
    } else {
        q
    }
}

// ---------------------------------------------------------------------------
// 内存统计（供编译器/调试器查询）
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn lightrt_live_arrays() -> usize {
    LIVE_ARRAY.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn lightrt_live_strings() -> usize {
    LIVE_STR.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn lightrt_live_dicts() -> usize {
    LIVE_DICT.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// 字典
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_dict_new() -> *mut LightDict {
    let d = Box::new(LightDict {
        keys: Vec::new(),
        vals: Vec::new(),
    });
    LIVE_DICT.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(d)
}

#[no_mangle]
pub extern "C" fn light_dict_len(d: *const LightDict) -> usize {
    if d.is_null() {
        unsafe { abort("取长度：字典指针为空"); }
    }
    unsafe { (*d).keys.len() }
}

fn dict_find(d: &LightDict, ktag: u8, k: i64) -> Option<usize> {
    for (i, (t, v)) in d.keys.iter().enumerate() {
        if *t != ktag {
            continue;
        }
        let eq = match ktag {
            TAG_STR => {
                let a = *v as *const LightStr;
                let b = k as *const LightStr;
                !a.is_null() && !b.is_null()
                    && unsafe { (*a)._data.as_ref() } == unsafe { (*b)._data.as_ref() }
            }
            TAG_FLOAT => f64::from_bits(*v as u64) == f64::from_bits(k as u64),
            _ => *v == k,
        };
        if eq {
            return Some(i);
        }
    }
    None
}

#[no_mangle]
pub extern "C" fn light_dict_has(d: *const LightDict, ktag: i32, k: i64) -> bool {
    if d.is_null() {
        unsafe { abort("取出：字典指针为空"); }
    }
    let dd = unsafe { &*d };
    dict_find(dd, ktag as u8, k).is_some()
}

#[no_mangle]
pub extern "C" fn light_dict_get(d: *const LightDict, ktag: i32, k: i64) -> i64 {
    if d.is_null() {
        unsafe { abort("取出：字典指针为空"); }
    }
    let dd = unsafe { &*d };
    match dict_find(dd, ktag as u8, k) {
        Some(i) => dd.vals[i].1,
        None => unsafe {
            abort(&format!(
                "字典键不存在：{}",
                fmt_value(ktag as u8, k)
            ));
        },
    }
}

#[no_mangle]
pub extern "C" fn light_dict_get_tag(d: *const LightDict, ktag: i32, k: i64) -> i32 {
    if d.is_null() {
        unsafe { abort("取出：字典指针为空"); }
    }
    let dd = unsafe { &*d };
    match dict_find(dd, ktag as u8, k) {
        Some(i) => dd.vals[i].0 as i32,
        None => unsafe {
            abort(&format!(
                "字典键不存在：{}",
                fmt_value(ktag as u8, k)
            ));
        },
    }
}

#[no_mangle]
pub extern "C" fn light_dict_set(d: *mut LightDict, ktag: i32, k: i64, vtag: i32, v: i64) {
    if d.is_null() {
        unsafe { abort("写入：字典指针为空"); }
    }
    let dd = unsafe { &mut *d };
    // 键值以深拷贝持有，保证调用方仍保有自身副本
    let kc = clone_val(ktag as u8, k);
    let vc = clone_val(vtag as u8, v);
    if let Some(i) = dict_find(dd, ktag as u8, kc) {
        free_val(dd.keys[i].0, dd.keys[i].1);
        free_val(dd.vals[i].0, dd.vals[i].1);
        dd.keys[i] = (ktag as u8, kc);
        dd.vals[i] = (vtag as u8, vc);
    } else {
        dd.keys.push((ktag as u8, kc));
        dd.vals.push((vtag as u8, vc));
    }
}

/// 返回键数组（深拷贝），供 for-in 遍历；调用方负责释放。
#[no_mangle]
pub extern "C" fn light_dict_keys(d: *const LightDict) -> *mut LightArray {
    if d.is_null() {
        unsafe { abort("取键：字典指针为空"); }
    }
    let dd = unsafe { &*d };
    let mut data = Vec::with_capacity(dd.keys.len());
    let mut tags = Vec::with_capacity(dd.keys.len());
    for (t, v) in &dd.keys {
        data.push(clone_val(*t, *v));
        tags.push(*t);
    }
    make_array(data, tags)
}

fn dict_clone_raw(d: *const LightDict) -> *mut LightDict {
    if d.is_null() {
        return std::ptr::null_mut();
    }
    let dd = unsafe { &*d };
    let mut keys = Vec::with_capacity(dd.keys.len());
    let mut vals = Vec::with_capacity(dd.vals.len());
    for (t, v) in &dd.keys {
        keys.push((*t, clone_val(*t, *v)));
    }
    for (t, v) in &dd.vals {
        vals.push((*t, clone_val(*t, *v)));
    }
    let nd = Box::new(LightDict { keys, vals });
    LIVE_DICT.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(nd)
}

#[no_mangle]
pub extern "C" fn light_dict_clone(d: *const LightDict) -> *mut LightDict {
    dict_clone_raw(d)
}

fn dict_free_raw(d: *mut LightDict) {
    if d.is_null() {
        return;
    }
    let dd = unsafe { Box::from_raw(d) };
    for (t, v) in &dd.keys {
        free_val(*t, *v);
    }
    for (t, v) in &dd.vals {
        free_val(*t, *v);
    }
    LIVE_DICT.fetch_sub(1, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn light_dict_free(d: *mut LightDict) {
    dict_free_raw(d);
}

// ---------------------------------------------------------------------------
// 数学模块
// ---------------------------------------------------------------------------

/// 将字节值(0-255)转为单字符字符串
#[no_mangle]
pub extern "C" fn light_byte_to_char(byte: i64) -> *mut LightStr {
    let b = (byte as u8) as u8;
    make_str(vec![b])
}

// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn light_math_sin(x: f64) -> f64 { x.sin() }

#[no_mangle]
pub extern "C" fn light_math_cos(x: f64) -> f64 { x.cos() }

#[no_mangle]
pub extern "C" fn light_math_tan(x: f64) -> f64 { x.tan() }

#[no_mangle]
pub extern "C" fn light_math_sqrt(x: f64) -> f64 { x.sqrt() }

#[no_mangle]
pub extern "C" fn light_math_log(x: f64) -> f64 { x.ln() }

#[no_mangle]
pub extern "C" fn light_math_log10(x: f64) -> f64 { x.log10() }

#[no_mangle]
pub extern "C" fn light_math_pow(x: f64, y: f64) -> f64 { x.powf(y) }

#[no_mangle]
pub extern "C" fn light_math_pi() -> f64 { std::f64::consts::PI }

#[no_mangle]
pub extern "C" fn light_math_e() -> f64 { std::f64::consts::E }

#[no_mangle]
pub extern "C" fn light_math_ceil(x: f64) -> f64 { x.ceil() }

#[no_mangle]
pub extern "C" fn light_math_floor(x: f64) -> f64 { x.floor() }

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[no_mangle]
pub extern "C" fn light_math_random() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    (h.finish() % 1000000) as f64 / 1000000.0
}

#[no_mangle]
pub extern "C" fn light_math_random_int(min: i64, max: i64) -> i64 {
    if min >= max { return min; }
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    min + (h.finish() as i64 % (max - min))
}

// ---------------------------------------------------------------------------
// 字符串模块
// ---------------------------------------------------------------------------
/// 返回子串 sub 在 s 中首次出现的位置，未找到返回 -1
#[no_mangle]
pub extern "C" fn light_str_find(s: *const LightStr, sub: *const LightStr) -> i64 {
    if s.is_null() || sub.is_null() {
        unsafe { abort("查找：参数为空"); }
    }
    let s = unsafe { &*s };
    let sub = unsafe { &*sub };
    let haystack = String::from_utf8_lossy(&s._data);
    let needle = String::from_utf8_lossy(&sub._data);
    match haystack.find(&*needle) {
        Some(pos) => pos as i64,
        None => -1,
    }
}

/// 判断 s 是否包含子串 sub
#[no_mangle]
pub extern "C" fn light_str_contains(s: *const LightStr, sub: *const LightStr) -> bool {
    if s.is_null() || sub.is_null() {
        unsafe { abort("包含：参数为空"); }
    }
    let s = unsafe { &*s };
    let sub = unsafe { &*sub };
    let haystack = String::from_utf8_lossy(&s._data);
    let needle = String::from_utf8_lossy(&sub._data);
    haystack.contains(&*needle)
}

/// 替换 s 中所有 old 为 new
#[no_mangle]
pub extern "C" fn light_str_replace(s: *const LightStr, old: *const LightStr, new: *const LightStr) -> *mut LightStr {
    if s.is_null() || old.is_null() || new.is_null() {
        unsafe { abort("替换：参数为空"); }
    }
    let s = unsafe { &*s };
    let old = unsafe { &*old };
    let new = unsafe { &*new };
    let text = String::from_utf8_lossy(&s._data).to_string();
    let old_text = String::from_utf8_lossy(&old._data).to_string();
    let new_text = String::from_utf8_lossy(&new._data).to_string();
    let result = text.replace(&old_text, &new_text);
    make_str(result.into_bytes())
}

/// 按分隔符分割字符串，返回字符串数组（元素标签为 TAG_STR，由数组持有）
#[no_mangle]
pub extern "C" fn light_str_split(s: *const LightStr, sep: *const LightStr) -> *mut LightArray {
    if s.is_null() || sep.is_null() {
        unsafe { abort("分割：参数为空"); }
    }
    let s = unsafe { &*s };
    let sep = unsafe { &*sep };
    let text = String::from_utf8_lossy(&s._data).to_string();
    let sep_text = String::from_utf8_lossy(&sep._data).to_string();
    let mut data = Vec::new();
    let mut tags = Vec::new();
    for part in text.split(&sep_text) {
        let ptr = make_str(part.to_string().into_bytes());
        data.push(ptr as i64);
        tags.push(TAG_STR);
    }
    make_array(data, tags)
}

/// 将字符串数组（i64 指针数组）用分隔符连接成一个字符串
#[no_mangle]
pub extern "C" fn light_str_join(arr: *const LightArray, sep: *const LightStr) -> *mut LightStr {
    if arr.is_null() || sep.is_null() {
        unsafe { abort("合并：参数为空"); }
    }
    let a = unsafe { &*arr };
    let sep = unsafe { &*sep };
    let sep_text = String::from_utf8_lossy(&sep._data).to_string();
    let mut result = String::new();
    for i in 0..a.len {
        let ptr = a.data[i] as *const LightStr;
        if !ptr.is_null() {
            let s = unsafe { &*ptr };
            let part = String::from_utf8_lossy(&s._data);
            result.push_str(&part);
        }
        if i + 1 < a.len {
            result.push_str(&sep_text);
        }
    }
    make_str(result.into_bytes())
}

/// 转大写
#[no_mangle]
pub extern "C" fn light_str_upper(s: *const LightStr) -> *mut LightStr {
    if s.is_null() { unsafe { abort("大写：参数为空"); } }
    let s = unsafe { &*s };
    let text = String::from_utf8_lossy(&s._data);
    let upper: String = text.chars().flat_map(|c| c.to_uppercase()).collect();
    make_str(upper.into_bytes())
}

/// 转小写
#[no_mangle]
pub extern "C" fn light_str_lower(s: *const LightStr) -> *mut LightStr {
    if s.is_null() { unsafe { abort("小写：参数为空"); } }
    let s = unsafe { &*s };
    let text = String::from_utf8_lossy(&s._data);
    let lower: String = text.chars().flat_map(|c| c.to_lowercase()).collect();
    make_str(lower.into_bytes())
}

/// 去除首尾空白
#[no_mangle]
pub extern "C" fn light_str_trim(s: *const LightStr) -> *mut LightStr {
    if s.is_null() { unsafe { abort("去空白：参数为空"); } }
    let s = unsafe { &*s };
    let text = String::from_utf8_lossy(&s._data);
    make_str(text.trim().to_string().into_bytes())
}

/// 截取子串 [start, end)（按字节索引）
#[no_mangle]
pub extern "C" fn light_str_substr(s: *const LightStr, start: i64, end: i64) -> *mut LightStr {
    if s.is_null() { unsafe { abort("切片：参数为空"); } }
    let s = unsafe { &*s };
    let len = s._data.len();
    let st = start.max(0) as usize;
    let en = (end.max(0) as usize).min(len);
    if st >= en {
        return make_str(Vec::new());
    }
    make_str(s._data[st..en].to_vec())
}

// ---------------------------------------------------------------------------
// 数组扩展模块
// ---------------------------------------------------------------------------
/// 判断数组是否包含某个值（字符串按内容比较）
#[no_mangle]
pub extern "C" fn light_array_contains(arr: *const LightArray, val: i64) -> bool {
    if arr.is_null() { unsafe { abort("包含：数组指针为空"); } }
    let a = unsafe { &*arr };
    for i in 0..a.len {
        let t = a.tags[i];
        let eq = match t {
            TAG_STR => {
                let x = a.data[i] as *const LightStr;
                let y = val as *const LightStr;
                !x.is_null() && !y.is_null()
                    && unsafe { (*x)._data.as_ref() } == unsafe { (*y)._data.as_ref() }
            }
            TAG_FLOAT => f64::from_bits(a.data[i] as u64) == f64::from_bits(val as u64),
            _ => a.data[i] == val,
        };
        if eq {
            return true;
        }
    }
    false
}

/// 返回值在数组中首次出现的索引，未找到返回 -1
#[no_mangle]
pub extern "C" fn light_array_index(arr: *const LightArray, val: i64) -> i64 {
    if arr.is_null() { unsafe { abort("索引：数组指针为空"); } }
    let a = unsafe { &*arr };
    for i in 0..a.len {
        let t = a.tags[i];
        let eq = match t {
            TAG_STR => {
                let x = a.data[i] as *const LightStr;
                let y = val as *const LightStr;
                !x.is_null() && !y.is_null()
                    && unsafe { (*x)._data.as_ref() } == unsafe { (*y)._data.as_ref() }
            }
            TAG_FLOAT => f64::from_bits(a.data[i] as u64) == f64::from_bits(val as u64),
            _ => a.data[i] == val,
        };
        if eq {
            return i as i64;
        }
    }
    -1
}

/// 反转数组，返回新数组（字符串/容器元素深拷贝）
#[no_mangle]
pub extern "C" fn light_array_reverse(arr: *const LightArray) -> *mut LightArray {
    if arr.is_null() { unsafe { abort("反转：数组指针为空"); } }
    let a = unsafe { &*arr };
    let mut data: Vec<i64> = a.data.to_vec();
    let mut tags: Vec<u8> = a.tags.to_vec();
    data.reverse();
    tags.reverse();
    for i in 0..data.len() {
        data[i] = clone_val(tags[i], data[i]);
    }
    make_array(data, tags)
}

/// 排序数组（升序），原地修改（按原始 i64 值比较）
#[no_mangle]
pub extern "C" fn light_array_sort(arr: *mut LightArray) {
    if arr.is_null() { unsafe { abort("排序：数组指针为空"); } }
    let a = unsafe { &mut *arr };
    let v: Vec<i64> = std::mem::take(&mut a.data).into_vec();
    let t: Vec<u8> = std::mem::take(&mut a.tags).into_vec();
    let mut pairs: Vec<(i64, u8)> = v.into_iter().zip(t).collect();
    pairs.sort_by_key(|(x, _)| *x);
    let mut v2 = Vec::with_capacity(pairs.len());
    let mut t2 = Vec::with_capacity(pairs.len());
    for (x, tag) in pairs {
        v2.push(x);
        t2.push(tag);
    }
    a.data = v2.into_boxed_slice();
    a.tags = t2.into_boxed_slice();
}

/// 截取子数组 [start, end)，返回新数组
#[no_mangle]
pub extern "C" fn light_array_slice(arr: *const LightArray, start: i64, end: i64) -> *mut LightArray {
    if arr.is_null() { unsafe { abort("切片：数组指针为空"); } }
    let a = unsafe { &*arr };
    let st = start.max(0) as usize;
    let en = (end.max(0) as usize).min(a.len);
    if st >= en {
        return light_array_new(0);
    }
    let mut data: Vec<i64> = a.data[st..en].to_vec();
    let tags: Vec<u8> = a.tags[st..en].to_vec();
    for i in 0..data.len() {
        data[i] = clone_val(tags[i], data[i]);
    }
    make_array(data, tags)
}

/// 连接两个数组，返回新数组
#[no_mangle]
pub extern "C" fn light_array_concat(a: *const LightArray, b: *const LightArray) -> *mut LightArray {
    if a.is_null() || b.is_null() { unsafe { abort("连接：数组指针为空"); } }
    let a = unsafe { &*a };
    let b = unsafe { &*b };
    let mut data: Vec<i64> = Vec::with_capacity(a.len + b.len);
    let mut tags: Vec<u8> = Vec::with_capacity(a.len + b.len);
    for i in 0..a.len {
        data.push(clone_val(a.tags[i], a.data[i]));
        tags.push(a.tags[i]);
    }
    for i in 0..b.len {
        data.push(clone_val(b.tags[i], b.data[i]));
        tags.push(b.tags[i]);
    }
    make_array(data, tags)
}

// ---------------------------------------------------------------------------
// 文件模块
// ---------------------------------------------------------------------------
/// 读取文件全部内容为字符串
#[no_mangle]
pub extern "C" fn light_file_read(path: *const LightStr) -> *mut LightStr {
    if path.is_null() { unsafe { abort("读取文件：路径为空"); } }
    let p = unsafe { &*path };
    let pstr = String::from_utf8_lossy(&p._data);
    match std::fs::read_to_string(&*pstr) {
        Ok(content) => make_str(content.into_bytes()),
        Err(_) => make_str(Vec::new()),
    }
}

/// 写入内容到文件（覆盖）
#[no_mangle]
pub extern "C" fn light_file_write(path: *const LightStr, content: *const LightStr) -> bool {
    if path.is_null() || content.is_null() { unsafe { abort("写入文件：参数为空"); } }
    let p = unsafe { &*path };
    let c = unsafe { &*content };
    let pstr = String::from_utf8_lossy(&p._data);
    let cstr = String::from_utf8_lossy(&c._data);
    std::fs::write(&*pstr, &*cstr).is_ok()
}

/// 追加内容到文件
#[no_mangle]
pub extern "C" fn light_file_append(path: *const LightStr, content: *const LightStr) -> bool {
    if path.is_null() || content.is_null() { unsafe { abort("追加写入：参数为空"); } }
    let p = unsafe { &*path };
    let c = unsafe { &*content };
    let pstr = String::from_utf8_lossy(&p._data);
    let cstr = String::from_utf8_lossy(&c._data);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*pstr)
        .and_then(|mut f| f.write_all(cstr.as_bytes()))
        .is_ok()
}

/// 判断文件是否存在
#[no_mangle]
pub extern "C" fn light_file_exists(path: *const LightStr) -> bool {
    if path.is_null() { unsafe { abort("文件存在：路径为空"); } }
    let p = unsafe { &*path };
    let pstr = String::from_utf8_lossy(&p._data);
    std::path::Path::new(&*pstr).exists()
}

// ---------------------------------------------------------------------------
// 系统模块
// ---------------------------------------------------------------------------
/// 执行系统命令，返回输出的字符串
#[no_mangle]
pub extern "C" fn light_sys_exec(cmd: *const LightStr) -> *mut LightStr {
    if cmd.is_null() { unsafe { abort("执行命令：参数为空"); } }
    let c = unsafe { &*cmd };
    let cstr = String::from_utf8_lossy(&c._data);
    match std::process::Command::new("sh").arg("-c").arg(&*cstr).output() {
        Ok(out) => make_str(out.stdout),
        Err(_) => make_str(Vec::new()),
    }
}

/// 获取环境变量，不存在返回空字符串
#[no_mangle]
pub extern "C" fn light_sys_getenv(name: *const LightStr) -> *mut LightStr {
    if name.is_null() { unsafe { abort("环境变量：参数为空"); } }
    let n = unsafe { &*name };
    let nstr = String::from_utf8_lossy(&n._data);
    match std::env::var(&*nstr) {
        Ok(val) => make_str(val.into_bytes()),
        Err(_) => make_str(Vec::new()),
    }
}

/// 退出程序
#[no_mangle]
pub extern "C" fn light_sys_exit(code: i64) -> ! {
    std::process::exit(code as i32)
}