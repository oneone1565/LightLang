#!/usr/bin/env bash
# 构建 Windows MSI 安装包
#
# 用法：
#   ./scripts/package-msi.sh            # 自动选择工具链
#   WIX_TOOLCHAIN=windows ./scripts/package-msi.sh    # 在 Windows 上用本机 wix
#
# 工具链：
#   1) Windows：dotnet tool install 全局 wix，直接 wix build（可带图形向导）
#   2) 其他平台：wine + Windows .NET 运行时 + Linux 侧 wix CLI（wine 里的 wix.dll）
#
# 图形向导（WixUI）与开始菜单快捷方式需要 Windows Installer 接口：
#   - Windows 上加 -d WixUI=1 -d Shortcuts=1 即可生成完整图形安装包
#   - wine 下这两部分无法生成（wine 的 msi.dll 支持不完整），脚本会自动跳过
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' lightc/Cargo.toml | head -1)"
[[ -n "$version" ]] || { printf '%s\n' "错误：无法读取版本号" >&2; exit 1; }
msi="dist/LightLang-$version-windows-x64.msi"
mkdir -p dist

# ---------------------------------------------------------------- 准备素材
printf '%s\n' "==> 1/3 准备安装素材"
stage="build/msi/stage"
rm -rf build/msi
mkdir -p "$stage"
out="target/x86_64-pc-windows-gnu/release"
if [[ ! -f "$out/nlight.exe" ]]; then
    printf '%s\n' "提示：未找到 MinGW 产物，先执行 ./scripts/package-win.sh"
    export PATH="${HOME}/.cargo/bin:$PATH"
    cargo build --release --target x86_64-pc-windows-gnu \
        -p light -p lightc -p lightgo -p lightrt
fi
cp "$out/nlight.exe" "$out/nlightc.exe" "$out/nlightGo.exe" "$stage/"
cp "$out/libnlightrt.a" "$stage/"
cp README.md LICENSE "$stage/"
cp packaging/windows/light.ico packaging/windows/License.rtf "$stage/"
(cd examples && zip -qr "$root_dir/$stage/examples.zip" .)
(cd skills && zip -qr "$root_dir/$stage/skills.zip" .)
printf '  素材：%s（%s 个文件）\n' "$stage" "$(ls "$stage" | wc -l)"

# ---------------------------------------------------------------- 选择工具链
wine_prefix="${WINEPREFIX:-$HOME/.wine-light}"
wix_ui=0
shortcuts=0
toolchain="windows"

wix_cli=""
if [[ "${WIX_TOOLCHAIN:-auto}" != "windows" ]]; then
    wix_cli="$(find "$HOME/.dotnet/tools/.store" -name wix.dll 2>/dev/null | head -1)"
    if [[ -n "$wix_cli" ]] && command -v wine >/dev/null 2>&1 \
        && [[ -f "$wine_prefix/drive_c/dotnet/dotnet.exe" ]]; then
        toolchain="wine"
    else
        wix_cli=""
    fi
fi

printf '%s\n' "==> 2/3 生成 MSI（工具链：$toolchain）"
case "$toolchain" in
wine)
    export WINEPREFIX="$wine_prefix" WINEDEBUG=-all
    win_wix="Z:$(printf '%s' "$wix_cli" | sed 's|^/||; s|/|\\|g' | sed 's|^|\\|')"
    root_win="Z:$(pwd | sed 's|^/||; s|/|\\|g' | sed 's|^|\\|')"
    wine "C:\\dotnet\\dotnet.exe" "$win_wix" build packaging/windows/Product.wxs \
        -arch x64 \
        -d "ProductVersion=$version" \
        -d "WixUI=$wix_ui" \
        -d "Shortcuts=$shortcuts" \
        -d "SourceDir=$root_win\\build\\msi\\stage" \
        -o "$root_win\\dist\\LightLang-$version-windows-x64.msi" 2>&1 \
        | grep -v "MESA-INTEL" || true
    ;;
*)
    command -v wix >/dev/null 2>&1 || {
        printf '%s\n' "错误：未找到 wix（dotnet tool install --global wix）" >&2
        exit 1
    }
    wix build packaging/windows/Product.wxs \
        -arch x64 \
        -d "ProductVersion=$version" \
        -d "WixUI=${WIXUI:-1}" \
        -d "Shortcuts=${SHORTCUTS:-1}" \
        -d "SourceDir=$(pwd)/build/msi/stage" \
        -o "$msi"
    ;;
esac

[[ -f "$msi" ]] || { printf '%s\n' "错误：未生成 MSI" >&2; exit 1; }

# ---------------------------------------------------------------- 校验
printf '%s\n' "==> 3/3 校验产物"
tables="$(msiinfo tables "$msi" 2>/dev/null | tr ' ' '\n' | sort -u | tr '\n' ' ')"
printf '  表：%s\n' "${tables% }"
printf '  文件数：%s\n' "$(msiinfo export "$msi" File 2>/dev/null | grep -cE '^[A-Za-z0-9_]+\s+[A-Za-z0-9_]+')"
if [[ "$toolchain" == "wine" ]]; then
    printf '  图形向导：未生成（wine 的 Windows Installer 接口不完整，Windows 上构建才有）\n'
fi
sha256sum "$msi" > dist/sha256sum-msi.txt
printf '\n%s\n' "构建完成："
ls -lh "$msi" | sed 's/^/  /'
file -b "$msi" | cut -c1-70 | sed 's/^/  类型: /'
sed 's/^/  /' dist/sha256sum-msi.txt
