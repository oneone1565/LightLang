#!/usr/bin/env bash
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
prefix="/usr/local"
uninstall=false
assume_yes=false

usage() {
    printf '%s\n' "用法："
    printf '%s\n' "  ./install.sh [--prefix /usr/local]"
    printf '%s\n' "  ./install.sh --uninstall [--prefix /usr/local]"
    printf '%s\n' ""
    printf '%s\n' "选项："
    printf '%s\n' "  --prefix <目录>  安装前缀，默认 /usr/local"
    printf '%s\n' "  --uninstall      卸载 LightLang"
    printf '%s\n' "  -y, --yes        跳过确认"
    printf '%s\n' "  -h, --help       显示帮助"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            [[ $# -ge 2 ]] || { printf '%s\n' "错误：--prefix 缺少目录" >&2; exit 1; }
            prefix="$2"
            shift 2
            ;;
        --uninstall)
            uninstall=true
            shift
            ;;
        -y|--yes)
            assume_yes=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf '%s\n' "未知参数：$1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

as_root() {
    if [[ "$(id -u)" -eq 0 ]]; then
        "$@"
        return
    fi
    local probe="$prefix"
    while [[ ! -e "$probe" ]]; do
        local parent
        parent="$(dirname "$probe")"
        if [[ "$parent" == "$probe" ]]; then
            break
        fi
        probe="$parent"
    done
    if [[ -w "$probe" ]]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        printf '%s\n' "错误：需要 root 权限或 sudo" >&2
        exit 1
    fi
}

confirm() {
    if [[ "$assume_yes" == true ]]; then
        return 0
    fi
    printf '确认执行此操作吗？输入 y 继续：'
    read -r answer
    case "$answer" in
        y|Y|是) return 0 ;;
        *) printf '%s\n' "已取消"; exit 0 ;;
    esac
}

if [[ "$uninstall" == true ]]; then
    confirm
    as_root rm -f "$prefix/bin/nlight" "$prefix/bin/nlightc" "$prefix/bin/nlightGo" "$prefix/bin/light" "$prefix/bin/lightc" "$prefix/bin/lightGo" "$prefix/lib/libnlightrt.a" "$prefix/lib/libnlightRT.a" "$prefix/lib/liblightrt.a" "$prefix/etc/profile.d/lightlang.sh"
    as_root rm -rf "$prefix/share/light-lang"    printf '%s\n' "LightLang 已卸载"
    exit 0
fi

required_files=(
    "$script_dir/bin/nlight"
    "$script_dir/bin/nlightc"
    "$script_dir/bin/nlightGo"
    "$script_dir/lib/libnlightrt.a"
    "$script_dir/lib/libnlightRT.a"
)
for file in "${required_files[@]}"; do
    if [[ ! -f "$file" ]]; then
        printf '%s\n' "发行包缺少文件：$file" >&2
        exit 1
    fi
done

confirm
as_root install -Dm755 "$script_dir/bin/nlight" "$prefix/bin/nlight"
as_root install -Dm755 "$script_dir/bin/nlightc" "$prefix/bin/nlightc"
as_root install -Dm755 "$script_dir/bin/nlightGo" "$prefix/bin/nlightGo"
as_root install -Dm644 "$script_dir/lib/libnlightrt.a" "$prefix/lib/libnlightrt.a"
as_root install -Dm644 "$script_dir/lib/libnlightRT.a" "$prefix/lib/libnlightRT.a"
as_root mkdir -p "$prefix/share/light-lang"
for share_dir in examples lightffi lightrt lightweb nlight; do
    if [[ -d "$script_dir/share/light-lang/$share_dir" ]]; then
        as_root cp -R "$script_dir/share/light-lang/$share_dir" "$prefix/share/light-lang/"
    fi
done
# 记录已安装版本，供安装程序比较并执行更新
installed_version="${LIGHT_VERSION:-}"
if [[ -z "$installed_version" ]]; then
    installed_version="$("$script_dir/bin/nlightc" --version 2>/dev/null | awk '{print $2}')"
fi
if [[ -n "$installed_version" ]]; then
    printf '%s\n' "$installed_version" | as_root tee "$prefix/share/light-lang/VERSION" >/dev/null
fi
as_root mkdir -p "$prefix/etc/profile.d"
printf 'export PATH="%s/bin:$PATH"\n' "$prefix" | as_root tee "$prefix/etc/profile.d/lightlang.sh" >/dev/null
printf '%s\n' "LightLang 已安装到 $prefix"
printf '%s\n' "请重新打开终端，或执行：source $prefix/etc/profile.d/lightlang.sh"
