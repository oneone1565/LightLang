// Light 原生模块的类型安全桥接层。
//
// 零依赖：只用 `extern "C"` 声明包住 lightrt 的 ABI，因此可以被
// `nlightc` 直接内联进任意 `.rs` 原生模块，不需要额外的 Cargo 依赖，
// 也不会引入第二份 std 造成链接期符号冲突。
//
// 用法：nlightc 会自动把本文件预置到你的模块里，直接写
//
// ```ignore
// use light_ffi::*;
//
// #[no_mangle]
// pub extern "C" fn greet(name: &LightStr) -> LightStr {
//     LightStr::new(&format!("你好，{}！", name.as_str()))
// }
// ```
//
// 值的表示：高 8 位是标签，低 56 位是载荷（整数直接存值，字符串/容器存指针）。

// ---------------------------------------------------------------- 标签

/// 运行期类型标签，与 lightrt 的编码一一对应。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    Int = 0,
    Float = 1,
    Str = 2,
    Bool = 3,
    None = 4,
    List = 5,
    Dict = 6,
    Tuple = 7,
}

impl Tag {
    #[inline]
    pub fn from_i32(v: i32) -> Tag {
        match v {
            1 => Tag::Float,
            2 => Tag::Str,
            3 => Tag::Bool,
            4 => Tag::None,
            5 => Tag::List,
            6 => Tag::Dict,
            7 => Tag::Tuple,
            _ => Tag::Int,
        }
    }

    #[inline]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

/// 一个带标签的动态值在 ABI 上的表示。
pub type LightValue = i64;

#[inline]
const fn pack(tag: Tag, payload: i64) -> LightValue {
    ((tag.as_i32() as i64) << 56) | (payload & 0x00FF_FFFF_FFFF_FFFF)
}

#[inline]
pub fn tag_of(v: LightValue) -> Tag {
    Tag::from_i32(((v as u64) >> 56) as i32)
}

#[inline]
pub fn untag(v: LightValue) -> i64 {
    v & 0x00FF_FFFF_FFFF_FFFF
}

// ---------------------------------------------------------------- ABI 声明

extern "C" {
    fn light_tagged(value: i64, tag: i32) -> i64;
    fn light_untag(value: i64) -> i64;
    fn light_tag_of(value: i64) -> i32;
    fn light_dynamic_len(value: i64) -> i64;
    fn light_dynamic_get(base: i64, index: i64) -> i64;
    fn light_dynamic_to_text(value: i64, tag: i32) -> *mut c_void_alias;

    fn light_str_from_utf8(ptr: *const u8, len: usize) -> *mut c_void_alias;
    fn light_str_ptr(s: *const c_void_alias) -> *const u8;
    fn light_str_len(s: *const c_void_alias) -> usize;
    fn light_str_free(s: *mut c_void_alias);
    fn light_str_dup(s: *const c_void_alias) -> *mut c_void_alias;
    fn light_str_concat(a: *const c_void_alias, b: *const c_void_alias) -> *mut c_void_alias;

    fn light_array_new(len: usize) -> *mut c_void_alias;
    fn light_array_len(arr: *const c_void_alias) -> usize;
    fn light_array_get(arr: *mut c_void_alias, index: usize) -> i64;
    fn light_array_get_tag(arr: *mut c_void_alias, index: usize) -> i32;
    fn light_array_set_tag(arr: *mut c_void_alias, index: usize, tag: i32, value: i64);
    fn light_array_push(arr: *mut c_void_alias, value: i64);
    fn light_array_push_tag(arr: *mut c_void_alias, tag: i32, value: i64);
    fn light_array_free(arr: *mut c_void_alias);

    fn light_int_to_str(value: i64) -> *mut c_void_alias;
    fn light_bool_to_str(value: i64) -> *mut c_void_alias;
    fn light_float_to_str(value: f64) -> *mut c_void_alias;
    fn light_str_to_int(s: *const c_void_alias) -> i64;
    fn light_str_to_float(s: *const c_void_alias) -> f64;
}

#[allow(non_camel_case_types)]
type c_void_alias = core::ffi::c_void;

// ---------------------------------------------------------------- 字符串

/// 借用的字符串，不参与释放。
#[derive(Clone, Copy)]
pub struct LightStrRef(*const c_void_alias);

impl LightStrRef {
    /// # Safety
    /// 指针必须来自 lightrt 的字符串函数且在调用期间有效。
    #[inline]
    pub unsafe fn from_ptr(p: *const c_void_alias) -> Option<LightStrRef> {
        if p.is_null() {
            None
        } else {
            Some(LightStrRef(p))
        }
    }

    /// 从标签值里取出字符串引用；不是字符串时返回 None。
    #[inline]
    pub fn from_value(v: LightValue) -> Option<LightStrRef> {
        if tag_of(v) != Tag::Str {
            return None;
        }
        unsafe { LightStrRef::from_ptr(untag(v) as *const c_void_alias) }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe {
            let data = light_str_ptr(self.0);
            let len = light_str_len(self.0);
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe { light_str_len(self.0) }
    }

    /// 裸结构指针，供收 `*const c_void` 的接口使用。
    #[inline]
    pub fn as_ptr(&self) -> *const c_void_alias {
        self.0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 取字节内容，文本不是合法 UTF-8 时不会 panic。
    pub fn to_string_lossy(&self) -> String {
        unsafe {
            let data = light_str_ptr(self.0);
            let len = light_str_len(self.0);
            String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
        }
    }

    /// 复制一份自己持有。
    pub fn to_owned(&self) -> LightStr {
        unsafe { LightStr(light_str_dup(self.0)) }
    }
}

impl std::fmt::Debug for LightStrRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightStrRef({:?})", self.to_string_lossy())
    }
}

/// 自己持有的字符串，析构时释放。
pub struct LightStr(*mut c_void_alias);

impl LightStr {
    /// 从 Rust 字符串新建一个 lightrt 字符串。
    pub fn new(s: &str) -> LightStr {
        unsafe { LightStr(light_str_from_utf8(s.as_ptr(), s.len())) }
    }

    /// 接管一个 lightrt 返回的字符串指针（`from_utf8`/`dup`/`concat` 的返回值）。
    ///
    /// # Safety
    /// 指针必须来自 lightrt，且调用方放弃其所有权。
    #[inline]
    pub unsafe fn from_raw(p: *mut c_void_alias) -> Option<LightStr> {
        if p.is_null() {
            None
        } else {
            Some(LightStr(p))
        }
    }

    #[inline]
    pub fn as_ref(&self) -> LightStrRef {
        LightStrRef(self.0)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe {
            let data = light_str_ptr(self.0);
            let len = light_str_len(self.0);
            let slice = std::slice::from_raw_parts(data, len);
            std::str::from_utf8_unchecked(slice)
        }
    }

    #[inline]
    pub fn as_str_lossy(&self) -> String {
        self.as_ref().to_string_lossy()
    }

    /// 交出所有权，返回**裸指针**。
    /// 原生函数 `-> 字符串` 时用这个：Light 侧按 LightStr* 直接接收，不看标签。
    #[inline]
    pub fn into_raw(self) -> *mut c_void_alias {
        let p = self.0;
        std::mem::forget(self);
        p
    }

    /// 交出所有权，返回**带标签的值**。
    /// 放进数组/字典元素、或交给按标签分派的接口时用这个。
    #[inline]
    pub fn into_value(self) -> LightValue {
        let p = self.0;
        std::mem::forget(self);
        pack(Tag::Str, p as i64)
    }

    /// 取字节指针，供 `light_str_*` 这类收裸指针的接口使用。
    #[inline]
    pub fn as_ptr(&self) -> *const c_void_alias {
        self.0
    }
}

impl Drop for LightStr {
    fn drop(&mut self) {
        unsafe { light_str_free(self.0) }
    }
}

impl std::fmt::Debug for LightStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightStr({:?})", self.as_ref().to_string_lossy())
    }
}

impl std::fmt::Display for LightStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ---------------------------------------------------------------- 数组

/// 借用的数组/列表/元组。
#[derive(Clone, Copy)]
pub struct LightArrayRef(*mut c_void_alias);

impl LightArrayRef {
    /// # Safety
    /// 指针必须来自 lightrt 的数组函数且在调用期间有效。
    #[inline]
    pub unsafe fn from_ptr(p: *mut c_void_alias) -> Option<LightArrayRef> {
        if p.is_null() {
            None
        } else {
            Some(LightArrayRef(p))
        }
    }

    /// 从标签值取出容器引用；不是容器时返回 None。
    #[inline]
    pub fn from_value(v: LightValue) -> Option<LightArrayRef> {
        match tag_of(v) {
            Tag::List | Tag::Tuple | Tag::Dict => unsafe {
                LightArrayRef::from_ptr(untag(v) as *mut c_void_alias)
            },
            _ => None,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe { light_array_len(self.0) }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 取第 i 个元素（仍是标签值）。越界由运行期终止进程。
    #[inline]
    pub fn get(&self, i: usize) -> LightValue {
        unsafe { light_array_get(self.0, i) }
    }

    #[inline]
    pub fn get_tag(&self, i: usize) -> Tag {
        Tag::from_i32(unsafe { light_array_get_tag(self.0, i) })
    }

    pub fn iter(&self) -> LightArrayIter {
        LightArrayIter { arr: *self, i: 0 }
    }

    /// 按下标收集为 Rust 值；容器元素再递归展开一层。
    pub fn to_values(&self) -> Vec<LightValue> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }
}

impl std::fmt::Debug for LightArrayRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightArrayRef(len={})", self.len())
    }
}

pub struct LightArrayIter {
    arr: LightArrayRef,
    i: usize,
}

impl Iterator for LightArrayIter {
    type Item = LightValue;
    fn next(&mut self) -> Option<LightValue> {
        if self.i < self.arr.len() {
            let v = self.arr.get(self.i);
            self.i += 1;
            Some(v)
        } else {
            None
        }
    }
}

/// 自己持有的数组，析构时释放。
pub struct LightArray(*mut c_void_alias);

impl LightArray {
    /// 新建长度为 n 的数组（内容为零值整数）。
    pub fn new(n: usize) -> LightArray {
        unsafe { LightArray(light_array_new(n)) }
    }

    /// 从标签值接管一个容器。
    #[inline]
    pub fn from_value(v: LightValue) -> Option<LightArray> {
        match tag_of(v) {
            Tag::List | Tag::Tuple | Tag::Dict => {
                unsafe { LightArray::from_raw(untag(v) as *mut c_void_alias) }
            }
            _ => None,
        }
    }

    /// # Safety
    /// 指针必须来自 lightrt，且调用方放弃其所有权。
    #[inline]
    pub unsafe fn from_raw(p: *mut c_void_alias) -> Option<LightArray> {
        if p.is_null() {
            None
        } else {
            Some(LightArray(p))
        }
    }

    #[inline]
    pub fn as_ref(&self) -> LightArrayRef {
        LightArrayRef(self.0)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.as_ref().len()
    }

    #[inline]
    pub fn get(&self, i: usize) -> LightValue {
        self.as_ref().get(i)
    }

    pub fn push(&mut self, v: LightValue) {
        unsafe { light_array_push_tag(self.0, tag_of(v).as_i32(), untag(v)) }
    }

    pub fn push_int(&mut self, n: i64) {
        unsafe { light_array_push_tag(self.0, Tag::Int.as_i32(), n) };
    }

    pub fn push_str(&mut self, s: &str) {
        let tmp = LightStr::new(s);
        unsafe {
            light_array_push_tag(self.0, Tag::Str.as_i32(), tmp.0 as i64);
            std::mem::forget(tmp);
        }
    }

    pub fn iter(&self) -> LightArrayIter {
        self.as_ref().iter()
    }

    /// 交出所有权，返回裸指针（原生函数 `-> 列表` 时用）。
    #[inline]
    pub fn into_raw(self) -> *mut c_void_alias {
        let p = self.0;
        std::mem::forget(self);
        p
    }

    /// 交出所有权，返回带标签的值（放进容器元素时用）。
    #[inline]
    pub fn into_value(self) -> LightValue {
        let p = self.0;
        std::mem::forget(self);
        pack(Tag::List, p as i64)
    }
}

impl std::fmt::Debug for LightArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightArray(len={})", self.len())
    }
}

impl Drop for LightArray {
    fn drop(&mut self) {
        unsafe { light_array_free(self.0) }
    }
}

// ---------------------------------------------------------------- 动态值

/// 运行期才知道类型的值；`from_raw` 按标签自动落到对应分支。
#[derive(Debug)]
pub enum LightDyn {
    Int(i64),
    Float(f64),
    Str(LightStrRef),
    Bool(bool),
    None,
    Seq(LightArrayRef),
    Dict(LightArrayRef),
}

impl LightDyn {
    pub fn from_raw(v: LightValue) -> LightDyn {
        let raw = untag(v);
        match tag_of(v) {
            Tag::Int => LightDyn::Int(raw),
            Tag::Float => LightDyn::Float(f64::from_bits(raw as u64)),
            Tag::Str => unsafe {
                LightDyn::Str(LightStrRef::from_ptr(raw as *const c_void_alias).unwrap())
            },
            Tag::Bool => LightDyn::Bool(raw != 0),
            Tag::None => LightDyn::None,
            Tag::List | Tag::Tuple => unsafe {
                LightDyn::Seq(LightArrayRef::from_ptr(raw as *mut c_void_alias).unwrap())
            },
            Tag::Dict => unsafe {
                LightDyn::Dict(LightArrayRef::from_ptr(raw as *mut c_void_alias).unwrap())
            },
        }
    }

    /// 取运行期长度：字符串按字节、容器按元素数。
    pub fn len(&self) -> i64 {
        unsafe { light_dynamic_len(self.value()) }
    }

    /// 字典的项数：布局与 LightArray 不同，要走运行期分派
    pub fn dict_len(&self) -> i64 {
        match self {
            LightDyn::Dict(a) => unsafe { light_dynamic_len(pack(Tag::Dict, a.0 as i64)) },
            _ => 0,
        }
    }

    /// 索引取值，类型未知也能用。
    pub fn index(&self, i: i64) -> LightValue {
        unsafe { light_dynamic_get(self.value(), i) }
    }

    /// 转成文本（返回新建的字符串，析构时自动释放）。
    pub fn to_text_owned(&self) -> LightStr {
        let v = self.value();
        unsafe { LightStr(light_dynamic_to_text(untag(v), tag_of(v).as_i32())) }
    }

    /// 当前值对应的 ABI 标签值（不消耗 self）。
    pub fn value(&self) -> LightValue {
        match self {
            LightDyn::Int(n) => pack(Tag::Int, *n),
            LightDyn::Float(f) => pack(Tag::Float, f.to_bits() as i64),
            LightDyn::Str(s) => pack(Tag::Str, s.0 as i64),
            LightDyn::Bool(b) => pack(Tag::Bool, *b as i64),
            LightDyn::None => pack(Tag::None, 0),
            LightDyn::Seq(a) | LightDyn::Dict(a) => pack(Tag::List, a.0 as i64),
        }
    }

    /// 还原成 ABI 上的标签值。
    pub fn into_value(self) -> LightValue {
        match self {
            LightDyn::Int(n) => pack(Tag::Int, n),
            LightDyn::Float(f) => pack(Tag::Float, f.to_bits() as i64),
            LightDyn::Str(s) => pack(Tag::Str, s.0 as i64),
            LightDyn::Bool(b) => pack(Tag::Bool, b as i64 as i64),
            LightDyn::None => pack(Tag::None, 0),
            LightDyn::Seq(a) | LightDyn::Dict(a) => pack(Tag::List, a.0 as i64),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            LightDyn::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            LightDyn::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<LightArrayRef> {
        match self {
            LightDyn::Seq(a) | LightDyn::Dict(a) => Some(*a),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- 便捷转换

/// 整数转 lightrt 字符串。
pub fn int_to_str(n: i64) -> LightStr {
    unsafe { LightStr(light_int_to_str(n)) }
}

pub fn bool_to_str(b: bool) -> LightStr {
    unsafe { LightStr(light_bool_to_str(b as i64)) }
}

pub fn float_to_str(f: f64) -> LightStr {
    unsafe { LightStr(light_float_to_str(f)) }
}

/// 把 lightrt 字符串转成整数/浮点。
pub fn str_to_int(s: &LightStrRef) -> i64 {
    unsafe { light_str_to_int(s.as_ptr() as *const c_void_alias) }
}

pub fn str_to_float(s: &LightStrRef) -> f64 {
    unsafe { light_str_to_float(s.as_ptr() as *const c_void_alias) }
}

/// 拼接两个 lightrt 字符串，返回新分配的。
pub fn concat(a: &LightStr, b: &LightStr) -> LightStr {
    unsafe { LightStr(light_str_concat(a.as_ptr(), b.as_ptr())) }
}