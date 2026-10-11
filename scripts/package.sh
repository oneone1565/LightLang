#!/usr/bin/env bash
# 构建 LightLang 发行包与自解压安装程序（Linux x86_64）
#
# 产物：
#   dist/light-lang-<版本>-linux-x86_64.tar.gz      发行包
#   dist/light-lang-<版本>-linux-x86_64-installer   自解压安装程序（ELF）
#   dist/sha256sum.txt                              校验和
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

arch="${ARCH:-x86_64}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' lightc/Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
    printf '%s\n' "错误：无法从 lightc/Cargo.toml 读取版本号" >&2
    exit 1
fi

stage="dist/light-lang-$version"
package="dist/light-lang-$version-linux-$arch.tar.gz"
installer="dist/light-lang-$version-linux-$arch-installer"

printf '%s\n' "==> 1/5 编译 release（版本 $version）"
cargo build --release

printf '%s\n' "==> 2/5 组装发行包目录"
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/lib" "$stage/share/light-lang/lightrt/src" "$stage/share/light-lang/lightweb/src"
mkdir -p "$stage/share/light-lang/nlight"
# nextlight 套件：nlight / nlightc / nlightGo
for bin in nlight nlightc nlightGo; do
    [[ -f "target/release/$bin" ]] || { printf '错误：缺少 target/release/%s\n' "$bin" >&2; exit 1; }
    cp "target/release/$bin" "$stage/bin/$bin"
    chmod 755 "$stage/bin/$bin"
done
cp target/release/nlightc "$stage/bin/nlightc"
chmod 755 "$stage/bin/nlightc"
rust_rt="target/release/libnlightrt.a"
light_rt="target/libnlightrt.a"
for rt in "$rust_rt" "$light_rt"; do
    [[ -f "$rt" ]] || { printf '错误：缺少 %s\n' "$rt" >&2; exit 1; }
done
cp "$rust_rt" "$stage/lib/libnlightrt.a"
cp "$light_rt" "$stage/lib/libnlightRT.a"
cp nlight/nrt.light nlight/machine.c "$stage/share/light-lang/nlight/"
cp bootstrap/lc.light "$stage/share/light-lang/nlight/nlightc.light"
cp install.sh LICENSE README.md "$stage/"
chmod 755 "$stage/install.sh"
cp -R examples "$stage/share/light-lang/examples"
cp -R skills "$stage/share/light-lang/skills"
if [[ -d lightgo/examples ]]; then
    cp -R lightgo/examples "$stage/share/light-lang/examples/lightgo"
fi
mkdir -p "$stage/share/light-lang/lightffi"
cp lightffi/light_ffi.rs "$stage/share/light-lang/lightffi/"
cp lightrt/Cargo.toml "$stage/share/light-lang/lightrt/"
cp lightrt/src/lib.rs "$stage/share/light-lang/lightrt/src/"
cp lightweb/Cargo.toml "$stage/share/light-lang/lightweb/"
cp lightweb/src/lib.rs "$stage/share/light-lang/lightweb/src/"
# 清理构建残留
find "$stage" -name '.lightGo' -type d -prune -exec rm -rf {} + 2>/dev/null || true
find "$stage" -name 'target' -type d -prune -exec rm -rf {} + 2>/dev/null || true

printf '%s\n' "==> 3/5 打包 $package"
rm -f "$package"
tar -czf "$package" -C dist "light-lang-$version"

printf '%s\n' "==> 4/5 构建自解压安装程序（内嵌发行包）"
touch installer/build.rs
LIGHT_PACKAGE="$package" cargo build --release -p light-installer
cp target/release/light-installer "$installer"
chmod 755 "$installer"

printf '%s\n' "==> 5/5 生成校验和"
rm -f dist/sha256sum.txt
(cd dist && sha256sum "$(basename "$package")" "$(basename "$installer")" > sha256sum.txt)

printf '\n%s\n' "构建完成："
ls -lh "$package" "$installer" | sed "s/^/  /"
printf '\n%s\n' "文件类型："
file -b "$installer" | sed 's/^/  /'
printf '\n%s\n' "校验和："
sed 's/^/  /' dist/sha256sum.txt
printf '\n%s\n' "本地试用："
printf '  %s --target /tmp/lightlang-extract\n' "$installer"
printf '  %s --prefix "$HOME/.local" -y\n' "$installer"
