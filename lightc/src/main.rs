//! nlightc —— nextlight 套件的 Light 编译器
//!
//! 用法：nlightc <输入.light> [-o 输出] [--emit-llvm] [--emit-asm] [--no-link]
//!
//! 流程：.light 源 → 词法 → 语法 → LLVM IR → clang-18 编译为机器码
//!       与 nlightRT（Rust 内存安全运行时）静态链接

mod alias;
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

/// nextlight 套件的帮助
fn print_nextlight_help() {
    let v = env!("CARGO_PKG_VERSION");
    println!("nlightc {} —— nextlight 套件的 Light 编译器", v);
    println!();
    println!("nextlight 套件：");
    println!("  nlightc    编译器（本程序；用 Light 写的版本即自举编译器）");
    println!("  nlightRT   运行时（用 Light 实现，替代 lightrt）");
    println!("  nlightGo   工程工具（脚手架、依赖、构建）");
    println!("  nlight     交互环境与脚本入口");
    println!();
    println!("用法：nlightc <输入.light> [选项]");
    println!("  -o 输出              输出可执行文件（默认与输入同名）");
    println!("      --self-hosted    用自举编译器（用 Light 写的 nlightc）做代码生成");
    println!("      --runtime=next   链接 nlightRT（用 Light 实现的运行时）");
    println!("      --native 名称=路径  绑定外部函数到静态库");
    println!("      --page-out 文件   把 页面 生成的 HTML 静态导出到该文件
      --no-lightrt     不链接任何运行时");
    println!("      --emit-llvm      只输出 LLVM IR");
    println!("      --emit-asm       只输出汇编");
    println!("      --no-link        只生成中间文件，不链接");
    println!("      --debug          启用断言");
    println!("      --quiet          安静模式");
    println!("      --version        版本信息");
}

/// 用自举编译器（用 Light 写的 nlightc）生成 LLVM IR：
/// 把源文件从标准输入喂给它，它把 IR 写到标准输出。
fn self_hosted_codegen(input: &std::path::Path) -> Result<String, String> {
    let binary = match env::var("NLIGHTC") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            let local = std::env::current_dir()
                .unwrap_or_default()
                .join("target/bootstrap/stage2");
            if local.exists() {
                local.to_string_lossy().into_owned()
            } else {
                "nlightc".to_string()
            }
        }
    };
    let source = fs::read_to_string(input).map_err(|e| format!("读取 {} 失败：{}", input.display(), e))?;
    // 自举编译器按当前工作目录解析 导入，所以让它在源文件所在目录里跑
    let work_dir = input.parent().unwrap_or(std::path::Path::new("."));
    let mut child = Command::new(&binary)
        .current_dir(work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("启动自举编译器 {} 失败：{}", binary, e))?;
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or("无法写入子进程标准输入")?;
        stdin
            .write_all(source.as_bytes())
            .map_err(|e| format!("写入源码失败：{}", e))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("等待自举编译器失败：{}", e))?;
    if !out.status.success() {
        return Err("自举编译器返回失败".to_string());
    }
    let ir = String::from_utf8_lossy(&out.stdout).into_owned();
    if ir.is_empty() {
        return Err("自举编译器没有输出".to_string());
    }
    Ok(ir)
}

/// 找 nlightRT（Light 实现的运行时）静态库
fn find_nlightrt() -> String {
    if let Ok(p) = env::var("NLIGHTRT") {
        if !p.is_empty() {
            return p;
        }
    }
    let local = std::env::current_dir()
        .unwrap_or_default()
        .join("target/libnlightrt.a");
    if local.exists() {
        return local.to_string_lossy().into_owned();
    }
    "libnlightrt.a".to_string()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && matches!(args[1].as_str(), "--version" | "-V") {
        println!(
            "nlightc {}（nextlight 套件：nlightc 编译器 / nlightRT 运行时 / nlightGo 工程工具 / nlight 交互环境）",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h") {
        print_nextlight_help();
        return;
    }
    if args.len() < 2 {
            eprintln!("用法：nlightc <输入.light> [-o 输出] [--self-hosted] [--runtime=next|rust] [--native 名称=静态库路径] [--no-lightrt] [--emit-llvm] [--emit-asm] [--no-link] [--debug] [--quiet] [--repl-line 行号] [--page-out 文件]");
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
    let mut repl_line: Option<i64> = None;
    let mut page_output: Option<String> = None;
    let mut quiet = false;
    let mut self_hosted = false;
    let mut runtime_nextlight = false;

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
            "--self-hosted" => self_hosted = true,
            "--runtime=next" => runtime_nextlight = true,
            "--emit-llvm" => emit_llvm = true,
            "--emit-asm" => emit_asm = true,
            "--no-link" => no_link = true,
            "--page-out" => {
                i += 1;
                match args.get(i) {
                    Some(value) => page_output = Some(value.clone()),
                    None => {
                        eprintln!("--page-out 需要输出路径");
                        std::process::exit(1);
                    }
                }
            }
            "--no-lightrt" => no_lightrt = true,
            "--debug" => debug_assertions = true,
            "--quiet" | "-q" => quiet = true,
            "--repl-line" => {
                i += 1;
                let raw = match args.get(i) {
                    Some(value) => value,
                    None => {
                        eprintln!("--repl-line 需要行号");
                        std::process::exit(1);
                    }
                };
                match raw.parse::<i64>() {
                    Ok(line) => repl_line = Some(line),
                    Err(_) => {
                        eprintln!("无效的 --repl-line 参数：{}", raw);
                        std::process::exit(1);
                    }
                }
            }
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
    preload_aliases(&input_path, quiet);

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
    let mut env = typeinfer::infer(&program);
    // 显式标注优先于推断结果
    for item in &program.items {
        if let ast::Item::Fn(f) = item {
            for (index, name) in f.param_types.iter().enumerate() {
                if let Some(text) = name {
                    if let Some(ty) = codegen::type_by_name(text) {
                        env.param_types
                            .insert((f.name.clone(), index), ty);
                    }
                }
            }
            if let Some(text) = &f.return_type {
                if let Some(ty) = codegen::type_by_name(text) {
                    env.return_types.insert(f.name.clone(), ty);
                }
            }
        }
    }

    // IR 的 target triple 必须是 nlightc 自身的目标：
    // 交叉场景（例如在 x86_64 上跑 ARM 版 nlightc）下用宿主 clang 的 triple 会链接失败
    let (clang, clang_lead) = clang_command();
    // LIGHTC_CLANG_TARGET 可覆盖目标三元组，默认取 lightc 自身的构建目标
    let mut target_triple = std::env::var("LIGHTC_CLANG_TARGET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("LIGHTC_TARGET_TRIPLE").to_string());
    if target_triple.is_empty() {
        target_triple = detect_target_triple(&clang);
    }
    // raw_triple 保留 Rust 形式（用于找 target/<三元组>/… 目录），
    // target_triple 去掉 -unknown- 厂商字段后给 clang / zig
    let raw_triple = target_triple.clone();
    let target_triple = normalize_triple(&target_triple);
    // clang 参数顺序：用户自定义参数 → --target → LIGHTC_CLANG_ARGS
    // （顺序很重要，例如 LIGHTC_CLANG="zig cc -target …" 时子命令必须在前）
    let mut clang_args = clang_lead;
    let has_target = clang_args
        .iter()
        .chain(clang_extra_args().iter())
        .any(|a| a == "--target" || a.starts_with("--target="));
    if !has_target {
        clang_args.push(format!("--target={}", target_triple));
    }
    clang_args.extend(clang_extra_args());

    // --self-hosted：代码生成交给用 Light 写的自举编译器（nlightc）
    let ir = if self_hosted {
        match self_hosted_codegen(&input_path) {
            Ok(ir) => ir,
            Err(e) => {
                eprintln!("nlightc 错误：{}", e);
                std::process::exit(1);
            }
        }
    } else {
        let mut cg = codegen::CodeGen::new(&env, target_triple.clone(), debug_assertions, repl_line, page_output.clone());
        match cg.gen(&program) {
            Ok(ir) => ir,
            Err(e) => {
                eprintln!("代码生成错误：{}", e);
                std::process::exit(1);
            }
        }
    };

    // 写 .ll（临时文件，编译后自动删除）
    let ll_path = std::env::temp_dir()
        .join(format!("nlightc-{}-{}.ll", std::process::id(), output_stem))
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

    // 定位运行时静态库：--runtime=next 用 Light 实现的 nlightRT
    let rt_lib = if no_lightrt {
        String::new()
    } else if runtime_nextlight {
        find_nlightrt()
    } else {
        find_lightrt(&raw_triple)
    };

    // nlightWeb 静态库：网页内建（页面重置/元素*/路由 等）要用它
    let web_lib = if no_lightrt {
        String::new()
    } else {
        find_lightweb(&raw_triple)
    };

    // 用 clang 编译 .ll → 可执行文件
    let out_exe = output.unwrap_or_else(|| output_stem.clone());

    if emit_asm {
        // 只生成汇编
        let asm_path = format!("{}.s", out_exe);
        let status = Command::new(&clang)
            .args(&clang_args)
            .args(["-S", "-O2", "-Wno-override-module", "-o", &asm_path, &ll_path])
            .status()
            .expect("clang 执行失败");
        if !status.success() {
            std::process::exit(1);
        }
        if !quiet {
            println!("汇编已生成：{}", asm_path);
        }
        let _ = fs::remove_file(&ll_path);
        if no_link {
            return;
        }
        // 汇编 + 链接（汇编阶段不需要 -O2）
        let obj_path = format!("{}.o", out_exe);
        let s2 = Command::new(&clang)
            .args(&clang_args)
            .args(["-c", "-Wno-override-module", "-o", &obj_path, &asm_path])
            .status()
            .expect("clang 汇编失败");
        if !s2.success() {
            std::process::exit(1);
        }
        link(
            &clang,
            &clang_args,
            &obj_path,
            &rt_lib,
            &out_exe,
            &extra_libs,
            !no_lightrt,
            &web_lib,
        );
        if !quiet {
            println!("可执行文件：{}", out_exe);
        }
        let _ = fs::remove_file(&ll_path);
        let _ = fs::remove_file(&obj_path);
        let _ = fs::remove_file(&asm_path);
        return;
    }

    if no_link {
        let obj_path = format!("{}.o", out_exe);
        let status = Command::new(&clang)
            .args(&clang_args)
            .args(["-c", "-O2", "-Wno-override-module", "-o", &obj_path, &ll_path])
            .status()
            .expect("clang 执行失败");
        if !status.success() {
            std::process::exit(1);
        }
        if !quiet {
            println!("目标文件：{}", obj_path);
        }
        let _ = fs::remove_file(&ll_path);
        return;
    }

    // 完整编译 + 链接
    let obj_path = format!("{}.o", out_exe);
    let status = Command::new(&clang)
        .args(&clang_args)
        .args(["-c", "-O2", "-Wno-override-module", "-o", &obj_path, &ll_path])
        .status()
        .expect("clang 执行失败");
    if !status.success() {
        std::process::exit(1);
    }
    link(
        &clang,
        &clang_args,
        &obj_path,
        &rt_lib,
        &out_exe,
        &extra_libs,
        !no_lightrt,
        &web_lib,
    );
    if !quiet {
        println!("可执行文件：{}", out_exe);
    }
    let _ = fs::remove_file(&ll_path);
    let _ = fs::remove_file(&obj_path);
}

fn link(
    clang: &str,
    clang_args: &[String],
    obj: &str,
    rt_lib: &str,
    out: &str,
    extra_libs: &[String],
    include_lightrt: bool,
    web_lib: &str,
) {
    // nlightRT 是 Rust staticlib，需要链接 C++ 运行时与 pthread
    let mut args: Vec<&str> = Vec::new();
    for extra in clang_args {
        args.push(extra);
    }
    args.push(obj);
    args.extend(extra_libs.iter().map(|lib| lib.as_str()));
    if include_lightrt {
        args.push(rt_lib);
    }
    // nlightWeb 静态库要排在 lightrt 之后：它依赖 lightrt 的符号。
    // 两个都是 Rust staticlib，各自带着一份 std，重复定义会链接失败；
    // 允许重复定义后，先出现的 lightrt 那份生效，语义一致。
    let allow_dup = !web_lib.is_empty();
    if !web_lib.is_empty() {
        args.push(web_lib);
    }
    if allow_dup {
        args.push("-Wl,--allow-multiple-definition");
    }
    args.extend(["-o", out, "-lstdc++", "-lpthread", "-ldl", "-lm"]);
    let status = Command::new(clang)
        .args(&args)
        .status()
        .expect("链接失败");
    if !status.success() {
        let triple = env!("LIGHTC_TARGET_TRIPLE");
        eprintln!("错误：链接失败。");
        if is_cross(triple) {
            eprintln!("  当前 nlightc 目标为 {}，与本机架构不同。", triple);
            eprintln!("  交叉编译需要目标架构的 C 工具链，例如用 zig 当驱动：");
            eprintln!("    LIGHTC_CLANG=\"zig cc -target {}\" lightc 程序.light -o 程序", zig_triple(triple));
            eprintln!("  也可以用 LIGHTC_CLANG_ARGS 追加参数（--target、--sysroot、-fuse-ld 等）。");
        }
        std::process::exit(1);
    }
}

/// 把 Rust 三元组转成 C 工具链常用形式（去掉 -unknown- 厂商字段）
fn normalize_triple(triple: &str) -> String {
    triple.replace("-unknown-", "-")
}

/// 判断 nlightc 目标是否与本机不同
fn is_cross(triple: &str) -> bool {
    let host = detect_target_triple("clang");
    !triple.is_empty() && !host.is_empty() && !triple.starts_with(&host)
}

/// 提示信息里用的 zig 目标名
fn zig_triple(triple: &str) -> String {
    normalize_triple(triple)
}

/// 编译 .rs 模块为静态库，返回 .a 文件路径
/// 预扫描源文件里的 `导入 "xxx.llang"`，装载别名。
/// 必须先于任何词法/语法分析跑完，否则主文件里的别名根本没机会生效。
/// 扩展名不能省：不写就分不清是 .light 模块、.rs 原生模块还是别名文件。
/// 源文件头的语言声明。必须写在最前面，形式为：
///
///     语言 : 中文          Lang : Zh-cn
///     语言 : 英文          Lang : Eng
///     语言 : 来自文件(./One.llang)   Lang : File(./One.llang)
///
/// 声明之前只允许空行与 `#` 注释。中文是默认，什么都不写也行。
fn language_declaration(source: &str, base_dir: &std::path::Path) -> Option<String> {
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return None;
        };
        let key = key.trim();
        let value = value.trim();
        let spec = match key {
            "语言" | "Lang" => value,
            _ => return None,
        };
        match load_language_aliases(spec, base_dir)
        {
            Ok(desc) => return Some(desc),
            Err(e) => {
                eprintln!("语言声明有误：{}", e);
                std::process::exit(1);
            }
        }
    }
    None
}

fn load_language_aliases(spec: &str, base_dir: &std::path::Path) -> Result<String, String> {
    let spec = spec.trim().trim_matches('"');
    match spec {
        "中文" | "zh-cn" | "Zh-cn" | "ZH-CN" => Ok("中文（默认，未装载别名）".to_string()),
        "英文" | "eng" | "Eng" | "En" | "en" => {
            let n = alias::load_english();
            Ok(format!("英文（内建别名 {} 条）", n))
        }
        other => {
            let path = other
                .strip_prefix("来自文件(")
                .or_else(|| other.strip_prefix("File("))
                .and_then(|rest| rest.strip_suffix(')'))
                .ok_or_else(|| {
                    format!(
                        "无法识别的写法 {}，可用：中文 / 英文 / 来自文件(路径) 或 Lang : Zh-cn / Eng / File(路径)",
                        other
                    )
                })?;
            let full = base_dir.join(path);
            let text = std::fs::read_to_string(&full)
                .map_err(|e| format!("读取 {} 失败：{}", full.display(), e))?;
            let n = alias::load_llang(&text).map_err(|e| format!("别名文件有误：{}", e))?;
            Ok(format!("来自文件 {}（{} 条）", path, n))
        }
    }
}

/// 去掉源文件头的语言声明行，让词法器看不到它。
/// 声明之前只允许空行与注释，所以只查最前面那段。
fn strip_language_declaration(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut done = false;
    for line in source.lines() {
        let trimmed = line.trim();
        let is_decl = !done
            && (trimmed.starts_with("语言") || trimmed.starts_with("Lang"))
            && trimmed.contains(':');
        if is_decl {
            done = true;
            continue;
        }
        if !done && !trimmed.is_empty() && !trimmed.starts_with('#') {
            done = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn preload_aliases(file_path: &std::path::Path, quiet: bool) {
    let Ok(canonical) = std::fs::canonicalize(file_path) else {
        return;
    };
    let Ok(source) = std::fs::read_to_string(&canonical) else {
        return;
    };
    let base_dir = canonical.parent().unwrap_or(Path::new(".")).to_path_buf();
    if !quiet {
        if let Some(decl) = language_declaration(&source, &base_dir) {
            eprintln!("源文件声明语言：{}", decl);
        }
    }
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("导入") else {
            continue;
        };
        let Some(inner) = rest.trim().strip_prefix('"') else {
            continue;
        };
        let Some(end) = inner.find('"') else {
            continue;
        };
        let module = &inner[..end];
        if !module.ends_with(".llang") {
            continue;
        }
        let path = base_dir.join(module);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                eprintln!("读取别名文件失败：{}", path.display());
                std::process::exit(1);
            }
        };
        match alias::load_llang(&text) {
            Ok(0) => {
                eprintln!("别名文件 {} 里没有任何声明", path.display());
                std::process::exit(1);
            }
            Ok(n) => {
                if !quiet {
                    eprintln!("已装载别名 {} 条（{}）", n, module);
                }
            }
            Err(e) => {
                eprintln!("别名文件有误（{}）：{}", path.display(), e);
                std::process::exit(1);
            }
        }
    }
}

/// 类型安全桥接层的位置：与编译器放在一起，供 .rs 原生模块自动引入
fn light_ffi_path() -> Option<std::path::PathBuf> {
    // 环境变量优先，便于自定义安装布局
    if let Some(custom) = std::env::var_os("LIGHT_FFI_BRIDGE") {
        let p = std::path::PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }
    let rel = std::path::Path::new("lightffi").join("light_ffi.rs");
    let exe_dir = std::env::current_exe().ok().and_then(|p| {
        p.parent().map(|d| d.to_path_buf())
    });
    // 开发布局：target/release/nlightc → 仓库根
    if let Some(dir) = &exe_dir {
        let mut up = dir.clone();
        for _ in 0..4 {
            let c = up.join(&rel);
            if c.exists() {
                return Some(c);
            }
            up = up.join("..").to_path_buf();
        }
        // 安装布局：<prefix>/bin/nlightc → <prefix>/share/light-lang/lightffi
        let c = dir.join("..").join("share").join("light-lang").join(&rel);
        if c.exists() {
            return Some(c);
        }
    }
    // 相对当前工作目录
    for base in ["", "..", "../.."] {
        let c = std::path::Path::new(base).join(&rel);
        if c.exists() {
            return Some(c);
        }
    }
    None
}

fn compile_rs_module(rs_path: &std::path::Path) -> Result<String, String> {
    let out_dir = std::env::temp_dir();
    let stem = rs_path.file_stem().unwrap().to_string_lossy().to_string();
    let lib_name = format!("lib{}.a", stem);
    let lib_path = out_dir.join(&lib_name);

    // 桥接层以内联方式预置：用户写 #[no_mangle] pub extern "C" fn 就能直接用
    // LightStr / LightArray / LightDyn，不用自己 extern 声明或加依赖。
    let entry = match light_ffi_path() {
        Some(bridge) => {
            // include! 是相对包含它的文件解析的，模块路径必须转成绝对路径
            let module_abs = std::fs::canonicalize(rs_path).unwrap_or_else(|_| rs_path.to_path_buf());
            let wrapper = out_dir.join(format!("{}_ffi_wrap.rs", stem));
            let wrapper_src = format!(
                "#[allow(dead_code, unused_imports)]\ninclude!({:?});\ninclude!({:?});\n",
                bridge.to_string_lossy(),
                module_abs.to_string_lossy()
            );
            std::fs::write(&wrapper, wrapper_src)
                .map_err(|e| format!("写入桥接包装失败: {}", e))?;
            wrapper
        }
        None => rs_path.to_path_buf(),
    };

    let status = Command::new("rustc")
        .args([
            "--crate-type", "staticlib",
            "-O",
            "--edition", "2021",
            "-o",
            lib_path.to_str().unwrap(),
            entry.to_str().unwrap(),
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

    let lexer = lexer::Lexer::new(&strip_language_declaration(&src));
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
                // 别名文件已在预扫描阶段消费，这里跳过，别拿 Light 语法去解析它
                if module_path.ends_with(".llang") {
                    continue;
                }
                // lightWeb 是内建虚拟模块：只用来开启网页语法，不对应磁盘文件
                if module_path == "lightWeb" {
                    continue;
                }
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
/// 追加给 clang 的参数（空格分隔），用于交叉编译等场景
fn clang_extra_args() -> Vec<String> {
    std::env::var("LIGHTC_CLANG_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(|item| item.to_string())
        .collect()
}

/// C 编译器：LIGHTC_CLANG 可以带参数，例如 "zig cc -target aarch64-linux-musl"
fn clang_command() -> (String, Vec<String>) {
    if let Ok(custom) = std::env::var("LIGHTC_CLANG") {
        let mut parts = custom.split_whitespace().map(|item| item.to_string());
        if let Some(program) = parts.next() {
            return (program, parts.collect());
        }
    }
    (find_clang(), Vec::new())
}

fn find_clang() -> String {
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

/// 定位 nlightWeb 静态库；找不到时返回空串，此时只用网页内建的程序会链接失败
fn find_lightweb(triple: &str) -> String {
    let own = normalize_triple(env!("LIGHTC_TARGET_TRIPLE"));
    let same_target = triple.is_empty() || own.is_empty() || own == triple;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let mut candidates: Vec<String> = Vec::new();
    if !same_target && !triple.is_empty() {
        candidates.push(format!("target/{}/release/liblightweb.a", triple));
        candidates.push(format!("target/{}/debug/liblightweb.a", triple));
    }
    if let Some(ref dir) = exe_dir {
        candidates.push(dir.join("liblightweb.a").to_string_lossy().into());
        candidates.push(dir.join("lib").join("liblightweb.a").to_string_lossy().into());
        candidates.push(
            dir.join("..")
                .join("lib")
                .join("liblightweb.a")
                .to_string_lossy()
                .into(),
        );
        candidates.push(
            dir.join("..")
                .join("..")
                .join("lib")
                .join("liblightweb.a")
                .to_string_lossy()
                .into(),
        );
    }
    if !triple.is_empty() {
        candidates.push(format!("target/{}/release/liblightweb.a", triple));
        candidates.push(format!("target/{}/debug/liblightweb.a", triple));
    }
    candidates.extend(
        [
            "target/release/liblightweb.a",
            "target/debug/liblightweb.a",
            "../target/release/liblightweb.a",
            "../target/debug/liblightweb.a",
            "lib/liblightweb.a",
            "liblightweb.a",
        ]
        .iter()
        .map(|item| item.to_string()),
    );

    for candidate in &candidates {
        if std::path::Path::new(candidate).exists() {
            return candidate.clone();
        }
    }
    String::new()
}

fn find_lightrt(triple: &str) -> String {
    // 与 nlightc 自身目标一致时才用 exe 同目录的 nlightRT；
    // 被 LIGHTC_CLANG_TARGET 覆盖时，那个库架构不对，要优先用目标自己的构建目录
    let own = normalize_triple(env!("LIGHTC_TARGET_TRIPLE"));
    let same_target = triple.is_empty() || own.is_empty() || own == triple;
    // 优先用 nlightc 自身所在目录的 nlightRT：交叉编译时（target/<三元组>/release）
    // 必须拿到同架构的静态库，否则链接会报 "is incompatible with ..."
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let mut candidates: Vec<String> = Vec::new();
    if !same_target && !triple.is_empty() {
        candidates.push(format!("target/{}/release/libnlightrt.a", triple));
        candidates.push(format!("target/{}/debug/libnlightrt.a", triple));
    }
    if let Some(ref dir) = exe_dir {
        candidates.push(dir.join("libnlightrt.a").to_string_lossy().into());
        candidates.push(dir.join("lib").join("libnlightrt.a").to_string_lossy().into());
        candidates.push(
            dir.join("..")
                .join("lib")
                .join("libnlightrt.a")
                .to_string_lossy()
                .into(),
        );
        // cargo 布局：target/<三元组>/release/nlightc 与 libnlightrt.a 同目录（已覆盖），
        // 以及 target/<三元组>/release/../../lib 之类的自定义安装布局
        candidates.push(
            dir.join("..")
                .join("..")
                .join("lib")
                .join("libnlightrt.a")
                .to_string_lossy()
                .into(),
        );
    }
    // 指定目标时，优先取该目标自己的构建目录（例如交叉编译）
    if !triple.is_empty() {
        candidates.push(format!("target/{}/release/libnlightrt.a", triple));
        candidates.push(format!("target/{}/debug/libnlightrt.a", triple));
    }
    // 开发环境：相对当前目录的候选
    candidates.extend(
        [
            "target/release/libnlightrt.a",
            "target/debug/libnlightrt.a",
            "../target/release/libnlightrt.a",
            "../target/debug/libnlightrt.a",
            "lib/libnlightrt.a",
            "libnlightrt.a",
        ]
        .iter()
        .map(|item| item.to_string()),
    );
    // 绝对路径兜底（开发环境）
    candidates.push("/workspace/light/lang/target/release/libnlightrt.a".into());
    candidates.push("/workspace/light/lang/target/debug/libnlightrt.a".into());

    for candidate in &candidates {
        if std::path::Path::new(candidate).exists() {
            return candidate.clone();
        }
    }
    eprintln!("警告：未找到 nlightRT 静态库，将尝试默认路径 target/release/libnlightrt.a");
    "target/release/libnlightrt.a".to_string()
}
