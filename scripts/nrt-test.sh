#!/usr/bin/env bash
# nlightRT 对比测试：同一批示例分别链接 Rust 运行时与 Light 运行时，
# 输出必须逐字节一致。
#
# 用法：scripts/nrt-test.sh [文件...]
set -uo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

work="target/nrt-test"
mkdir -p "$work"
export PATH="$root_dir/target/release:$PATH"

if [ ! -f target/libnlightrt.a ]; then
	echo "缺少 target/libnlightrt.a，先跑 scripts/build-nlightrt.sh" >&2
	exit 1
fi

if [ "$#" -gt 0 ]; then
	files=("$@")
else
	mapfile -t files < <(ls examples/*.lt examples/*.light 2>/dev/null)
fi

rm -f "$work/diff.log"
same=0; diff_out=0; both_fail=0
declare -a notes=()

web_lib="target/release/liblightweb.a"

build() { # $1=源文件 $2=输出前缀 $3=额外链接参数
	local src="$1" prefix="$2"
	local base="$(basename "$src" | tr '.' '_')"
	# --emit-llvm 把 IR 写到 stdout
	( cd "$(dirname "$src")" && nlightc "$(basename "$src")" --emit-llvm --no-link ) > "$work/$base.ll" 2>/dev/null || return 1
	[ -s "$root_dir/$work/$base.ll" ] || return 1
	# 页面/网页语法要用到 lightWeb 的符号（light_page_*、light_web_*）；
	# 它依赖 lightrt，排在运行时之后。两个 Rust 静态库各带一份 std 符号，
	# 与 nlightc 自身的链接做法一致：允许重复定义，先出现的那份生效。
	local web="$root_dir/$web_lib"
	[ -f "$web" ] || web=""
	clang "$root_dir/$work/$base.ll" $3 ${web:+"$web"} -lstdc++ -lpthread -lm \
		-Wl,--allow-multiple-definition -o "$prefix" 2>/dev/null || return 1
	return 0
}

for f in "${files[@]}"; do
	name="$(basename "$f")"
	if ! build "$f" "$root_dir/$work/rust" "$root_dir/target/release/libnlightrt.a"; then
		notes+=("$name Rust 侧编译失败"); both_fail=$((both_fail+1)); continue
	fi
	if ! build "$f" "$root_dir/$work/light" "$root_dir/target/libnlightrt.a"; then
		miss="$(clang "$root_dir/$work/${name//./_}.ll" "$root_dir/target/libnlightrt.a" -lstdc++ -lpthread -lm -o /dev/null 2>&1 | grep -oE "undefined reference to \`[a-z_0-9]+'" | head -3 | tr '\n' ' ')"
		notes+=("$name Light 运行时缺符号：${miss:-链接失败}"); diff_out=$((diff_out+1)); continue
	fi
	out_rust="$(cd "$(dirname "$f")" && timeout 20 "$root_dir/$work/rust" 2>&1 </dev/null)"; rc_rust=$?
	out_light="$(cd "$(dirname "$f")" && timeout 20 "$root_dir/$work/light" 2>&1 </dev/null)"; rc_light=$?
	if [ "$rc_rust" -ge 128 ] && [ "$rc_light" -ge 128 ]; then
		notes+=("$name 两边都异常退出(rust=$rc_rust light=$rc_light)"); both_fail=$((both_fail+1)); continue
	fi
	if [ "$out_rust" = "$out_light" ]; then
		same=$((same+1))
	else
		diff_out=$((diff_out+1))
		notes+=("$name 输出不一致")
		{
			echo "=== $name"
			diff <(echo "$out_rust") <(echo "$out_light") | head -10
		} >> "$work/diff.log"
	fi
done

echo "nlightRT 对比：一致 $same / 不一致 $diff_out / 两边都异常 $both_fail"
for n in "${notes[@]:-}"; do
	[ -n "$n" ] && echo "  · $n"
done
[ -f "$work/diff.log" ] && echo "差异详情：$work/diff.log"
[ "$diff_out" -eq 0 ]
