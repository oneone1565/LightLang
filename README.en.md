# LightLang

[中文文档](README.md)

A programming language you can write in Chinese or English. Compiles to native
machine code through LLVM. The entire toolchain — compiler, runtime, project
tool — is itself written in Light and can compile itself.

Current version: **1.0.0**

---

## Table of contents

- [Why LightLang](#why-lightlang)
- [Requirements](#requirements)
- [Install](#install)
- [English mode](#english-mode)
- [Quick start](#quick-start)
- [Language tour](#language-tour)
- [The two implementations](#the-two-implementations)
- [lightWeb: write web pages in Chinese](#lightweb-write-web-pages-in-chinese)
- [Type-safe FFI to Rust](#type-safe-ffi-to-rust)
- [nlightGo: project management](#nlightgo-project-management)
- [Building and verification](#building-and-verification)
- [Known limitations](#known-limitations)

---

## Why LightLang

Light has two implementations that check each other. The same source file is
compiled by both a Rust implementation and a Light implementation; their output
must be byte-for-byte identical. Neither one is assumed to be right — a
disagreement means a bug in one of them, and the test harness says which.

- **Bootstrap compiler.** `bootstrap/lc.light` is the compiler, written in
  Light. It compiles itself, and the compiler it produces generates LLVM IR
  identical to its own predecessor's — a fixed point.
- **Two runtimes.** `lightrt` (Rust) and `nlightRT` (Light). 20 of 23 example
  programs produce identical output on both.
- **Native code.** Output goes through LLVM IR to a real ELF binary. There is no
  bytecode interpreter in the loop.

## Requirements

- Linux x86_64
- Rust toolchain
- Clang
- Cargo

## Install

### Self-extracting installer

The release ships a self-extracting Linux ELF installer. Running it unpacks the
embedded tarball and runs `install.sh`:

```bash
./light-lang-1.0.0-linux-x86_64-installer

# Install to a specific prefix without prompting
./light-lang-1.0.0-linux-x86_64-installer --prefix "$HOME/.local" -y

# Only extract, do not install
./light-lang-1.0.0-linux-x86_64-installer --target /tmp/lightlang

# Uninstall
./light-lang-1.0.0-linux-x86_64-installer --uninstall --prefix "$HOME/.local" -y
```

The installer writes `$prefix/etc/profile.d/lightlang.sh` containing
`export PATH="$prefix/bin:$PATH"`. Reload your shell afterwards, or run:

```bash
source "$HOME/.local/etc/profile.d/lightlang.sh"
```

Note that `profile.d` is only sourced by login shells (bash, zsh — not csh),
and an already-open terminal will not pick it up.

### From the tarball

```bash
tar -xzf light-lang-1.0.0-linux-x86_64.tar.gz
cd light-lang-1.0.0
./install.sh
```

### Build from source

```bash
git clone git@gitcode.com:NomiLight2026/lightLang.git
cd light-lang
cargo build --release

./target/release/nlightc      # compiler
./target/release/nlightGo     # project tool
```

## English mode

Every keyword, type name, and builtin has an English spelling. Declare it on the
first lines of the source file:

```
Lang : Eng
```

or in Chinese:

```
语言 : 英文
```

The declaration must come before any code — only blank lines and `#` comments
may precede it. Available values:

| Declaration | Meaning |
| --- | --- |
| `语言 : 中文` / `Lang : Zh-cn` | Chinese. This is the default; you may omit the line entirely. |
| `语言 : 英文` / `Lang : Eng` | English. Loads 86 built-in aliases. |
| `语言 : 来自文件(路径)` / `Lang : File(路径)` | Load aliases from your own `.llang` file. |

A full example:

```
Lang : Eng

fn fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

fn main():
    let i = 0
    while i < 10:
        print "fib(" + str_of(i) + ") = " + str_of(fib(i))
        i = i + 1
    return 0
```

Strings, comments, and variable names are untouched by the mode, so the two
spellings can be mixed freely in one file.

### Custom aliases

A `.llang` file declares one alias per line as `alias : original`. `#` starts a
comment:

```
# My.llang
greet : 打印
shout : 大写
```

Reference it from a language declaration:

```
语言 : 来自文件(./My.llang)
```

Paths resolve relative to the directory of the source file. You can also keep
using `导入 "My.llang"` as a statement.

A function you define yourself always wins over an alias of the same name, so
`fn len()` is never swallowed by an alias.

## Quick start

```bash
cat > hello.light <<'EOF'
函数 main():
    打印 "你好，世界"
    返回 0
EOF

nlightc hello.light -o hello
./hello
```

## Language tour

```light
# Comments start with #

函数 阶乘(n: 整数) -> 整数:
    如果 n <= 1:
        返回 1
    返回 n * 阶乘(n - 1)

函数 main():
    让 xs = [1, 2, 3, 4]
    打印 长度(xs)
    打印 求和(xs)

    当 长度(xs) > 2:
        弹出(xs)

    对于 x 在 xs:
        打印 字符串(x)

    # 容器
    让 名字 = ["小明", "小红"]
    打印 名字[0]

    # 字典
    让 d = {"名字": "小明", "岁数": 18}
    打印 d["名字"]

    # 异常
    尝试:
        断言 1 > 0, "恒成立"
    捕获 e:
        打印 "捕获到异常"
    最终:
        打印 "收尾"

    返回 0
```

Notable points:

- Blocks are indented, Python style.
- Every value crossing a function boundary is a single `i64`. Pointers are
  `i64` too, which is why the ABI stays stable.
- Integer literals pick their own width: a value that fits in `i32` compiles to
  32-bit arithmetic, wider values to 64-bit. Stack slots, function signatures,
  and the runtime ABI always stay `i64`.
- `元素文本` reads an element as text, for containers whose element type is not
  known until runtime.

## The two implementations

Two compilers produce LLVM IR; both feed the same linker:

```
        source
           |
   +-------+--------+
   |                |
Rust nlightc   Light lc.light        (bootstrap compiler)
   |                |
   +-------+--------+
           |
        .ll IR
           |
    clang + runtime
           |
    executable
```

Two runtimes can be linked, and the choice is visible in program output:

```bash
nlightc prog.light -o prog                          # Rust runtime
nlightc prog.light --runtime=next -o prog           # Light runtime
nlightc prog.light --self-hosted --runtime=next -o prog   # Light compiler + Light runtime
```

`nlight-test.sh` builds each example both ways and diffs the output.

## lightWeb: write web pages in Chinese

Import `lightWeb`, then use Chinese element names inside a `页面` block:

```light
导入 "lightWeb"

页面(标题="我的站点", 图标="none"):
    让 主题 = "light"
    让 样式 = ".my-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:1rem}"

    标题 "你好，世界" 级别=1
    段落 "这一页由 Light 程序生成。"

    标签 "新" 颜色="绿"

    按钮 "了解更多" 动作="/about" 方式=get 样式=主要
    链接 "源码仓库" 地址="https://gitcode.com/NomiLight2026/lightLang"

    容器 元素=div 类名="my-grid":
        卡片 标题="特性一" 内容="卡片内容写在属性里。"
        卡片 标题="特性二" 内容="容器可以嵌套。"

    表格 表头=["能力", "状态"] 行=[["自举", "完成"], ["FFI", "完成"]]

    列表 项目=["第一项", "第二项"]
    进度条 值=75 最大=100
    代码块 "打印(\"hi\")"
    提示 "一条提示。" 级别=信息
    分隔线
```

Compile and export to a static HTML file:

```bash
nlightc examples/landing.light --page-out page.html -o page
./page                # writes page.html, open it in a browser
```

Elements: 标题 (level), 段落, 按钮 (variant: 主要/次要/危险/链接), 链接,
图片, 输入框, 文本域, 复选框, 单选框, 下拉框, 列表, 表格, 分隔线, 卡片,
标签 (color), 进度条, 代码块, 提示 (level), 容器.

Syntax details that are easy to get wrong:

- `页面()` accepts only 标题 and 图标. Theme, language, and extra CSS go in
  separate statements inside the block: `让 主题 = "light"`.
- A card's content lives in the 标题= and 内容= **attributes**, not in nested
  elements.
- Array-valued attributes must be written on one line; the parser rejects
  multi-line literals.
- `打印` does not take comma-separated arguments. Use `+` and `str_of()`.

## Type-safe FFI to Rust

A `.rs` native module is compiled with `lightffi/light_ffi.rs` injected
automatically. Write ordinary `extern "C"` functions and use the typed wrappers
— no `extern` blocks, no Cargo dependencies:

```rust
#[no_mangle]
pub extern "C" fn greet(name: *const core::ffi::c_void) -> *mut core::ffi::c_void {
    let Some(s) = (unsafe { LightStrRef::from_ptr(name) }) else {
        return LightStr::new("(null)").into_raw();
    };
    LightStr::new(&format!("Hello, {}!", s.as_str())).into_raw()
}
```

The Light side needs a type annotation, and an unannotated (or `任意`) parameter
makes the argument carry a runtime tag:

```light
导入 "my_native"
外部 greet(name: 字符串) -> 字符串
打印(greet("Light"))
```

The bridge provides `LightStr`, `LightStrRef`, `LightArray`, `LightArrayRef`,
and `LightDyn`, each modelling owning versus borrowed semantics precisely.

Use `into_raw()` for a returned pointer, `into_value()` for a tagged value that
goes into a container.

## nlightGo: project management

Cargo-style project layout:

```bash
nlightGo new myproject        # scaffold
nlightGo build                # build
nlightGo clean                # remove build output
```

## Building and verification

Four gates, all runnable:

```bash
scripts/bootstrap.sh        # bootstrap fixed point: stage2 IR == stage3 IR
scripts/corpus.sh           # differential regression: stage1 vs stage2 over a corpus
scripts/build-nlightrt.sh   # build the Light runtime
scripts/nrt-test.sh         # Light runtime vs Rust runtime, output must match
scripts/package.sh          # build the release tarball and ELF installer
```

Current results: bootstrap fixed point holds, corpus differential regression 23
consistent / 0 inconsistent, runtime comparison 20 consistent / 4 divergent, all
29 examples compile.

## Known limitations

- **Tagged values carry a 56-bit payload.** The runtime tag occupies the top 8
  bits of an `i64`, so a `double` — which needs all 64 — is truncated when it
  travels the tagged path. Four examples (`all_features.lt`, `dynamic.lt`,
  `ops.light`, `noleak.lt`) diverge because of this. Container elements with a
  statically known type do not go through that path and are unaffected.
- `i128` is declared but not reachable, for the same reason.
- `打印` with multiple comma-separated arguments is not supported.
- `max` and `min` take exactly two arguments.
- `threads.light` output interleaving is nondeterministic, so it flips between
  matching and not matching across runs.
- `ffi*.light` examples need a Rust native module and therefore run only on the
  Rust side.

## License

See `LICENSE`.