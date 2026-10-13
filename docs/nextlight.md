# nextlight 套件

Light 语言有两套实现并存，两条链互相验证：同一份源码，用参考实现编出来的程序输出应当与用 nextlight 链编出来的完全一致。

```
源码 ──nlightc(Rust)──► .ll ──libnlightrt.a──► 可执行文件      参考实现
     └─nlightc(Light 自举编译器)──► .ll ──libnlightRT.a──► 可执行文件   nextlight
```

运行时库有两个同名不同位置的产物：Rust 版是 `libnlightrt.a`（`cargo build` 产出），
Light 版 nlightRT 是 `libnlightRT.a`（`scripts/build-nlightrt.sh` 产出）。

## 四个组件

| 命令 | 说明 | 实现语言 |
| --- | --- | --- |
| `nlightc` | 编译器。核心编译器用 Light 写在 `nlight/nlightc.light`（源自 `bootstrap/lc.light`），能编译自己并保持定点 | Light |
| `nlightRT` | 运行时。字符串/数组/字典/标签值/格式化/文件/线程/异常都是 Light 实现（`nlight/nrt.light`），机器层只有内存原语与几个 libc 包装（`nlight/machine.c`） | Light + 薄 C 垫片 |
| `nlightGo` | 工程工具（脚手架、构建、清理）。仍是 Rust 实现，功能不依赖自举 | Rust |
| `nlight` | 交互环境与脚本入口。仍是 Rust 实现 | Rust |

## 用法

```bash
# 参考链（Rust）
nlightc prog.light -o prog
./prog

# nextlight 链：自举编译器 + Light 运行时
nlightc prog.light --self-hosted --runtime=next -o prog
./prog

# 也可直接用自举编译器（从标准输入读源码，LLVM IR 写到标准输出）
target/bootstrap/stage2 < prog.light > prog.ll
clang prog.ll target/libnlightRT.a -lstdc++ -lpthread -lm -o prog
```

## lightWeb：网页

写网页语法要先在文件开头 `导入 "lightWeb"`，然后用 `页面(...)` 块加中文元素名。同一个程序有两种用法：

```bash
# 静态导出：编译后运行一次，把 HTML 写到文件
nlightc examples/web_page.light --page-out page.html -o page
./page                # 写出 page.html，可直接用浏览器打开

# HTTP 服务：配合 路由/启动 起服务
nlightc 服务.light -o 服务 && ./服务
```

可用元素：标题、段落、按钮、链接、图片、输入框、文本域、复选框、单选框、
下拉框、列表、表格、分隔线、卡片、标签、进度条、代码块、提示、容器。
属性名同样是中文：地址、名称、占位提示、默认值、样式、级别、颜色、动作、方式……
内置样式表已覆盖全部元素的默认外观，标签与提示的变体（绿/黄/红/灰/信息等）可直接用。

## 别名：用自己的写法

`.llang` 文件里声明「原名也能用别的写法」，每行一条 `别名 : 原名`，`#` 是注释：

```
# 英文别名.llang
print : 打印
len : 长度
true : 真
```

导入后两种写法完全等价，其余语义不变：

```
导入 "英文别名.llang"
print "hi"            # 等价于 打印 "hi"
print(计数([1,2,3]))  # len 也已换成 长度
```

扩展名不能省：不写就分不清是 `.light` 模块、`.rs` 原生模块还是别名文件。

两点规则：

- 用户自己定义的同名函数优先于别名，`函数 len()` 不会被别名吞掉
- 别名只影响关键字与内建函数的**名字**，不改变运行期表示与类型

## Rust FFI：类型安全桥接层

`.rs` 原生模块由 `nlightc` 自动注入 `lightffi/light_ffi.rs`，直接写
`#[no_mangle] pub extern "C" fn` 即可用类型安全的包装，无需 extern 声明或 Cargo 依赖：

```rust
#[no_mangle]
pub extern "C" fn greet(name: *const core::ffi::c_void) -> *mut core::ffi::c_void {
    let Some(s) = (unsafe { LightStrRef::from_ptr(name) }) else {
        return LightStr::new("(空)").into_raw();
    };
    LightStr::new(&format!("你好，{}！", s.as_str())).into_raw()
}
```

Light 侧需要类型标注，动态形参（`任意` 或不写）才会给实参打运行期标签：

```
导入 "my_native"
外部 greet(name: 字符串) -> 字符串
打印(greet("Light"))
```

桥接层提供 `LightStr` / `LightStrRef` / `LightArray` / `LightArrayRef` / `LightDyn`，
分别处理拥有与借用两种语义，析构时精确释放。完整示例见
`examples/ffi_native.rs` 与 `examples/ffi_bridge.light`。

注意：`into_raw()` 给裸指针（返回字符串时用），`into_value()` 给带标签值（放容器元素时用）。
带标签值只有 56 位载荷，浮点会被截断，`任意` 形参不要传浮点。

## 环境变量

- `NLIGHTC`：自举编译器可执行文件（默认 `target/bootstrap/stage2`，找不到则用 PATH 里的 `nlightc`）
- `NLIGHTRT`：nlightRT 静态库路径（默认 `target/libnlightRT.a`）
- `LIGHT_FFI_BRIDGE`：自定义桥接层 light_ffi.rs 的路径
- `LIGHT_DEBUG_ASSERTS`：非空时自举编译器才生成 `断言` 检查代码

## 构建与验证

```bash
scripts/bootstrap.sh        # 自举定点：stage2 == stage3
scripts/corpus.sh           # 差分回归：stage1 与 stage2 对语料生成同样的 IR
scripts/build-nlightrt.sh   # 构建 nlightRT（Light -> .ll -> 包装 -> .a）
scripts/nrt-test.sh         # nlightRT 与 Rust 运行时的输出一致性对比
```

当前状态：`scripts/nrt-test.sh` 23 个示例里 19 个与参考实现输出逐字节一致，
不一致 5、两边都异常退出 3；`scripts/corpus.sh` 23 个一致，
`scripts/bootstrap.sh` 定点成立。

已知差异：

- `all_features.lt` / `dynamic.lt` / `ops.light`：浮点结果在 nlightRT 侧变成
  `2.85e-308` 一类非规格化数。根因是带标签值的载荷只有 56 位（标签占高 8 位），
  而 double 位型需要满 64 位，走带标签路径时被截断。要修得改 lightrt 的值编码格式。
- `array.lt`：两边都会在越界时安全终止（不破坏内存），但提示里的运行时名不同
  （`lightrt` / `nlightRT`）
- `threads.light`：多线程输出的交错顺序不固定
- `noleak*.lt`：依赖 `lightrt_live_*` 存活计数，nlightRT 侧未实现这些符号，返回 0
- `ffi*.light`：需要 Rust 侧的 `.rs` 原生模块，自举链不支持
