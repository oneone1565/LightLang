// Light 原生模块：类型安全桥接层用法示例
//
// nlightc 会自动把 lightffi/light_ffi.rs 内联进来，
// 所以这里直接写 #[no_mangle] pub extern "C" fn 就能用
// LightStr / LightStrRef / LightArray / LightArrayRef / LightDyn，
// 不需要 extern 声明、不需要 Cargo 依赖。

/// 收一个 Light 字符串（裸 LightStr 指针），回一个新字符串
#[no_mangle]
pub extern "C" fn greet(name: *const core::ffi::c_void) -> *mut core::ffi::c_void {
    let Some(s) = (unsafe { LightStrRef::from_ptr(name) }) else {
        return LightStr::new("(空)").into_raw();
    };
    LightStr::new(&format!("你好，{}！共 {} 字节", s.as_str(), s.len())).into_raw()
}

/// 收列表（裸指针），统计整数和与元素个数
#[no_mangle]
pub extern "C" fn sum_list(list: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    let Some(a) = (unsafe { LightArrayRef::from_ptr(list) }) else {
        return LightStr::new("(空列表)").into_raw();
    };
    let mut total = 0i64;
    let mut ints = 0i64;
    for v in a.iter() {
        if let LightDyn::Int(n) = LightDyn::from_raw(v) {
            total += n;
            ints += 1;
        }
    }
    LightStr::new(&format!("{} 个元素，其中 {} 个整数，和 = {}", a.len(), ints, total)).into_raw()
}

/// 收任意标签值，按运行期类型分派
#[no_mangle]
pub extern "C" fn describe(v: i64) -> *mut core::ffi::c_void {
    let d = LightDyn::from_raw(v);
    let kind = tag_of(v).as_i32();
    let text = match &d {
        LightDyn::Int(n) => format!("整数 {}", n),
        LightDyn::Float(f) => format!("浮点 {}", f),
        LightDyn::Str(s) => format!("字符串 {:?}", s.as_str()),
        LightDyn::Bool(b) => format!("布尔 {}", b),
        LightDyn::None => "空值".to_string(),
        LightDyn::Seq(a) => format!("容器，{} 个元素", a.len()),
        LightDyn::Dict(_) => format!("字典，{} 项", d.dict_len()),
    };
    LightStr::new(&format!("[标签 {}] {}", kind, text)).into_raw()
}

/// 构造一个新列表回给 Light：证明原生侧能造容器
#[no_mangle]
pub extern "C" fn make_list(n: i64) -> *mut core::ffi::c_void {
    let mut arr = LightArray::new(n as usize);
    for i in 0..n {
        arr.push_int(i * i);
    }
    LightStr::new(&format!("造了 {} 个平方数", arr.len())).into_raw()
}