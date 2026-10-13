//! 把发行包内嵌进安装程序。
//!
//! 优先使用 `LIGHT_PACKAGE` 指定的包；否则使用 `dist/light-lang-<版本>-linux-x86_64.tar.gz`。
//! 包不存在时写入空载荷，保证普通 `cargo build` 仍能通过（安装程序会在运行时给出提示）。

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LIGHT_PACKAGE");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let dist = manifest.join("..").join("dist");
    let default_package = dist.join(format!("light-lang-{}-linux-x86_64.tar.gz", version));

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(custom) = env::var("LIGHT_PACKAGE") {
        candidates.push(PathBuf::from(custom));
    }
    candidates.push(default_package.clone());

    println!("cargo:rerun-if-changed={}", default_package.display());
    println!(
        "cargo:rerun-if-changed={}",
        dist.join(format!("light-lang-{}-linux-aarch64.tar.gz", version)).display()
    );

    let target = out_dir.join("package.tar.gz");
    match candidates.iter().find(|path| path.is_file()) {
        Some(source) => {
            fs::copy(source, &target).expect("复制发行包失败");
            let size = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
            println!("cargo:rerun-if-changed={}", source.display());
            println!("cargo:warning=安装程序已内嵌发行包 {} ({} 字节)", source.display(), size);
        }
        None => {
            fs::write(&target, b"").expect("写入空载荷失败");
            println!(
                "cargo:warning=未找到发行包 {}，安装程序不含载荷；请先运行 scripts/package.sh",
                default_package.display()
            );
        }
    }
}
