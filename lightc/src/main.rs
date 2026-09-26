//! lightc —— Light 语言编译器
//!
//! 用法：lightc <输入.light> [-o 输出] [--emit-llvm] [--emit-asm] [--no-link]
//!
//! 流程：.light 源 → 词法 → 语法 → LLVM IR → clang-18 编译为机器码
//!       与 lightrt（Rust 内存安全运行时）静态链接

mod ast;
mod codegen;
mod lexer;
mod parser;
mod typeinfer;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法：lightc <输入.light> [-o 输出] [--native 名称=静态库路径] [--no-lightrt] [--emit-llvm] [--emit-asm] [--no-link]");
        std::process::exit(1);
    }

    let mut input = None;
    let mut output = None;
    let mut native_modules: HashMap<String, String> = HashMap::new();
    let mut emit_llvm = false;
    let mut emit_asm = false;
    let mut no_link = false;
    let mut no_lightrt = false;
    let mut debug_assertions = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                output = Some(args[i].clone());
            }
            "--native" => {
                i += 1;
                let spec = match args.get(i) {
                    Some(value) => value,
                    None => {
                        eprintln!("--native 需要名称=静态库路径");
                        std::process::exit(1);
                    }
                };
                let (name, path) = match spec.split_once('=') {
                    Some(parts) if !parts.0.is_empty() && !parts.1.is_empty() => parts,
                    _ => {
                        eprintln!("无效的 --native 参数：{}", spec);
                        std::process::exit(1);
                    }
                };
                native_modules.insert(name.to_string(), path.to_string());
            }
            "--emit-llvm" => emit_llvm = true,
            "--emit-asm" => emit_asm = true,
            "--no-link" => no_link = true,
            "--no-lightrt" => no_lightrt = true,
            "--debug" => debug_assertions = true,
            s if !s.starts_with('-') => input = Some(s.to_string()),
            other => {
                eprintln!("未知参数：{}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let input = input.expect("缺少输入文件");
    let input_path = PathBuf::from(&input);
    let output_stem = input_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(input.as_str())
        .to_string();
    let input_dir = input_path.parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // 递归解析所有文件（含导入），合并为单个 Program
    let mut visited = HashSet::new();
    let mut all_items = Vec::new();
    let mut extra_libs = Vec::new();
    resolve_imports(
        &input_path,
        &input_dir,
        &mut visited,
        &mut all_items,
        &mut extra_libs,
        &native_modules,
    );

    let program = ast::Program { items: all_items };

    // 类型推断（参数类型、返回类型）
    let env = typeinfer::infer(&program);

    // 检测 clang 的默认 target triple，嵌入 IR 以避免 -Woverride-module 警告
    let clang = find_clang();
    let target_triple = detect_target_triple(&clang);

    let mut cg = codegen::CodeGen::new(&env, target_triple, debug_assertions);
    let ir = match cg.gen(&program) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("代码生成错误：{}", e);
            std::process::exit(1);
        }
    };

    // 写 .ll（临时文件，编译后自动删除）
    let ll_path = std::env::temp_dir()
        .join(format!("lightc-{}-{}.ll", std::process::id(), output_stem))
        .to_string_lossy()
        .into_owned();
    if let Err(e) = fs::write(&ll_path, &ir) {
        eprintln!("写入临时 LLVM 文件失败：{}", e);
        std::process::exit(1);
    }

    if emit_llvm {
        println!("{}", ir);
        let _ = fs::remove_file(&ll_path);
        return;
    }

    // 定位 lightrt 静态库
    let rt_lib = if no_lightrt { String::new() } else { find_lightrt() };

    // 用 clang 编译 .ll → 可执行文件
    let out_exe = output.unwrap_or_else(|| output_stem.clone());

    if emit_asm {
        // 只生成汇编
        let asm_path = format!("{}.s", out_exe);
        let status = Command::new(&clang)
            .args(["-S", "-O2", "-Wno-override-module", "-o", &asm_path, &ll_path])
            .status()
            .expect("clang 执行失败");
        if !status.success() {
            std::process::exit(1);
        }
        println!("汇编已生成：{}", asm_path);
        let _ = fs::remove_file(&ll_path);
        if no_link {
            return;
        }
        // 汇编 + 链接（汇编阶段不需要 -O2）
        let obj_path = format!("{}.o", out_exe);
        let s2 = Command::new(&clang)
            .args(["-c", "-Wno-override-module", "-o", &obj_path, &asm_path])
            .status()
            .expect("clang 汇编失败");
        if !s2.success() {
            std::process::exit(1);
        }
        link(&clang, &obj_path, &rt_lib, &out_exe, &extra_libs, !no_lightrt);
        println!("可执行文件：{}", out_exe);
        let _ = fs::remove_file(&ll_path);
        let _ = fs::remove_file(&obj_path);
        let _ = fs::remove_file(&asm_path);
        return;
    }

    if no_link {
        let obj_path = format!("{}.o", out_exe);
        let status = Command::new(&clang)
            .args(["-c", "-O2", "-Wno-override-module", "-o", &obj_path, &ll_path])
            .status()
            .expect("clang 执行失败");
        if !status.success() {
            std::process::exit(1);
        }
        println!("目标文件：{}", obj_path);
        let _ = fs::remove_file(&ll_path);
        return;
    }

    // 完整编译 + 链接
    let obj_path = format!("{}.o", out_exe);
    let status = Command::new(&clang)
        .args(["-c", "-O2", "-Wno-override-module", "-o", &obj_path, &ll_path])
        .status()
        .expect("clang 执行失败");
    if !status.success() {
        std::process::exit(1);
    }
    link(&clang, &obj_path, &rt_lib, &out_exe, &extra_libs, !no_lightrt);
    println!("可执行文件：{}", out_exe);
    let _ = fs::remove_file(&ll_path);
    let _ = fs::remove_file(&obj_path);
}

fn link(clang: &str, obj: &str, rt_lib: &str, out: &str, extra_libs: &[String], include_lightrt: bool) {
    // lightrt 是 Rust staticlib，需要链接 C++ 运行时与 pthread
    let mut args = vec![obj];
    args.extend(extra_libs.iter().map(|lib| lib.as_str()));
    if include_lightrt {
        args.push(rt_lib);
    }
    args.extend(["-o", out, "-lstdc++", "-lpthread", "-ldl", "-lm"]);
    let status = Command::new(clang)
        .args(&args)
        .status()
        .expect("链接失败");
    if !status.success() {
        std::process::exit(1);
    }
}

/// 编译 .rs 模块为静态库，返回 .a 文件路径
fn compile_rs_module(rs_path: &std::path::Path) -> Result<String, String> {
    let out_dir = std::env::temp_dir();
    let lib_name = format!("lib{}.a", rs_path.file_stem().unwrap().to_string_lossy());
    let lib_path = out_dir.join(&lib_name);
    let status = Command::new("rustc")
        .args([
            "--crate-type", "staticlib",
            "-O",
            "--edition", "2021",
            "-o", lib_path.to_str().unwrap(),
            rs_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("rustc 执行失败: {}", e))?;
    if !status.success() {
        return Err(format!("编译 .rs 模块失败: {}", rs_path.display()));
    }
    Ok(lib_path.to_string_lossy().into())
}

/// 递归解析文件及其导入，将所有 Item 收集到 all_items 中
/// 返回额外的 .a 库文件路径（用于 .rs 模块导入）
fn resolve_imports(
    file_path: &Path,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    all_items: &mut Vec<ast::Item>,
    extra_libs: &mut Vec<String>,
    native_modules: &HashMap<String, String>,
) {
    let canonical = match fs::canonicalize(file_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("读取文件失败：{} ({})", file_path.display(), e);
            std::process::exit(1);
        }
    };

    if visited.contains(&canonical) {
        return; // 避免循环导入
    }
    visited.insert(canonical);

    let src = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("读取文件失败：{} ({})", file_path.display(), e);
            std::process::exit(1);
        }
    };

    // 检查是否为 .rs 文件
    if file_path.extension().map_or(false, |e| e == "rs") {
        // 编译 .rs 文件为静态库
        match compile_rs_module(file_path) {
            Ok(lib_path) => extra_libs.push(lib_path),
            Err(e) => {
                eprintln!("编译 .rs 模块失败 ({}): {}", file_path.display(), e);
                std::process::exit(1);
            }
        }
        return;
    }

    let lexer = lexer::Lexer::new(&src);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("词法错误 ({}): {}", file_path.display(), e);
            std::process::exit(1);
        }
    };

    let mut parser = parser::Parser::new(tokens);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("语法错误 ({}): {}", file_path.display(), e);
            std::process::exit(1);
        }
    };

    for item in program.items {
        match item {
            ast::Item::Import(ref module_path) => {
                // 解析导入路径：相对于当前文件目录
                 let mut resolved = base_dir.join(module_path);
                 if !resolved.exists() {
                     if let Some(lib_path) = native_modules.get(module_path) {
                         extra_libs.push(lib_path.clone());
                         continue;
                     }
                     // 尝试加 .light 后缀
                    resolved = base_dir.join(format!("{}.light", module_path));
                }
                if !resolved.exists() {
                    // 尝试加 .rs 后缀
                    resolved = base_dir.join(format!("{}.rs", module_path));
                }
                if !resolved.exists() {
                    eprintln!("导入失败：找不到模块 '{}' ({})", module_path, resolved.display());
                    std::process::exit(1);
                }
                let new_base = resolved.parent()
                    .unwrap_or(base_dir)
                    .to_path_buf();
                 resolve_imports(
                     &resolved,
                     &new_base,
                     visited,
                     all_items,
                     extra_libs,
                     native_modules,
                 );
            }
            other => {
                all_items.push(other);
            }
        }
    }
}

/// 查找可用的 clang 编译器。
/// 优先级：LIGHTC_CLANG 环境变量 > 最高版本号的 clang-XX > 系统默认 clang
fn find_clang() -> String {
    if let Ok(c) = std::env::var("LIGHTC_CLANG") {
        return c;
    }
    // 从高到低扫描版本号，优先选择最新版
    for v in (13..=30).rev() {
        let name = format!("clang-{}", v);
        if which(&name).is_ok() {
            return name;
        }
    }
    // 兜底：系统默认 clang
    if which("clang").is_ok() {
        return "clang".to_string();
    }
    eprintln!("错误：未找到 clang 编译器。请安装 clang，或设置 LIGHTC_CLANG 环境变量指定 clang 路径。");
    std::process::exit(1);
}

/// 检测 clang 的默认 target triple，用于嵌入 IR 避免 -Woverride-module 警告。
/// 若检测失败则返回空字符串，此时 IR 不指定 target triple。
fn detect_target_triple(clang: &str) -> String {
    if let Ok(out) = Command::new(clang).arg("-print-target-triple").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    // 兜底：尝试 -dumpmachine
    if let Ok(out) = Command::new(clang).arg("-dumpmachine").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}

/// 检查命令是否存在于 PATH 中
fn which(cmd: &str) -> Result<std::path::PathBuf, ()> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(cmd);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(())
}

fn find_lightrt() -> String {
    // 可执行文件所在目录，用于定位同目录的运行时库
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let mut candidates: Vec<String> = vec![
        "target/release/liblightrt.a".into(),
        "target/debug/liblightrt.a".into(),
        "../target/release/liblightrt.a".into(),
        "../target/debug/liblightrt.a".into(),
        "lib/liblightrt.a".into(),
        "liblightrt.a".into(),
    ];
    // 相对于可执行文件目录的候选路径
    if let Some(ref d) = exe_dir {
        candidates.push(d.join("liblightrt.a").to_string_lossy().into());
        candidates.push(d.join("lib").join("liblightrt.a").to_string_lossy().into());
        candidates.push(d.join("..").join("lib").join("liblightrt.a").to_string_lossy().into());
    }
    // 绝对路径兜底（开发环境）
    candidates.push("/workspace/light/lang/target/release/liblightrt.a".into());
    candidates.push("/workspace/light/lang/target/debug/liblightrt.a".into());

    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return c.clone();
        }
    }
    eprintln!("警告：未找到 lightrt 静态库，将尝试默认路径 target/release/liblightrt.a");
    "target/release/liblightrt.a".to_string()
}
