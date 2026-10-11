#!/usr/bin/env bash
# 构建 nlightRT（用 Light 实现的运行时）
#
# 产物：target/libnlightrt.a
#   nrt.light   —— 字符串/数组/字典/打印/文件/数学（Light 实现）
#   machine.c   —— 8 个裸内存原语（自举地板，必须是机器级）
#   nrt-wrap.py —— 给 Light 生成的 i64 签名补一层 C ABI 包装
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

out="target/nlightrt"
mkdir -p "$out"
export PATH="$root_dir/target/release:$PATH"

step() { printf '\n=== %s\n' "$1"; }

step "1/4 用 nlightc 把 nrt.light 编译成 LLVM IR"
nlightc nlight/nrt.light --no-lightrt --emit-llvm > "$out/nrt.ll"

step "2/4 补 C ABI 包装"
python3 scripts/nrt-wrap.py "$out/nrt.ll" "$out/nrt_wrap.ll"

step "3/4 机器层（clang -c）"
clang -c "$out/nrt_wrap.ll" -o "$out/nrt.o" 2>&1 | grep -v 'overriding the module target triple' || true
clang -c -O2 nlight/machine.c -o "$out/machine.o"

step "4/4 打包静态库"
ar rcs target/libnlightrt.a "$out/nrt.o" "$out/machine.o"
echo "产物：target/libnlightrt.a（$(nm target/libnlightrt.a 2>/dev/null | grep -c ' T ') 个符号）"
