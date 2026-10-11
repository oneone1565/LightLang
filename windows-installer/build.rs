//! 把 Windows 安装素材（zip）内嵌进图形安装程序。
//!
//! 优先使用 `LIGHT_PAYLOAD` 指定的包；否则使用 `build/win/stage/payload.zip`。
//! 素材不存在时写入空载荷，保证普通 `cargo build` 仍能通过。

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LIGHT_PAYLOAD");
    // 图形安装程序：双击时不带控制台窗口；命令行模式会再挂接父控制台
    // 只对 Windows 目标生效，避免影响本机（Linux）构建
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bins=-Wl,--subsystem,windows");
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let default_payload = manifest.join("..").join("build").join("win").join("stage").join("payload.zip");

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(custom) = env::var("LIGHT_PAYLOAD") {
        candidates.push(PathBuf::from(custom));
    }
    candidates.push(default_payload.clone());

    println!("cargo:rerun-if-changed={}", default_payload.display());

    let target = out_dir.join("payload.zip");
    match candidates.iter().find(|path| path.is_file()) {
        Some(source) => {
            fs::copy(source, &target).expect("复制安装素材失败");
            let size = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
            println!("cargo:rerun-if-changed={}", source.display());
            println!("cargo:warning=Windows 安装程序已内嵌素材 {} ({} 字节)", source.display(), size);
        }
        None => {
            fs::write(&target, b"").expect("写入空载荷失败");
            println!(
                "cargo:warning=未找到 Windows 安装素材 {}，请先运行 scripts/package-win.sh",
                default_payload.display()
            );
        }
    }
}
