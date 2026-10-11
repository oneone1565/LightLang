#!/usr/bin/env bash
# 构建 Windows 图形安装程序（自解压 EXE，MinGW 交叉编译）
#
# 依赖：
#   rustup target add x86_64-pc-windows-gnu
#   MinGW-w64 交叉链接器（Fedora: dnf install mingw64-gcc）
#
# 产物：
#   dist/LightLang-<版本>-windows-x64-setup.exe
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

target="x86_64-pc-windows-gnu"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' lightc/Cargo.toml | head -1)"
[[ -n "$version" ]] || { printf '%s\n' "错误：无法读取版本号" >&2; exit 1; }

command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1 || {
    printf '%s\n' "错误：未找到 x86_64-w64-mingw32-gcc（Fedora: dnf install mingw64-gcc）" >&2
    exit 1
}

printf '%s\n' "==> 1/4 交叉编译 MinGW 工具链（$version）"
rustup target list --installed | grep -qx "$target" || rustup target add "$target"
cargo build --release --target "$target" -p light -p lightc -p lightgo -p lightrt

printf '%s\n' "==> 2/4 准备安装素材"
stage="build/win/stage"
rm -rf build/win
mkdir -p "$stage/bin" "$stage/lib" "$stage/share/light-lang"
out="target/$target/release"
cp "$out/nlight.exe" "$out/nlightc.exe" "$out/nlightGo.exe" "$stage/bin/"
cp "$out/libnlightrt.a" "$stage/lib/"
cp README.md LICENSE "$stage/"
cp -R examples "$stage/share/light-lang/examples"
cp -R skills "$stage/share/light-lang/skills"
if [[ -d lightgo/examples ]]; then
    cp -R lightgo/examples "$stage/share/light-lang/examples/lightgo"
fi
printf '%s\n' "$version" > "$stage/share/light-lang/VERSION"
find "$stage" -name '.lightGo' -type d -prune -exec rm -rf {} + 2>/dev/null || true
find "$stage" -name 'target' -type d -prune -exec rm -rf {} + 2>/dev/null || true
(cd "$stage" && zip -qr payload.zip . -x 'payload.zip')

printf '%s\n' "==> 3/4 构建图形安装程序"
touch windows-installer/build.rs
LIGHT_PAYLOAD="$PWD/$stage/payload.zip" cargo build --release --target "$target" -p windows-installer

printf '%s\n' "==> 4/4 输出与校验和"
mkdir -p dist
setup="dist/LightLang-$version-windows-x64-setup.exe"
cp "$out/light-setup.exe" "$setup"
chmod 755 "$setup"
rm -f dist/sha256sum-win.txt
(cd dist && sha256sum "$(basename "$setup")" > sha256sum-win.txt)

printf '\n%s\n' "构建完成："
ls -lh "$setup" | sed 's/^/  /'
file -b "$setup" | cut -c1-110 | sed 's/^/  类型: /'
printf '\n%s\n' "校验和："
sed 's/^/  /' dist/sha256sum-win.txt
printf '\n%s\n' "说明："
printf '  %s\n' "· 双击打开图形安装界面，可选择安装目录、安装、卸载"
printf '  %s\n' "· 内含 MinGW 构建的 nlight/nlightc/nlightGo，不依赖 MSVC 运行库"
printf '  %s\n' "· 安装时需要管理员权限（写入系统 PATH），不足会自动请求提权"
printf '  %s\n' "· nlightc 编译 Light 程序时需要目标机器已安装 LLVM/clang"
