#!/usr/bin/env bash
# 构建 Linux ARM 发行包与自解压安装程序
#
# 依赖：
#   rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
#   zig（Fedora: dnf install zig）作为交叉链接器
#   可选：cargo-zigbuild（用于 musl 静态构建）
#
# 产物：
#   dist/light-lang-<版本>-linux-aarch64.tar.gz        与 -installer
#   dist/light-lang-<版本>-linux-armv7.tar.gz          与 -installer（可选）
#   dist/light-lang-<版本>-linux-aarch64-musl.tar.gz   与 -installer（可选，需要 cargo-zigbuild）
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
root_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$root_dir"

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' lightc/Cargo.toml | head -1)"
[[ -n "$version" ]] || { printf '%s\n' "错误：无法读取版本号" >&2; exit 1; }

command -v zig >/dev/null 2>&1 || {
    printf '%s\n' "错误：未找到 zig（Fedora: dnf install zig）" >&2
    exit 1
}

# 目标与 zig 三元组对应关系
TARGETS=(
    "aarch64-unknown-linux-gnu:aarch64-linux-gnu:aarch64"
    "armv7-unknown-linux-gnueabihf:arm-linux-gnueabihf:armv7"
)
if [[ "${MUSL:-1}" != "0" ]] && command -v cargo-zigbuild >/dev/null 2>&1; then
    TARGETS+=("aarch64-unknown-linux-musl:aarch64-linux-musl:aarch64-musl")
elif [[ "${MUSL:-1}" != "0" ]]; then
    printf '%s\n' "提示：未安装 cargo-zigbuild，跳过 musl 静态构建（cargo install cargo-zigbuild）"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

has_zigbuild() {
    command -v cargo-zigbuild >/dev/null 2>&1
}

stage_release() {
    # $1=target $2=zig triple $3=短名
    local target="$1" triple="$2" short="$3"
    rustup target list --installed | grep -qx "$target" || rustup target add "$target"
    local packages=(-p light -p lightc -p lightgo -p lightrt -p light-installer)
    if has_zigbuild; then
        # cargo-zigbuild 会正确处理 sysroot 与 musl 静态链接
        printf '  -> 编译 %s（cargo-zigbuild）\n' "$target"
        cargo zigbuild --release --target "$target" "${packages[@]}"
        return 0
    fi
    # 退路：直接把 zig cc 当链接器
    local dir="$work/wrap-$short"
    mkdir -p "$dir"
    printf '#!/bin/sh\nexec zig cc -target %s "$@"\n' "$triple" > "$dir/cc"
    chmod +x "$dir/cc"
    local triple_upper
    triple_upper="$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')"
    export "CARGO_TARGET_${triple_upper}_LINKER=$dir/cc"
    printf '  -> 编译 %s（zig cc）\n' "$target"
    cargo build --release --target "$target" "${packages[@]}"
    unset "CARGO_TARGET_${triple_upper}_LINKER"
}

stage_dist() {
    # $1=target $2=短名 $3=zig 三元组
    local target="$1" short="$2" triple="$3"
    local stage="$work/light-lang-$version"
    local out="target/$target/release"
    rm -rf "$stage"
    mkdir -p "$stage/bin" "$stage/lib" "$stage/share/light-lang/lightrt/src" \
        "$stage/share/light-lang/lightweb/src"
    for bin in nlight nlightc nlightGo; do
        cp "$out/$bin" "$stage/bin/$bin"
        chmod 755 "$stage/bin/$bin"
    done
    cp "$out/libnlightrt.a" "$stage/lib/libnlightrt.a"
    cp install.sh LICENSE README.md "$stage/"
    chmod 755 "$stage/install.sh"
    cp -R examples "$stage/share/light-lang/examples"
    cp -R skills "$stage/share/light-lang/skills"
    cp lightrt/Cargo.toml "$stage/share/light-lang/lightrt/"
    cp lightrt/src/lib.rs "$stage/share/light-lang/lightrt/src/"
    cp lightweb/Cargo.toml "$stage/share/light-lang/lightweb/"
    cp lightweb/src/lib.rs "$stage/share/light-lang/lightweb/src/"
    find "$stage" -name '.lightGo' -type d -prune -exec rm -rf {} + 2>/dev/null || true
    find "$stage" -name 'target' -type d -prune -exec rm -rf {} + 2>/dev/null || true
    printf '%s\n' "$version" > "$stage/share/light-lang/VERSION"

    mkdir -p dist
    local package="dist/light-lang-$version-linux-$short.tar.gz"
    rm -f "$package"
    tar -czf "$package" -C "$work" "light-lang-$version"

    # 自解压安装程序（内嵌上面的包）
    printf '  -> 构建安装程序 %s\n' "$target"
    touch installer/build.rs
    if has_zigbuild; then
        LIGHT_PAYLOAD= LIGHT_PACKAGE="$PWD/$package" \
            cargo zigbuild --release --target "$target" -p light-installer
    else
        local linker_var="CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_LINKER"
        printf '#!/bin/sh\nexec zig cc -target %s "$@"\n' "$triple" > "$work/cc-$short"
        chmod +x "$work/cc-$short"
        LIGHT_PACKAGE="$PWD/$package" "${!linker_var:=$work/cc-$short}" \
            cargo build --release --target "$target" -p light-installer
        unset "$linker_var"
    fi
    cp "$out/light-installer" "dist/light-lang-$version-linux-$short-installer"
    chmod 755 "dist/light-lang-$version-linux-$short-installer"
}

printf '%s\n' "==> 交叉编译 ARM 目标（版本 $version）"
for entry in "${TARGETS[@]}"; do
    IFS=':' read -r target triple short <<<"$entry"
    printf '==> 目标 %s（zig: %s）\n' "$target" "$triple"
    stage_release "$target" "$triple" "$short"
    stage_dist "$target" "$short" "$triple"
done

printf '%s\n' "==> 生成校验和"
rm -f dist/sha256sum-arm.txt
(cd dist && sha256sum light-lang-"$version"-linux-aarch64* light-lang-"$version"-linux-armv7* \
    light-lang-"$version"-linux-aarch64-musl* 2>/dev/null | sort -u > sha256sum-arm.txt) || true

printf '\n%s\n' "构建完成："
for file in dist/light-lang-"$version"-linux-aarch64.tar.gz \
    dist/light-lang-"$version"-linux-aarch64-installer \
    dist/light-lang-"$version"-linux-armv7.tar.gz \
    dist/light-lang-"$version"-linux-armv7-installer \
    dist/light-lang-"$version"-linux-aarch64-musl.tar.gz \
    dist/light-lang-"$version"-linux-aarch64-musl-installer; do
    [[ -f "$file" ]] || continue
    ls -lh "$file" | sed 's/^/  /'
    printf '      %s\n' "$(file -b "$file" | cut -d, -f1-3)"
done
printf '\n%s\n' "校验和："
sed 's/^/  /' dist/sha256sum-arm.txt
printf '\n%s\n' "说明："
printf '  %s\n' "· glibc 版面向常规 ARM Linux（树莓派 64 位、ARM 服务器）"
printf '  %s\n' "· musl 版为静态链接，可直接拷贝运行"
printf '  %s\n' "· armv7 适用于 32 位 ARM 设备（如树莓派 1/2 代）"
