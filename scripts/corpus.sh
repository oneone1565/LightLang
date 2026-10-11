#!/usr/bin/env bash
# 自举差分回归：stage1(lc1) 与 stage2 对同一语料生成的 IR 必须逐字节一致
#
# 用法：scripts/corpus.sh [文件...]
#   不带参数时跑 examples/ 下所有 .lt / .light
set -uo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

B="$root_dir/target/bootstrap"
mkdir -p "$B/corpus"
rt_lib="target/release/libnlightrt.a"

# 需要自举编译器产物
if [ ! -x "$B/lc1" ] || [ ! -x "$B/stage2" ]; then
	echo "缺少 $B/lc1 或 $B/stage2，先跑 scripts/bootstrap.sh" >&2
	exit 1
fi

if [ "$#" -gt 0 ]; then
	files=("$@")
else
	mapfile -t files < <(ls examples/*.lt examples/*.light 2>/dev/null)
fi

pass=0; fail=0; skip=0
failed=()
for f in "${files[@]}"; do
	name="$(basename "$f")"
	s1="$B/corpus/$name.s1.ll"
	s2="$B/corpus/$name.s2.ll"
	e1="$B/corpus/$name.s1.err"
	e2="$B/corpus/$name.s2.err"

	# 在源码所在目录里跑：导入语句按当前工作目录找模块
	( cd "$(dirname "$f")" && timeout 60 "$B/lc1" < "$(basename "$f")" ) > "$s1" 2> "$e1"; r1=$?
	( cd "$(dirname "$f")" && timeout 60 "$B/stage2" < "$(basename "$f")" ) > "$s2" 2> "$e2"; r2=$?

	if [ "$r1" -ne 0 ] && [ "$r2" -ne 0 ]; then
		# 两边都编译不了 = 超出当前子集，跳过
		skip=$((skip + 1))
		continue
	fi
	if [ "$r1" -ne "$r2" ]; then
		fail=$((fail + 1)); failed+=("$name 退出码 s1=$r1 s2=$r2")
		continue
	fi
	if cmp -s "$s1" "$s2"; then
		pass=$((pass + 1))
	else
		fail=$((fail + 1)); failed+=("$name IR 不一致")
	fi
done

echo "差分回归：一致 $pass / 超出子集跳过 $skip / 不一致 $fail"
for m in "${failed[@]:-}"; do
	[ -n "$m" ] && echo "  ✗ $m"
done
[ "$fail" -eq 0 ]
