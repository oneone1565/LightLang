# LightLang

<div align="center">

**让中文编程更自然，让 Rust 能力触手可及。**

一个正在成长中的中文编程语言、独立编译器和 `nlightGo` 项目工具。

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
![工具](https://img.shields.io/badge/tool-nlightGo-purple)

## 核心特点

### 1. 直接写程序，不必先写 `main`

LightLang 支持顶层语句。保存为 `hello.light` 后即可编译运行：

```light
打印 "你好，LightLang"
```

```bash
nlightc hello.light -o hello
./hello
```

也可以使用 `light` 直接运行源码：

```bash
light hello.light
```

`light` 会把程序临时编译到系统临时目录，运行结束后自动清理；使用 `--debug` 可以启用 Debug 断言。

### 2. 交互模式：像 Python 一样逐句执行

不带任何文件参数运行 `light`，即可进入交互模式：

```bash
light
```

```text
LightLang 0.2.1 交互模式（输入 退出 结束会话，输入 帮助 查看命令）
>>> 名字 = "LightLang"
>>> 打印 "你好，" + 名字
你好，LightLang
>>> 函数 平方(n):
...     返回 n * n
>>> 打印 平方(7)
49
```

每输入一句就编译并执行整段会话，因此变量、函数都会保留；输出只显示当前这一句的结果，不会重复打印之前的内容。

以 `:` 结尾的语句（函数、`如果`、`当` 等）可以跨行输入，缩进回退到顶层或输入空行时执行。

交互命令：

| 命令 | 说明 |
| --- | --- |
| `退出` / `exit` | 结束会话 |
| `帮助` / `help` | 显示命令帮助 |
| `清屏` / `clear` | 清空屏幕 |
| `重置` / `reset` | 清空当前会话变量 |
| `打印历史` | 显示已输入的语句 |
| `保存 <文件>` | 把当前会话保存为 `.light` 源文件 |

会话源码写在当前目录，因此 `导入 "模块名"` 会像普通脚本一样按当前目录解析。编译或运行失败时，该句会被丢弃，可以直接改正后重输。

### 3. 中文语法，面向快速表达

语言保留了清晰的中文关键字，让代码更接近自然语言：

```light
名字 = 输入()
如果 名字 != "":
    打印 "你好，" + 名字
否则:
    打印 "请输入名字"
```

当前语言已经提供字符串、整数、浮点数、布尔值、空值、列表、元组和字典等基础能力。

### 4. 打印支持 Python 式格式化

`打印` 的字符串里可以直接用 `{名字}` 取当前作用域的变量，也可以写任意表达式：

```light
名字 = "LightLang"
版本 = 2
打印("你好，{名字}！当前版本 {版本}")
打印("下个版本是 {版本 + 1}")
打印(f"f 前缀写法：{名字}")
```

也可以像 `printf` 一样显式传参，用 `{}` 按顺序填充，或用 `{0}`、`{1}` 指定位置：

```light
打印("{} 和 {}", "甲", "乙")
打印("{1} 在 {0} 前面", "甲", "乙")
打印("{0} 与 {0}", 7)
```

| 写法 | 说明 |
| --- | --- |
| `{名字}` | 取当前作用域的变量 |
| `{表达式}` | 求值后插入，支持 `+ - * /`、函数调用、方法调用等 |
| `{}` | 按顺序填入第一个未使用的位置参数 |
| `{0}` `{1}` | 按下标填入位置参数，可重复使用 |
| `{{` `}}` | 输出字面量 `{` 与 `}` |

补充说明：

- 位置参数只在第一个参数是字符串字面量时生效；`打印(3, 4)` 仍按原语义打印元组 `(3, 4)`。
- 占位符里的名字在编译期检查，写错会立即报错并指出未定义的变量。
- 格式化同样适用于交互模式、`页面` 块和 `lightWeb` 输出。

### 5. 内存资源由运行时管理

LightLang 配套的 `lightrt` 使用 Rust 实现字符串、数组和字典等运行时对象。编译器会跟踪拥有堆资源的变量，并在适当位置插入释放操作。

这让 LightLang 在保持脚本体验的同时，也能拥有可靠的资源管理基础。

### 6. Light 模块与 Rust FFI 互通

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

### 7. 字符串方法直接可用

字符串可以使用简洁的方法式调用：

```light
文本 = " Hello LightLang "
打印(文本.去空白())
打印(文本.大写())
打印(文本.替换("Light", "World"))
```

### 8. 基础异常处理

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
./target/release/nlightc
./target/release/nlightGo
```

### 使用自解压安装程序

发行包提供一个自解压的 Linux ELF 安装程序，双击（执行）即可完成安装：

```bash
./light-lang-0.2.1-linux-x86_64-installer
```

它会把内嵌的发行包释放到临时目录，执行其中的 `install.sh`，结束后自动清理临时文件。

```bash
# 安装到指定位置并跳过确认
./light-lang-0.2.1-linux-x86_64-installer --prefix "$HOME/.local" -y

# 只释放安装包、不执行安装
./light-lang-0.2.1-linux-x86_64-installer --target /tmp/lightlang

# 卸载
./light-lang-0.2.1-linux-x86_64-installer --uninstall --prefix "$HOME/.local" -y
```

### 使用发行包

```bash
tar -xzf light-lang-0.2.1-linux-x86_64.tar.gz
cd light-lang-0.2.1
./install.sh
```

安装脚本默认安装到 `/usr/local`，会将 `nlightc`、`nlightGo` 加入系统路径，并安装 `nlightrt` 静态库。

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

### 自行构建发行包

```bash
./scripts/package.sh
```

会依次完成 release 编译、组装发行目录、打包 `dist/light-lang-<版本>-linux-x86_64.tar.gz`、
构建内嵌该包的自解压安装程序 `dist/light-lang-<版本>-linux-x86_64-installer`，并生成 `dist/sha256sum.txt`。

## Linux ARM 支持

ARM 产物用 `zig` 作为交叉链接器构建，覆盖 aarch64 与 armv7：

```bash
# 依赖
dnf install zig                      # 交叉链接器
cargo install cargo-zigbuild          # 可选，用于 musl 静态构建
rustup target add aarch64-unknown-linux-gnu \
    aarch64-unknown-linux-musl armv7-unknown-linux-gnueabihf

# 构建全部 ARM 产物
./scripts/package-arm.sh
```

| 产物 | 架构 | 链接方式 | 适用场景 |
| --- | --- | --- | --- |
| `light-lang-<版本>-linux-aarch64.tar.gz` / `-installer` | ARM64 | 动态 glibc | 树莓派 4/5、ARM 服务器 |
| `light-lang-<版本>-linux-aarch64-musl.tar.gz` / `-installer` | ARM64 | 静态 musl | 拷贝即用，不依赖系统库 |
| `light-lang-<版本>-linux-armv7.tar.gz` / `-installer` | ARMv7 | 动态 glibc | 树莓派 1/2 代等 32 位设备 |

自解压安装程序的行为与 x86_64 版一致：检测已有安装、比较版本、需要时更新。

#### 交叉编译 Light 程序

`nlightc` 生成的 IR 带的是它自身的目标三元组，并把这个目标传给 C 编译器，
所以在 x86_64 机器上也能编译出 ARM 程序——只要给它一个目标架构的 C 工具链：

```bash
# 用 zig 提供目标架构的 libc 与链接器
LIGHTC_CLANG="zig cc -target aarch64-linux-musl" \
    ./nlightc 程序.light -o 程序
```

| 环境变量 | 作用 |
| --- | --- |
| `LIGHTC_CLANG` | 指定 C 编译器，**可以带参数**，例如 `zig cc -target aarch64-linux-musl` |
| `LIGHTC_CLANG_ARGS` | 追加参数，例如 `--sysroot`、`-fuse-ld=lld`、`-static` |
| `LIGHTC_CLANG_TARGET`（可选）| 覆盖目标三元组（默认取 `nlightc` 自身的构建目标） |

链接失败时 `nlightc` 会提示当前目标与本机不同，并给出对应的命令示例。
`nlightc` 会优先使用自身目录下的 `libnlightrt.a`，因此 `target/<三元组>/release/`
里的交叉编译产物不会误用宿主架构的运行时库。

## Windows 安装

Windows 版二进制由 MinGW（`x86_64-pc-windows-gnu`）交叉编译，不依赖 MSVC 运行库。

### 图形安装程序（自解压 EXE）

```bash
./scripts/package-win.sh
# 产物：dist/LightLang-0.2.1-windows-x64-setup.exe
```

双击打开图形安装界面：选择安装目录（程序目录 / 用户目录）、安装、卸载，状态实时显示。

- 安装时把 `bin` 追加到系统 `PATH`（需要管理员权限，不足会自动弹 UAC 提权），并写入「程序和功能」卸载项。
- 界面会检测已安装版本，可直接覆盖升级。
- 命令行同样可用：

```powershell
light-setup.exe --install "C:\Program Files\LightLang"
light-setup.exe --uninstall "C:\Program Files\LightLang"
light-setup.exe --check
```

构建依赖：

```bash
dnf install mingw64-gcc        # Fedora 提供 MinGW-w64 交叉链接器
rustup target add x86_64-pc-windows-gnu
```

### 图形安装包（MSI）

`packaging/windows/Product.wxs` 是 WiX 工程（v4+ schema），用 WixUI 向导
（欢迎 / 许可协议 / 安装目录 / 进度 / 完成），安装到 `%ProgramFiles%\LightLang`。

```bash
./scripts/package-msi.sh
# 产物：dist/LightLang-0.2.1-windows-x64.msi
```

脚本会自动选工具链：

| 环境 | 工具链 | 图形向导 |
| --- | --- | --- |
| Linux / macOS | wine + Windows .NET 运行时 + wix CLI | 无（见下） |
| Windows | 本机 `wix`（`dotnet tool install --global wix`） | 有 |

在 Windows 上构建完整图形安装包：

```powershell
wix extension add WixToolset.UI
wix build packaging/windows/Product.wxs -arch x64 `
    -d ProductVersion=0.2.1 `
    -d WixUI=1 `
    -d Shortcuts=1 `
    -d SourceDir=<素材目录> `
    -o dist/LightLang-0.2.1-windows-x64.msi
```

> 为什么 wine 下没有向导：WixUI 的对话框与开始菜单快捷方式都要通过
> Windows Installer 接口写库，wine 的 `msi.dll` 支持不完整（WiX v3 会直接抛
> Win32 异常，v4+ 会静默跳过这两张表）。所以本地产出的 MSI 是**基础安装包**：
> 文件、目录、注册表、升级与卸载都正常，双击即可安装，但没有向导界面。
> 想要图形向导，请在 Windows（或 Windows 虚拟机）上执行上面的命令。

MSI 不会修改系统 `PATH`（写回前无法可靠读取旧值，覆盖会破坏用户环境）：
安装后可用 EXE 安装程序（自动维护 PATH），或在「系统属性 → 环境变量」里
手动把 `bin` 加进去。

## nlightGo：用 Cargo 的思路管理 Light 项目

`nlightGo` 是一个轻量的项目构建工具，目标是让 Light 项目也拥有清晰的清单、依赖和构建流程。

### 创建项目

```bash
nlightGo 新建 demo
cd demo
nlightGo 运行
```

也可以使用英文命令：

```bash
nlightGo new demo
cd demo
nlightGo run
```

`nlightGo` 会自动从当前目录逐级向上寻找 `lightGo.toml`，因此在项目子目录中也可以直接运行：

```bash
cd src
nlightGo 构建
```

### lightGo.toml

一个最小的项目文件：

```toml
[package]
name = "demo"
version = "0.2.1"
entry = "src/main.light"
```

### 常用 Cargo 风格选项

```bash
nlightGo 构建 --release
nlightGo 构建 --debug
nlightGo 构建 --offline --locked
nlightGo 构建 --target-dir ./build
nlightGo 构建 -j 4
nlightGo 运行 --release
nlightGo 清理
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
version = "0.2.1"
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
version = "0.2.1"
entry = "src/main.light"

[[ffi]]
name = "sha2"
manifest = "ffi/Cargo.toml"
```

随后运行：

```bash
nlightGo 构建
nlightGo 运行
```

`nlightGo` 会调用 Cargo 下载和构建 `sha2`，再将生成的静态库交给 Light 链接器。

## GUI：使用 Rust 的成熟工具箱

GUI 示例使用 `eframe` 和 `egui`。它们是 Rust 生态中成熟的即时模式 GUI 工具，可以实现窗口、按钮、文本和交互逻辑。

示例位置：

```text
lightgo/examples/gui/
```

运行：

```bash
nlightGo 运行 --manifest-path lightgo/examples/gui/lightGo.toml
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

### 前端元素：不用写 HTML

页面代码块里可以直接写元素语句，编译器会生成对应的 HTML 与配套样式（亮色/暗色主题都可用）：

```light
页面 (标题="demo" 图标=none):
    主题 = "dark"
    标题 "欢迎" 级别=1
    段落 "当前版本 {1 + 1}。"
    分隔线

    容器 元素=form 动作="/submit" 方式=post 标题="留言":
        输入框 名称="名字" 标签="你的名字" 占位提示="请输入"
        下拉框 名称="城市" 选项=["北京", "上海"] 选中="上海"
        按钮 "发送" 样式=主要
    结束

    进度条 值=72 最大=100
    提示 "已保存" 级别=成功
```

元素清单：

| 元素 | 文本 | 常用属性 |
| --- | --- | --- |
| `标题` | 文本 | `级别`(1-6) |
| `段落` | 文本 | — |
| `按钮` | 文字 | `动作`、`方式`(get/post)、`样式`(主要/次要/危险) |
| `链接` | 文字 | `地址` |
| `图片` | — | `地址`、`替代文本`、`宽度`、`高度` |
| `输入框` | — | `名称`、`标签`、`默认值`、`占位提示`、`类型` |
| `文本域` | — | `名称`、`标签`、`占位提示`、`行数`、`默认值` |
| `复选框` / `单选框` | — | `名称`、`文字`、`值`、`选中` |
| `下拉框` | — | `名称`、`选项=[...]`、`选中` |
| `列表` | — | `项目=[...]`、`有序` |
| `表格` | — | `表头=[...]`、`行=[[...], [...]]` |
| `分隔线` | — | — |
| `卡片` | — | `标题`、`内容` |
| `标签` | 文本 | `颜色`(蓝/绿/黄/红/灰) |
| `进度条` | — | `值`、`最大` |
| `代码块` | 代码 | — |
| `提示` | 文本 | `级别`(信息/成功/警告/错误) |
| `容器` | — | `元素`(div/section/form/…)、`名称`、`标题`、`类名`、`动作`、`方式` |

写法要点：

- 属性值可以直接写单词（`样式=主要`、`方式=post`）；如果单词与某个变量同名，则按变量取值。
- 元素顺序即渲染顺序；`容器` 既可以用冒号加缩进块（自动闭合），也可以用 `结束` 手动闭合。
- 元素只能写在 `页面` 代码块里，写错元素名会给出可用元素提示。
- 按钮默认是 `submit`，放在 `容器 元素=form` 里即可提交表单；后端用 `请求参数(请求, "名字")` 读取查询串或表单参数。

后端读取参数：

```light
函数 提交(请求):
    名字 = 请求参数(请求, "名字")
    返回 "收到：" + 名字

路由("POST", "/submit", 提交)
```

完整示例见 `lightgo/examples/elements`：

```bash
nlightGo 运行 --manifest-path lightgo/examples/elements/lightGo.toml
```

> 提示：URL 中的参数需要百分号编码（浏览器会自动处理）；命令行可用 `curl --data-urlencode`。

运行或停止 Web 服务：

```bash
nlightGo web start --manifest-path lightgo/examples/page/lightGo.toml --port 8080
nlightGo web stop --manifest-path lightgo/examples/page/lightGo.toml
```

中文命令同样可用：

```bash
nlightGo 网页 启动
nlightGo 网页 停止
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

使用 `nlightc --debug` 或 `nlightGo 构建 --debug` 启用 Debug 断言；默认 Release 构建会忽略它们。

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
light                              进入交互模式（REPL）
light hello.light
light --debug hello.light
nlightc hello.light -o hello
nlightc --help

nlightGo new demo
nlightGo build
nlightGo run
nlightGo clean
nlightGo delete demo --yes
```

## 项目结构

```text
light-lang/
├── installer/                Linux 自解压安装程序（内嵌发行包的 ELF）
├── windows-installer/        Windows 图形安装程序（MinGW GUI，自解压 EXE）
├── packaging/windows/        WiX 5 的 MSI 工程（Product.wxs / 图标 / 许可）
├── scripts/                  打包脚本（package.sh、package-arm.sh、package-win.sh、package-msi.sh）
├── light/                   Light 运行与交互（REPL）命令
├── lightc/                  Light 编译器
├── lightrt/                 Rust 运行时
├── lightgo/                 项目工具与示例（含 elements 元素示例）
├── lightweb/                HTTP 服务器运行时
├── examples/                Light 示例
├── install.sh               发行版安装脚本
├── Cargo.toml               Rust workspace
└── lightGo.toml             Light 项目清单示例
```

### 核心组件

- **installer** - Linux 自解压安装程序：内嵌发行包，运行时释放并执行 `install.sh`，支持版本比较与更新
- **windows-installer** - Windows 图形安装程序：MinGW 构建，选择目录安装/卸载，自动维护 `PATH`
- **light** - 运行与交互命令：临时编译并直接运行 Light 源码，或进入保留变量的交互模式
- **nlightc** - 编译器，包含词法分析、语法解析、类型推导和 LLVM 代码生成
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
- 交互模式的增量编译：当前每输入一句都会重新编译整段会话，会话很大时响应会变慢

目前 Linux x86_64、Linux aarch64/armv7 提供发行包与自解压安装程序；
Windows x64 提供图形安装程序（EXE，自解压）与 MSI 安装包（MSI 的图形向导需在 Windows 上构建）。
Windows ARM64 可通过 `cargo build --release --target aarch64-pc-windows-gnullvm` 自行构建。


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

发行包与自解压安装程序在本地用一条命令构建：

```bash
./scripts/package.sh
```

它会生成 `dist/light-lang-<版本>-linux-x86_64.tar.gz` 和同名的 `-installer`
自解压安装程序（Linux ELF，运行后自动释放并执行 `install.sh`）。

如果希望参与语言设计，请优先说明：

- 想解决的问题
- 期望的语法
- 一个最小可运行示例
- 对兼容性的影响

## 许可证

LightLang 使用 GNU General Public License v3.0（GPL-3.0），详见 [LICENSE](LICENSE)。

项目中的第三方依赖仍然遵循各自的许可证。使用或分发发行包时，请同时遵守相应依赖的许可证要求。