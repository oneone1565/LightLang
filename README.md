# LightLang

<div align="center">

**让中文编程更自然，让 Rust 能力触手可及。**

一个正在成长中的中文编程语言、独立编译器和 `lightGo` 项目工具。

[快速开始](#快速开始) · [功能概览](#功能概览) · [Rust 与 GUI](#rust-与-gui) · [路线图](#路线图)

</div>

---

## 项目简介

LightLang 想解决两个很实际的问题：

1. 编程入门和小型脚本不应该被复杂工程结构打断；
2. 当项目需要高性能、本地库或成熟生态时，又不应该失去 Rust 的能力。

因此，LightLang 采用“简单语法 + 原生扩展”的路线：普通程序直接使用 `.light` 文件编写，复杂能力通过 Rust FFI、`lightGo.toml` 和 Cargo 生态接入。

这是一个**早期项目**，语法、工具链和运行时仍在持续演进，但它已经可以编译并运行真实的 Light 程序，也能够通过 Rust 静态库扩展功能。

## 项目标签

`light-language` · `中文编程` · `rust` · `llvm` · `编译器` · `ffi` · `gui` · `eframe` · `工具链`

### 项目徽章

![语言](https://img.shields.io/badge/language-LightLang-ff6b35)
![底层](https://img.shields.io/badge/core-Rust%20%2B%20LLVM-blue)
![平台](https://img.shields.io/badge/platform-Linux%20x86_64-lightgrey)
![许可证](https://img.shields.io/badge/license-GPL--3.0-green)
![阶段](https://img.shields.io/badge/status-early%20development-orange)
![欢迎](https://img.shields.io/badge/star%20%7C%20welcome!-ff6b35?style=for-the-badge)
![工具](https://img.shields.io/badge/tool-lightGo-purple)

## 核心特点

### 1. 直接写程序，不必先写 `main`

LightLang 支持顶层语句。保存为 `hello.light` 后即可编译运行：

```light
打印 "你好，LightLang"
```

```bash
lightc hello.light -o hello
./hello
```

### 2. 中文语法，面向快速表达

语言保留了清晰的中文关键字，让代码更接近自然语言：

```light
名字 = 输入()
如果 名字 != "":
    打印 "你好，" + 名字
否则:
    打印 "请输入名字"
```

当前语言已经提供字符串、整数、浮点数、布尔值、空值、列表、元组和字典等基础能力。

### 3. 内存资源由运行时管理

LightLang 配套的 `lightrt` 使用 Rust 实现字符串、数组和字典等运行时对象。编译器会跟踪拥有堆资源的变量，并在适当位置插入释放操作。

这让 LightLang 在保持脚本体验的同时，也能拥有可靠的资源管理基础。

### 4. Light 模块与 Rust FFI 互通

普通 Light 模块可以直接导入：

```light
导入 "math"
打印(math.加一(41))
```

也可以使用选择性导入：

```light
从 "math" 导入 加一
打印(加一(41))
```

Rust 能力通过带有 `extern "C"` 接口的静态库接入。当前 `lightGo.toml` 可以声明 FFI 模块，Cargo 会自动构建对应依赖。

### 5. 字符串方法直接可用

字符串可以使用简洁的方法式调用：

```light
文本 = " Hello LightLang "
打印(文本.去空白())
打印(文本.大写())
打印(文本.替换("Light", "World"))
```

### 6. 基础异常处理

LightLang 提供显式的抛出、捕获和最终清理结构：

```light
尝试:
    抛出 "这是一个错误"
捕获 错误:
    打印 "捕获到：" + 错误
最终:
    打印 "程序结束"
```

目前异常机制主要覆盖显式 `抛出`，运行时错误和更复杂的错误对象仍在继续完善。

## 快速开始

### 环境要求

- Linux x86_64
- Rust 工具链
- Clang
- Cargo

### 从源码构建

```bash
git clone <仓库地址>
cd light-lang
cargo build --release
```

构建完成后可以使用：

```bash
./target/release/lightc
./target/release/lightGo
```

### 使用发行包

```bash
tar -xzf light-lang-0.1.0-linux-x86_64.tar.gz
cd light-lang-0.1.0
./install.sh
```

安装脚本默认安装到 `/usr/local`，会将 `lightc`、`lightGo` 加入系统路径，并安装 `lightrt` 静态库。

自定义安装位置：

```bash
./install.sh --prefix "$HOME/.local"
source "$HOME/.local/etc/profile.d/lightlang.sh"
```

跳过确认：

```bash
./install.sh --yes
```

卸载：

```bash
./install.sh --uninstall
```

## lightGo：用 Cargo 的思路管理 Light 项目

`lightGo` 是一个轻量的项目构建工具，目标是让 Light 项目也拥有清晰的清单、依赖和构建流程。

### 创建项目

```bash
lightGo 新建 demo
cd demo
lightGo 运行
```

也可以使用英文命令：

```bash
lightGo new demo
cd demo
lightGo run
```

`lightGo` 会自动从当前目录逐级向上寻找 `lightGo.toml`，因此在项目子目录中也可以直接运行：

```bash
cd src
lightGo 构建
```

### lightGo.toml

一个最小的项目文件：

```toml
[package]
name = "demo"
version = "0.1.0"
entry = "src/main.light"
```

### 常用 Cargo 风格选项

```bash
lightGo 构建 --release
lightGo 构建 --debug
lightGo 构建 --offline --locked
lightGo 构建 --target-dir ./build
lightGo 构建 -j 4
lightGo 运行 --release
lightGo 清理
```

当前支持并会实际生效的选项包括：

- `--release`、`--debug`、`--profile`
- `--target-dir`
- `--offline`、`--locked`、`--frozen`
- `-j` / `--jobs`
- `--features`、`--all-features`、`--no-default-features`
- `--config`、`--color`、`--timings`
- `-q` / `--quiet`、`-v` / `--verbose`

## Rust 依赖与 FFI

Rust crate 不能直接被 Light 函数调用，因为 Rust 的 ABI、泛型和所有权模型与 Light 的调用接口不同。正确的方式是使用一个 C ABI 桥接层。

### FFI Cargo 配置

```toml
[package]
name = "sha2_ffi"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib"]

[dependencies]
sha2 = "0.11.0"
lightrt = { path = "../../../../lightrt" }
```

桥接函数需要导出稳定的 C ABI。当前 Light FFI 的参数和返回值按 `i64` 传递，因此可以通过 `lightrt::str_parts` 读取 Light 字符串：

```rust
use sha2::{Digest, Sha256};
use std::os::raw::c_void;
use std::slice;

#[no_mangle]
pub extern "C" fn light_sha256_prefix32(s: *const c_void) -> i64 {
    if s.is_null() {
        return 0;
    }
    let (ptr, len) = lightrt::str_parts(s);
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    let digest = Sha256::digest(bytes);
    i64::from(u32::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3],
    ]))
}
```

在 `lightGo.toml` 中声明桥接模块：

```toml
[package]
name = "sha2_demo"
version = "0.1.0"
entry = "src/main.light"

[[ffi]]
name = "sha2"
manifest = "ffi/Cargo.toml"
```

随后运行：

```bash
lightGo 构建
lightGo 运行
```

`lightGo` 会调用 Cargo 下载和构建 `sha2`，再将生成的静态库交给 Light 链接器。

## GUI：使用 Rust 的成熟工具箱

GUI 示例使用 `eframe` 和 `egui`。它们是 Rust 生态中成熟的即时模式 GUI 工具，可以实现窗口、按钮、文本和交互逻辑。

示例位置：

```text
lightgo/examples/gui/
```

运行：

```bash
lightGo 运行 --manifest-path lightgo/examples/gui/lightGo.toml
```

GUI 模块会自动寻找系统中的 CJK 字体，优先支持 Droid Sans Fallback 和 Noto CJK，避免中文显示为方框。

## lightWeb：轻量 Web 框架

`lightWeb` 让 Light 程序可以在同一个项目中同时提供后端接口和浏览器页面。页面中的 `打印` 内容会作为 HTML 返回，而不是输出到终端。

```light
导入 "lightWeb"

函数 接口(请求):
    返回 "后端接口"

路由("GET", "/api", 接口)

页面 (标题="hello" 图标=none):
    语言="zh-cn"
    主题="dark"
    CSS = "h1 { color: #ff6b35; }"
    打印 "<h1>hello</h1>"
```

`CSS` 或 `样式` 赋值会追加到页面的 `<style>` 中，后端仍然可以使用 `请求方法()`、`请求路径()`、`请求体()` 和 `请求头()`。页面会自动注册到 `/`，并生成带有语言、标题、主题、图标和 CSS 的 HTML 页面。

运行或停止 Web 服务：

```bash
lightGo web start --manifest-path lightgo/examples/page/lightGo.toml --port 8080
lightGo web stop --manifest-path lightgo/examples/page/lightGo.toml
```

中文命令同样可用：

```bash
lightGo 网页 启动
lightGo 网页 停止
```

启动后用浏览器打开 `http://127.0.0.1:8080/`。第一版使用同步 HTTP 服务器，支持页面 HTML、精确路径、GET、POST 和文本响应；模板、静态文件和路径参数会继续完善。

## 异步与多线程

LightLang 使用 `线程(编号):` 创建线程代码块。没有 `线程` 代码块时，程序仍然按照普通单线程方式执行。

```light
打印 "主线程开始"

线程(1):
    打印 "线程一"

线程(2):
    打印 "线程二"

打印 "主线程结束"
```

主线程没有语句时不会产生额外输出，空线程块也会静默结束。程序结束前会自动等待所有线程完成。也可以显式调用：

```light
等待全部()
```

当前线程块适合执行相互独立的耗时任务；线程之间共享复杂对象和同步通信仍在后续完善中。

## Debug 断言

断言只在 Debug 构建中生效。Release 构建会直接移除断言条件，不产生运行时检查。

```light
断言 1 == 1, "初始条件"
打印 "继续执行"

断言 0 == 1, "这里应该失败"
打印 "不会执行"
```

使用 `lightc --debug` 或 `lightGo 构建 --debug` 启用 Debug 断言；默认 Release 构建会忽略它们。

## 示例目录

仓库中提供了一组可以逐个运行的示例：

- 基础输出和直接赋值
- 字符串方法
- 列表、元组和字典
- Light 模块导入
- `从` 导入和基础命名空间
- `输入()`
- 异常捕获
- Rust FFI
- `sha2` Cargo 依赖
- `eframe` GUI
- `lightWeb` GET/POST 路由
- `线程(编号):` 多线程代码块
- Debug `断言`

示例文件主要位于：

```text
examples/
lightgo/examples/
```

## 常用命令

```bash
lightc hello.light -o hello
lightc --help

lightGo new demo
lightGo build
lightGo run
lightGo clean
lightGo delete demo --yes
```

## 项目结构

```text
light-lang/
├── lightc/                  Light 编译器
├── lightrt/                 Rust 运行时
├── lightgo/                 项目工具与示例
├── lightweb/                HTTP 服务器运行时
├── examples/                Light 示例
├── install.sh               发行版安装脚本
├── Cargo.toml               Rust workspace
└── lightGo.toml             Light 项目清单示例
```

### 核心组件

- **lightc** - 编译器，包含词法分析、语法解析、类型推导和 LLVM 代码生成
- **lightrt** - Rust 运行时，提供字符串、数组、字典等核心数据类型及数学、文件、系统函数
- **lightgo** - 项目构建工具，支持项目管理、依赖构建和 Cargo 风格选项
- **lightweb** - 基于 Rust `tiny_http` 的轻量 HTTP 服务器运行时

## 当前限制

LightLang 目前仍处于早期开发阶段，以下能力正在逐步完善：

- 更完整的模块隔离和运行时模块对象
- 更丰富、标准化的 FFI 类型系统
- 字符串、数组和字典的完整方法集合
- 更强的编译错误和类型错误提示
- 跨平台构建与安装流程
- 更完整的包管理、依赖缓存和发布流程
- 更完整的 Web 路由、模板和静态文件支持
- 基于 Light 的简单配置文件格式

目前发行包主要面向 Linux x86_64，其他平台需要根据系统工具链单独构建。

## 路线图

### 近期计划

- 完善 `lightWeb`
- 设计基于 Light 的简单配置文件格式
- 改进模块系统和错误信息
- 扩充 GUI 组件和交互示例

### 中期方向

- 更完整的 Rust ABI 类型描述
- 包仓库和依赖版本锁定
- 增量编译与构建缓存
- 更强的开发工具和调试支持
- 跨平台发行包

## 参与贡献

欢迎提交问题、建议和代码贡献。

开始开发前，可以先运行：

```bash
cargo check --release
cargo build --release
```

如果希望参与语言设计，请优先说明：

- 想解决的问题
- 期望的语法
- 一个最小可运行示例
- 对兼容性的影响

## 许可证

LightLang 使用 GNU General Public License v3.0（GPL-3.0），详见 [LICENSE](LICENSE)。

项目中的第三方依赖仍然遵循各自的许可证。使用或分发发行包时，请同时遵守相应依赖的许可证要求。