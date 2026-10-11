//! 记录 lightc 自身被编译到的目标三元组。
//!
//! 生成的 IR 需要带上正确的 triple，否则在交叉编译（例如在 x86_64 上使用
//! ARM 版 lightc）时，clang 会按宿主架构去链接，导致链接失败。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=LIGHTC_TARGET_TRIPLE={}", target);
}
