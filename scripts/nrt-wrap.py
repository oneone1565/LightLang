#!/usr/bin/env python3
"""给用 Light 写的运行时补一层 C ABI 包装。

Light 的函数一律是 `i64 f(i64, ...)`，而 lightc 生成的调用方按运行时的
C 签名走（`ptr f(ptr, i64)`、返回 `i1`/`double` 等）。同一个名字的两种签名
在 LLVM 里不能共存，所以这里把 Light 版本改名为 `f__light`，再按声明补一个
薄包装，保证符号与运行时 ABI 一致。

用法：nrt-wrap.py <输入.ll> <输出.ll>
"""

import re
import sys

DECL = re.compile(r"^declare\s+((?:\w+)\s+)?@([A-Za-z_][\w$]*)\(([^)]*)\)\s*$")
DEF = re.compile(r"^define\s+i64\s+@([A-Za-z_][\w$]*)\(i64")


def split_params(text):
    if not text.strip():
        return []
    return [p.strip() for p in text.split(",")]


def main():
    src, dst = sys.argv[1], sys.argv[2]
    lines = open(src, encoding="utf-8").read().split("\n")

    decls = {}      # 名字 -> (返回类型, [参数类型])
    for line in lines:
        m = DECL.match(line)
        if m:
            ret = (m.group(1) or "void").strip()
            decls[m.group(2)] = (ret, split_params(m.group(3)))

    # 第一遍：确定哪些函数要包装
    names = set()
    for line in lines:
        m = re.match(r"^define\s+i64\s+@([A-Za-z_][\w$]*)\(", line)
        if m and m.group(1) in decls:
            names.add(m.group(1))

    out = []
    wrappers = []
    pending = None      # 正在延后插入包装器的函数名
    skip_main = False
    for line in lines:
        # 编译器总会给每个文件生成 main()，运行时不需要它
        if line.startswith("define ") and re.match(r"^define\s+\S+\s+@main\(", line):
            skip_main = True
            continue
        if skip_main:
            if line == "}":
                skip_main = False
            continue
        # 同模块里 declare + define 同名函数是非法的：包装后会自己提供定义，
        # 所以这里把对应的声明去掉。
        md = re.match(r"^declare\s+\S+\s+@([A-Za-z_][\w$]*)\(", line)
        if md and md.group(1) in names:
            continue
        # 内部调用要指向 i64 版本，否则会撞上包装器的 ptr 签名
        if "call" in line and not line.startswith("define") and not line.startswith("declare"):
            for nm in names:
                line = line.replace("@" + nm + "(", "@" + nm + "__light(")
        if line.startswith("define "):
            m2 = re.match(r"^define\s+i64\s+@([A-Za-z_][\w$]*)\(", line)
            if m2 and m2.group(1) in decls and m2.group(1) not in wrappers:
                name = m2.group(1)
                wrappers.append(name)
                pending = name
                out.append(line.replace("@" + name + "(", "@" + name + "__light(", 1))
                continue
        out.append(line)
        # 函数体结束的 } 之后补上包装器
        if pending and line == "}":
            ret, params = decls[pending]
            out.extend(wrap(pending, ret, params))
            pending = None

    if not wrappers:
        print("没有需要包装的函数", file=sys.stderr)
    open(dst, "w", encoding="utf-8").write("\n".join(out) + "\n")
    print(f"包装 {len(wrappers)} 个函数: {' '.join(wrappers)}", file=sys.stderr)


def wrap(name, ret, params):
    """按声明的签名生成一个转发到 __light 版本的包装函数。"""
    args = []
    sig = []
    for i, decl in enumerate(params):
        parts = decl.split()
        ptype = parts[0]
        sig.append(f"{ptype} %p{i}")
        if ptype in ("ptr", "i8*"):
            conv = f"  %a{i} = ptrtoint ptr %p{i} to i64"
        elif ptype in ("double", "float"):
            conv = f"  %a{i} = bitcast {ptype} %p{i} to i64"
        else:
            conv = f"  %a{i} = zext {ptype} %p{i} to i64" if ptype != "i64" else f"  %a{i} = add i64 %p{i}, 0"
        args.append((i, conv))
    lines = [f"define {ret} @{name}({', '.join(sig) or '...'}) {{", "entry:"]
    for i, conv in args:
        lines.append(conv)
    call = f"  %r = call i64 @{name}__light({', '.join('i64 %a' + str(i) for i, _ in args)})"
    if ret == "void":
        lines.append(call)
        lines.append("  ret void")
    elif ret == "i1":
        lines.append(call)
        lines.append("  %c = icmp ne i64 %r, 0")
        lines.append("  ret i1 %c")
    elif ret == "double":
        lines.append(call)
        lines.append("  %d = bitcast i64 %r to double")
        lines.append("  ret double %d")
    elif ret == "float":
        lines.append(call)
        lines.append("  %d = bitcast i64 %r to float")
        lines.append("  ret float %d")
    else:  # ptr / i64 / 其他整数
        lines.append(call)
        if ret == "ptr":
            # 不透明指针模式下必须用指令，不能用常量表达式
            lines.append("  %p = inttoptr i64 %r to ptr")
            lines.append("  ret ptr %p")
        elif ret == "i64":
            lines.append("  ret i64 %r")
        else:
            lines.append(f"  %t = trunc i64 %r to {ret}")
            lines.append(f"  ret {ret} %t")
    lines.append("}")
    lines.append("")
    return lines


if __name__ == "__main__":
    main()
