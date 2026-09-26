use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("light 错误：{}", error);
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut debug = false;
    let mut keep = false;
    while !args.is_empty() {
        match args[0].as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(0);
            }
            "--version" | "-V" => {
                println!("light {}", VERSION);
                return Ok(0);
            }
            "--debug" | "-d" => {
                debug = true;
                args.remove(0);
            }
            "--keep" | "-k" => {
                keep = true;
                args.remove(0);
            }
            "--" => {
                args.remove(0);
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("未知参数：{}", value));
            }
            _ => break,
        }
    }
    // 不带文件参数时进入交互模式，像 Python 一样逐句执行
    if args.is_empty() {
        return repl(debug);
    }

    let input = args.remove(0);
    if !PathBuf::from(&input).is_file() {
        return Err(format!("找不到 Light 文件：{}", input));
    }

    let temp_dir = make_temp_dir("light")?;
    let output = temp_dir.join("program");
    let result = build_and_run(&input, &output, debug, None, &args);
    if !keep {
        let _ = fs::remove_dir_all(&temp_dir);
    } else {
        println!("临时程序：{}", output.display());
    }
    result
}

/// 交互式 REPL：每读入一句就重新编译整段会话，并只显示当前这句的输出。
fn repl(debug: bool) -> Result<i32, String> {
    let temp_dir = make_temp_dir("light-repl")?;
    let binary = temp_dir.join("会话");
    // 会话源码放在当前目录，使 导入 指令像普通脚本一样按当前目录解析
    let cwd_source = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(format!(".light-repl-{}.light", std::process::id()));
    let source_path = match fs::write(&cwd_source, b"") {
        Ok(_) => cwd_source.clone(),
        Err(_) => temp_dir.join("会话.light"),
    };

    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    let mut input = stdin.lock();

    let mut history: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    // 已提交的顶层条目数：与编译器中的语句序号保持一致
    let mut slots = 0usize;
    // 结束块的那一行（缩进退回顶层时先执行块，再处理这一行）
    let mut carry: Option<String> = None;

    if interactive {
        println!("LightLang {} 交互模式（输入 退出 结束会话，输入 帮助 查看命令）", VERSION);
    }

    loop {
        let line = match carry.take() {
            Some(value) => value,
            None => {
                if interactive {
                    let prompt = if pending.is_empty() { ">>> " } else { "... " };
                    print!("{}", prompt);
                    let _ = io::stdout().flush();
                }
                let mut buffer = String::new();
                match input.read_line(&mut buffer) {
                    Ok(0) => {
                        if !pending.is_empty() {
                            // 输入结束时提交尚未结束的块
                            execute_block(
                                &mut history,
                                &mut pending,
                                &mut slots,
                                &source_path,
                                &binary,
                                debug,
                                interactive,
                            );
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => return Err(format!("读取输入失败：{}", error)),
                }
                buffer.trim_end_matches(['\n', '\r']).to_string()
            }
        };
        let trimmed = line.trim();

        // 块内遇到顶层语句或空行：先提交整个块，这一行留到下一轮
        if !pending.is_empty() && (trimmed.is_empty() || !is_indented(&line)) {
            if trimmed.is_empty() {
                execute_block(
                    &mut history,
                    &mut pending,
                    &mut slots,
                    &source_path,
                    &binary,
                    debug,
                    interactive,
                );
                continue;
            }
            execute_block(
                &mut history,
                &mut pending,
                &mut slots,
                &source_path,
                &binary,
                debug,
                interactive,
            );
            carry = Some(line);
            continue;
        }

        if pending.is_empty() {
            match trimmed {
                "" => continue,
                "退出" | "exit" | "quit" | ":q" => break,
                "帮助" | "help" | "?" => {
                    print_repl_help();
                    continue;
                }
                "清屏" | "clear" => {
                    if interactive {
                        print!("\x1b[2J\x1b[H");
                        let _ = io::stdout().flush();
                    }
                    continue;
                }
                "重置" | "reset" => {
                    history.clear();
                    slots = 0;
                    if interactive {
                        println!("会话已重置。");
                    }
                    continue;
                }
                "打印历史" | "history" => {
                    for (index, item) in history.iter().enumerate() {
                        println!("{:4}  {}", index + 1, item);
                    }
                    continue;
                }
                value if value.starts_with("保存 ") || value.starts_with("save ") => {
                    let target = value
                        .split_once(char::is_whitespace)
                        .map(|(_, rest)| rest.trim())
                        .unwrap_or("");
                    if target.is_empty() {
                        println!("用法：保存 <文件名.light>");
                    } else {
                        let mut text = String::new();
                        for item in history.iter() {
                            text.push_str(item);
                            text.push('\n');
                        }
                        match fs::write(target, text) {
                            Ok(_) => println!("已保存：{}", target),
                            Err(error) => println!("保存失败：{}", error),
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }

        pending.push(line);
        // 只有单行语句才立即执行；块等待空行或缩进回退
        if !pending[0].trim_end().ends_with(':') {
            execute_block(
                &mut history,
                &mut pending,
                &mut slots,
                &source_path,
                &binary,
                debug,
                interactive,
            );
        }
    }

    if interactive {
        println!();
    }
    let _ = fs::remove_file(&source_path);
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(0)
}

fn is_indented(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// 提交当前输入块：重放全部历史 + 本块，并只显示本块的输出。
#[allow(clippy::too_many_arguments)]
fn execute_block(
    history: &mut Vec<String>,
    pending: &mut Vec<String>,
    slots: &mut usize,
    source_path: &std::path::Path,
    binary: &std::path::Path,
    debug: bool,
    interactive: bool,
) {
    if pending.is_empty() {
        return;
    }
    let slot = *slots + 1;
    let mut source = String::new();
    for item in history.iter() {
        source.push_str(item);
        source.push('\n');
    }
    for item in pending.iter() {
        source.push_str(item);
        source.push('\n');
    }
    if let Err(error) = fs::write(source_path, &source) {
        eprintln!("light 错误：写入临时文件失败：{}", error);
        pending.clear();
        return;
    }
    match build_and_run(
        &source_path.to_string_lossy(),
        binary,
        debug,
        Some(slot as i64),
        &[],
    ) {
        Ok(0) => {
            history.append(pending);
            *slots += 1;
        }
        Ok(status) => {
            // 运行时失败：丢弃本句，允许继续改正
            pending.clear();
            if interactive {
                println!("（运行时退出码 {}，本句已丢弃）", status);
            }
        }
        Err(error) => {
            pending.clear();
            if interactive {
                println!("（{}，本句已丢弃）", error);
            }
        }
    }
}

fn lightc_binary() -> std::ffi::OsString {
    env::var_os("LIGHTC").unwrap_or_else(|| "lightc".into())
}

/// 编译并运行一个 Light 文件，返回程序的退出码。
fn build_and_run(
    input: &str,
    output: &std::path::Path,
    debug: bool,
    repl_line: Option<i64>,
    program_args: &[String],
) -> Result<i32, String> {
    let mut compiler = Command::new(lightc_binary());
    compiler.arg("--quiet");
    if debug {
        compiler.arg("--debug");
    }
    if let Some(line) = repl_line {
        compiler.arg("--repl-line").arg(line.to_string());
    }
    let status = compiler
        .arg(input)
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|e| format!("找不到 lightc：{}", e))?;
    if !status.success() {
        return Err(format!("lightc 退出码 {}", status.code().unwrap_or(-1)));
    }
    let result = Command::new(output)
        .args(program_args)
        .status()
        .map_err(|e| format!("运行 Light 程序失败：{}", e))?;
    Ok(result.code().unwrap_or(1))
}

fn make_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let dir = env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), stamp));
    fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败：{}", e))?;
    Ok(dir)
}

fn print_help() {
    println!("用法：light [选项] [文件.light] [程序参数...]");
    println!("不带文件时进入交互模式（REPL），逐句执行并保留变量。");
    println!("选项：");
    println!("  -d, --debug   启用 Debug 断言");
    println!("  -k, --keep    保留临时编译结果");
    println!("  -h, --help    显示帮助");
    println!("  -V, --version 显示版本");
    println!();
    println!("交互命令：退出 / 帮助 / 清屏 / 重置 / 打印历史 / 保存 <文件>");
}

fn print_repl_help() {
    println!();
    println!("交互命令：");
    println!("  退出 / exit   结束会话");
    println!("  帮助 / help   显示本帮助");
    println!("  清屏 / clear  清空屏幕");
    println!("  重置 / reset  清空当前会话变量");
    println!("  打印历史      显示已输入的语句");
    println!("  保存 <文件>   把当前会话保存为 Light 源文件");
    println!("  输入 变量 = 值 直接执行；函数、循环等以冒号结尾的块可跨行输入。");
}
