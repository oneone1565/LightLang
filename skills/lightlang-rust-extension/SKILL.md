---
name: lightlang-rust-extension
description: 规范使用 Rust、lightrt 和 nlightGo 扩展 LightLang，设计安全 C ABI、静态库模块、nlightWeb 页面和线程接口。
---

# LightLang Rust 扩展规范

## 适用范围

使用本 Skill 创建或修改 LightLang 的 Rust 扩展 crate，尤其是 `nlightGo` FFI、GUI、HTTP、`lightrt` 函数和线程功能。

## 核心原则

- Rust 扩展必须通过稳定的 C ABI 暴露，不能把 Rust ABI、泛型或复杂结构体直接暴露给 Light。
- 优先复用 `lightrt` 的字符串、数组、字典和错误能力，不重复实现运行时对象。
- 所有进入 Rust 的指针都视为不可信输入，必须检查空指针、长度和 UTF-8 边界。
- 所有跨 FFI 的 Rust 线程都必须捕获 panic，不能让 panic 穿过 `extern "C"` 边界。
- 扩展 crate 默认输出 `staticlib`，由 `nlightGo` 构建并交给 `nlightc` 链接。

## C ABI 基线

当前 Light 函数和外部函数统一使用 `i64`：

- 整数、布尔值、浮点句柄和运行时对象句柄都通过 `i64` 传递。
- 字符串句柄是 `lightrt` 分配的 `LightStr*`。
- 函数返回值也必须遵守约定；不要返回 Rust `String`、`Vec`、枚举或引用。
- 函数名使用 ASCII C 标识符，Light 侧通过 `外部` 声明或 nlightWeb 特殊语法调用。

## 类型安全桥接层（首选）

`nlightc` 编译 `.rs` 原生模块时会自动把 `lightffi/light_ffi.rs` 内联进来，
所以直接写 `#[no_mangle] pub extern "C" fn` 就能用类型安全的包装，
**不需要写 extern 声明、不需要 Cargo 依赖**。

```rust
#[no_mangle]
pub extern "C" fn greet(name: *const core::ffi::c_void) -> *mut core::ffi::c_void {
    let Some(s) = (unsafe { LightStrRef::from_ptr(name) }) else {
        return LightStr::new("(空)").into_raw();
    };
    LightStr::new(&format!("你好，{}！", s.as_str())).into_raw()
}
```

Light 侧要写类型标注，否则实参不带运行期标签、原生侧看不到真实类型：

```
导入 "my_native"
外部 greet(name: 字符串) -> 字符串
打印(greet("Light"))
```

可用类型：

| 类型 | 说明 |
| --- | --- |
| `LightStr` | 拥有的字符串，析构自动 `light_str_free` |
| `LightStrRef` | 借用的字符串，不释放 |
| `LightArray` / `LightArrayRef` | 列表/元组，支持 `iter()` |
| `LightDyn` | 运行期分派：`from_raw` 后匹配 `Int/Float/Str/Bool/None/Seq/Dict` |

`into_raw()` 返回**裸指针**（原生函数 `-> 字符串` 时用）；
`into_value()` 返回**带标签的值**（放进容器元素时用）。两者不能混。

已知限制：带标签的值只有 56 位载荷，浮点经 `light_tagged` 会被截断，
所以 `任意` 形参不要用来传浮点。

模块文件名不要和调用它的 `.light` 同名 —— `导入 "x"` 会先找 `x.light`。

## 推荐 crate 结构

```text
my-extension/
├── Cargo.toml
└── src/
    └── lib.rs
```

`Cargo.toml`：

```toml
[package]
name = "my_extension"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["staticlib"]

[dependencies]
lightrt = { path = "../../lightrt" }
```

如果扩展需要 crates.io 依赖，在 `[dependencies]` 中正常声明，由 Cargo 下载和锁定。

## lightGo.toml

```toml
[package]
name = "demo"
version = "1.0.0"
entry = "src/main.light"

[[ffi]]
name = "my_extension"
manifest = "../my-extension/Cargo.toml"
```

- `name` 是 Light `导入` 使用的大小写敏感名称。
- `manifest` 指向真正的 Cargo crate，不要把 Rust 源码直接当作普通 `.rs` 文件导入。
- `nlightGo` 会把 FFI 静态库和运行时（Rust 版 `libnlightrt.a`，Light 版 `libnlightRT.a`）组合后传给 `nlightc`。

## 最小整数扩展

```rust
#[no_mangle]
pub extern "C" fn light_add(a: i64, b: i64) -> i64 {
    a + b
}
```

Light 侧：

```light
导入 "my_extension"
外部 add(a, b)
打印 add(20, 22)
```

函数名应保持短、ASCII、稳定，并在 Rust 和 Light 两侧保持完全一致。

## 字符串所有权

读取 Light 字符串时，优先使用 `lightrt` 提供的安全边界函数：

```rust
use std::os::raw::c_void;
use std::slice;

#[no_mangle]
pub extern "C" fn light_sum_bytes(value: *const c_void) -> i64 {
    if value.is_null() {
        return 0;
    }
    let (ptr, len) = lightrt::str_parts(value);
    if ptr.is_null() {
        return 0;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    bytes.iter().map(|byte| *byte as i64).sum()
}
```

规则：

- 传入的 Light 字符串通常是借用对象，函数返回后不能保存指针。
- 返回新字符串时，调用 `light_str_from_utf8` 创建由运行时拥有的字符串。
- 释放临时字符串时调用 `light_str_free`，不要直接释放 Light 传入的字符串。
- 不要假设指针一定来自合法的 `LightStr`，除非 ABI 文档明确规定。

## 错误和 panic

- C ABI 函数中使用 `catch_unwind(AssertUnwindSafe(...))` 包裹可能 panic 的逻辑。
- 不要在 `extern "C"` 函数中直接使用 `unwrap()`、`expect()` 或让 `Box` 错误跨边界。
- 失败优先返回明确的错误码、空句柄或由 `light_throw` 设置的运行时错误。
- 资源错误可以调用 `lightrt` 的安全失败函数，但要在文档中说明会终止进程。

## 线程扩展

线程任务的函数指针使用：

```rust
type LightTask = unsafe extern "C" fn(i64) -> i64;
```

创建线程时：

- 用 `std::thread::Builder` 创建线程。
- 在线程闭包内捕获 panic。
- 保存 `JoinHandle`，提供显式 join 或统一 join-all。
- 任务返回值必须符合 Light 的 `i64` ABI。
- 线程任务结束后必须回收句柄，不能无限累积。

## nlightWeb 和 GUI

- Web 请求对象只在处理函数调用期间有效，不要保存到全局变量。
- HTTP 处理器返回字符串；页面处理器使用页面运行时生成 HTML。
- GUI 扩展负责窗口生命周期，Light 侧只传递稳定的整数或字符串数据。
- GUI 必须处理中文字体缺失，优先加载系统 CJK 字体或随项目分发字体。
- 第三方 GUI 依赖较多，首次构建耗时长是正常现象。

## 安全清单

提交扩展前逐项检查：

- [ ] Cargo crate 输出 `staticlib`。
- [ ] 所有公开函数使用 `#[no_mangle] pub extern "C"`。
- [ ] 参数和返回值符合当前 `i64` ABI。
- [ ] 指针、长度、UTF-8 和空指针均已检查。
- [ ] 没有 Rust panic 穿越 FFI 边界。
- [ ] 字符串和容器的所有权约定已写入文档。
- [ ] 线程句柄可以回收，任务不会泄漏。
- [ ] `lightGo.toml` 的模块名与 Light `导入` 完全一致。
- [ ] `cargo check --release` 无 warning。
- [ ] 使用 `nlightGo 构建` 和实际 Light 程序完成验证。

## 验证流程

```bash
cargo check --release
cargo build --release
nlightGo 构建
nlightGo 运行
```

扩展 crate 单独验证：

```bash
cargo test --manifest-path path/to/crate/Cargo.toml
```

测试失败时先区分三类问题：

1. Rust 编译或类型错误；
2. C ABI、所有权或链接错误；
3. Light 语法、类型推导或运行时行为错误。

不要为了让测试通过而在 Light 侧加入未记录的强制类型转换。
