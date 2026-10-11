#!/usr/bin/env bash
# 自举验证：stage1(nlightc) 编译 lc.light -> stage2 编译自身 -> stage3
#
# 自举成立的判据：stage2 与 stage3 生成的 LLVM IR 逐字节一致（定点）。
# 也就是「用 Light 写的编译器编译自己，得到的编译器行为完全相同」。
#
# 用法：
#   scripts/bootstrap.sh              # 完整验证（约 30 秒）
#   scripts/bootstrap.sh --keep       # 保留中间产物
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

work_dir="${LIGHT_BOOT_DIR:-$root_dir/target/bootstrap}"
mkdir -p "$work_dir"
rt_lib="target/release/libnlightrt.a"
keep=0
[ "${1:-}" = "--keep" ] && keep=1

step() { printf '\n=== %s\n' "$1"; }

# 链接产物：IR 文件 + 运行时静态库
link_ir() {
	local ir="$1" out="$2"
	clang "$ir" "$rt_lib" -lstdc++ -lpthread -lm -o "$out" 2> >(grep -v 'overriding the module target triple' >&2 || true)
}

step "0/5 构建编译器与运行时"
cargo build --release -q
export PATH="$root_dir/target/release:$PATH"

step "1/5 stage1：nlightc 把 lc.light 编译成 lc1"
nlightc bootstrap/lc.light -o "$work_dir/lc1" --quiet
echo "lc1 就绪"

step "2/5 lc1 编译 lc.light，产出 stage2.ll"
"$work_dir/lc1" < bootstrap/lc.light > "$work_dir/stage2.ll"
echo "stage2.ll $(wc -l < "$work_dir/stage2.ll") 行"

step "3/5 链接 stage2（Light 写的编译器，由 lc1 编译得到）"
link_ir "$work_dir/stage2.ll" "$work_dir/stage2"
echo "stage2 就绪"

step "4/5 stage2 编译 lc.light，产出 stage3.ll"
"$work_dir/stage2" < bootstrap/lc.light > "$work_dir/stage3.ll"
echo "stage3.ll $(wc -l < "$work_dir/stage3.ll") 行"

step "5/5 定点比较 stage2.ll 与 stage3.ll"
if cmp -s "$work_dir/stage2.ll" "$work_dir/stage3.ll"; then
	echo "*** 自举成立：stage2.ll == stage3.ll ***"
	# 再确认 stage2 本身能编译普通程序
	printf '打印("自举 OK")\n' > "$work_dir/smoke.light"
	"$work_dir/stage2" < "$work_dir/smoke.light" > "$work_dir/smoke.ll"
	link_ir "$work_dir/smoke.ll" "$work_dir/smoke"
	"$work_dir/smoke"
	[ "$keep" -eq 0 ] && rm -f "$work_dir/smoke.light" "$work_dir/smoke.ll" "$work_dir/smoke"
	exit 0
fi

echo "!!! 自举不成立：stage2.ll 与 stage3.ll 有差异 !!!" >&2
diff "$work_dir/stage2.ll" "$work_dir/stage3.ll" | head -40 >&2 || true
exit 1
