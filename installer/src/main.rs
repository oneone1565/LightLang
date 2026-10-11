//! LightLang 自解压安装程序。
//!
//! 发行包（tar.gz）在编译期内嵌进本文件，运行后：
//!   1. 检测本地是否已有安装；有则比较版本，仅在更新时继续
//!   2. 把内嵌包释放到临时目录（或 `--target` 指定的目录）
//!   3. 调用包内的 `install.sh` 完成安装或更新
//!   4. 默认清理临时目录，`--keep` 可保留

use std::cmp::Ordering;
use std::env;
use std::fs;
use std::io::{stdin, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/package.tar.gz"));

/// 本地已有的安装
struct Installed {
    prefix: PathBuf,
    version: Option<String>,
}

fn main() {
    match run() {
        Ok(code) => exit(code),
        Err(error) => {
            eprintln!("安装程序错误：{}", error);
            exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    if PAYLOAD.is_empty() {
        return Err("该安装程序没有内嵌发行包，请在项目目录执行 scripts/package.sh 重新构建".into());
    }

    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut extract_only: Option<PathBuf> = None;
    let mut keep = false;
    let mut check_only = false;
    let mut force = false;
    let mut assume_yes = false;
    let mut prefix: Option<PathBuf> = None;
    let mut passthrough: Vec<String> = Vec::new();

    while !args.is_empty() {
        match args[0].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(0);
            }
            "-V" | "--version" => {
                println!("light-installer {}", VERSION);
                return Ok(0);
            }
            "-k" | "--keep" => {
                keep = true;
                args.remove(0);
            }
            "--check" => {
                check_only = true;
                args.remove(0);
            }
            "-f" | "--force" => {
                force = true;
                args.remove(0);
            }
            "-y" | "--yes" => {
                assume_yes = true;
                passthrough.push(args.remove(0));
            }
            "--update" => {
                force = true;
                args.remove(0);
            }
            "--prefix" => {
                if args.len() < 2 {
                    return Err("--prefix 缺少目录".into());
                }
                prefix = Some(PathBuf::from(args.remove(1)));
                args.remove(0);
            }
            "--target" => {
                if args.len() < 2 {
                    return Err("--target 缺少目录".into());
                }
                extract_only = Some(PathBuf::from(args.remove(1)));
                args.remove(0);
            }
            other => {
                passthrough.push(other.to_string());
                args.remove(0);
            }
        }
    }

    // 1. 检测本地安装并比较版本
    let existing = detect_install(prefix.as_deref());
    let mut target_prefix = prefix.clone();
    match &existing {
        Some(found) => {
            let old = found.version.clone().unwrap_or_else(|| "未知".to_string());
            let verdict = compare_versions(VERSION, &old);
            println!("检测到本地安装：{}", found.prefix.display());
            println!("本地版本：{}", old);
            println!("安装包版本：{}", VERSION);
            match verdict {
                Ordering::Greater => {
                    if check_only {
                        println!("有可用更新（{} -> {}）。", old, VERSION);
                        return Ok(0);
                    }
                    let proceed = if assume_yes {
                        true
                    } else if stdin().is_terminal() {
                        confirm(&format!("发现新版本 {}，是否更新？输入 y 继续：", VERSION))
                    } else {
                        println!("非交互环境：请加 -y 执行更新，或加 --check 只查看版本。");
                        return Ok(0);
                    };
                    if !proceed {
                        println!("已取消更新。");
                        return Ok(0);
                    }
                    println!("开始更新：{} -> {}", old, VERSION);
                    if target_prefix.is_none() {
                        target_prefix = Some(found.prefix.clone());
                    }
                }
                Ordering::Equal => {
                    if check_only {
                        println!("本地已是最新版本 {}。", VERSION);
                        return Ok(0);
                    }
                    if !force {
                        println!("本地已是 {}，无需更新；如需重装请加 --force。", VERSION);
                        return Ok(0);
                    }
                    println!("版本相同，按 --force 重新安装。");
                }
                Ordering::Less => {
                    if check_only {
                        println!("本地版本更新（{} > {}），无需降级。", old, VERSION);
                        return Ok(0);
                    }
                    if !force {
                        println!(
                            "本地版本 {} 高于安装包 {}，已跳过；如需降级请加 --force。",
                            old, VERSION
                        );
                        return Ok(0);
                    }
                    println!("按 --force 降级安装。");
                }
            }
        }
        None => {
            if check_only {
                println!("未检测到本地安装；安装包版本 {}。", VERSION);
                return Ok(0);
            }
            println!("未检测到本地安装，将执行全新安装。");
        }
    }

    // 2. 准备释放目录
    let work_dir = match &extract_only {
        Some(dir) => {
            fs::create_dir_all(dir).map_err(|e| format!("创建目录失败：{}", e))?;
            dir.clone()
        }
        None => env::temp_dir().join(format!("light-lang-installer-{}", std::process::id())),
    };
    fs::create_dir_all(&work_dir).map_err(|e| format!("创建目录失败：{}", e))?;
    let cleanup_temp = extract_only.is_none() && !keep;

    if let Some(dir) = &target_prefix {
        if !passthrough.iter().any(|item| item == "--prefix") {
            passthrough.push("--prefix".to_string());
            passthrough.push(dir.to_string_lossy().into_owned());
        }
    }

    let result = install(&work_dir, &passthrough, &extract_only);

    if cleanup_temp {
        let _ = fs::remove_dir_all(&work_dir);
    } else {
        println!("已保留释放目录：{}", work_dir.display());
    }
    result
}

/// 查找本地已有安装：优先指定前缀，其次 /usr/local 与 ~/.local
fn detect_install(prefix: Option<&Path>) -> Option<Installed> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = prefix {
        candidates.push(path.to_path_buf());
    } else {
        candidates.push(PathBuf::from("/usr/local"));
        if let Ok(home) = env::var("HOME") {
            candidates.push(PathBuf::from(home).join(".local"));
        }
    }
    candidates
        .into_iter()
        .find(|path| {
            // 新装用 nlightc；旧装仍是 lightc，两者都要认，升级时才不会漏检
            path.join("bin/nlightc").is_file()
                || path.join("bin/lightc").is_file()
                || path.join("share/light-lang/VERSION").is_file()
        })
        .map(|path| {
            let version = read_installed_version(&path);
            Installed { prefix: path, version }
        })
}

/// 读取已安装版本：优先 VERSION 标记文件，其次执行已安装的 lightc
fn read_installed_version(prefix: &Path) -> Option<String> {
    let marker = prefix.join("share/light-lang/VERSION");
    if let Ok(text) = fs::read_to_string(&marker) {
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    for binary in [prefix.join("bin/nlightc"), prefix.join("bin/lightc")] {
        if !binary.is_file() {
            continue;
        }
        if let Ok(output) = Command::new(&binary).arg("--version").output() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(version) = text.split_whitespace().nth(1) {
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    None
}

/// 比较版本号：0.2.10 高于 0.2.9；预发布版（0.2.1-rc1）低于正式版
fn compare_versions(left: &str, right: &str) -> Ordering {
    let split = |text: &str| -> (Vec<u64>, String) {
        let core = text
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let suffix = text
            .trim()
            .strip_prefix(core.as_str())
            .unwrap_or("")
            .trim_start_matches(['-', '+'])
            .to_string();
        let numbers = core
            .split('.')
            .map(|piece| piece.trim().parse::<u64>().unwrap_or(0))
            .collect();
        (numbers, suffix)
    };
    let (a, a_suffix) = split(left);
    let (b, b_suffix) = split(right);
    for index in 0..a.len().max(b.len()) {
        let x = a.get(index).copied().unwrap_or(0);
        let y = b.get(index).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    match (a_suffix.is_empty(), b_suffix.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a_suffix.cmp(&b_suffix),
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{}", prompt);
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim(), "y" | "Y" | "是")
}


fn install(
    work_dir: &Path,
    passthrough: &[String],
    extract_only: &Option<PathBuf>,
) -> Result<i32, String> {
    println!("LightLang {} 安装程序", VERSION);
    println!("正在释放发行包（{} 字节）...", PAYLOAD.len());
    let archive = work_dir.join("package.tar.gz");
    let mut file = fs::File::create(&archive).map_err(|e| format!("写入临时包失败：{}", e))?;
    file.write_all(PAYLOAD).map_err(|e| format!("写入临时包失败：{}", e))?;
    drop(file);

    let unpack_dir = work_dir.join("unpacked");
    fs::create_dir_all(&unpack_dir).map_err(|e| format!("创建解包目录失败：{}", e))?;
    let tar = match which("tar") {
        Some(path) => path,
        None => return Err("系统缺少 tar 命令，无法释放安装包".into()),
    };
    let status = Command::new(&tar)
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpack_dir)
        .status()
        .map_err(|e| format!("执行 tar 失败：{}", e))?;
    if !status.success() {
        return Err(format!("解包失败，tar 退出码 {}", status.code().unwrap_or(-1)));
    }
    let _ = fs::remove_file(&archive);

    // 找到包含 install.sh 的顶层目录
    let mut entries: Vec<PathBuf> = fs::read_dir(&unpack_dir)
        .map_err(|e| format!("读取解包目录失败：{}", e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    let root = entries
        .iter()
        .find(|path| path.join("install.sh").is_file())
        .cloned()
        .ok_or_else(|| "安装包结构异常：未找到 install.sh".to_string())?;

    for required in [
        "bin/nlight",
        "bin/nlightc",
        "bin/nlightGo",
        "lib/libnlightrt.a",
        "lib/libnlightRT.a",
    ] {
        if !root.join(required).is_file() {
            return Err(format!("安装包缺少文件：{}", required));
        }
    }

    if extract_only.is_some() {
        println!("安装包已释放到：{}", root.display());
        println!("可手动执行：{} --prefix /usr/local", root.join("install.sh").display());
        return Ok(0);
    }

    let script = root.join("install.sh");
    let mut permissions = fs::metadata(&script).map_err(|e| format!("读取安装脚本失败：{}", e))?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).map_err(|e| format!("设置脚本权限失败：{}", e))?;
    }
    #[cfg(not(unix))]
    let _ = permissions.set_readonly(false);

    println!("正在执行安装脚本...");
    let status = Command::new("bash")
        .arg(&script)
        .args(passthrough)
        .env("LIGHT_VERSION", VERSION)
        .current_dir(&root)
        .status()
        .map_err(|e| format!("执行安装脚本失败：{}", e))?;

    Ok(status.code().unwrap_or(1))
}

fn which(program: &str) -> Option<String> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

fn print_help() {
    println!("用法：light-installer [选项] [安装脚本参数...]");
    println!("把内嵌的 LightLang 发行包释放到临时目录，并执行其中的 install.sh。");
    println!("若检测到本地已有安装，会先比较版本，仅在需要时提示更新。");
    println!("选项：");
    println!("  --prefix <目录>  安装前缀，默认 /usr/local（已安装时沿用其前缀）");
    println!("  --check          只比较版本并显示结果，不做任何修改");
    println!("  --update         强制执行安装/更新流程");
    println!("  -f, --force      版本相同时也重装，或允许降级");
    println!("  -y, --yes        跳过确认（非交互环境必需）");
    println!("  --target <目录>  只释放安装包到指定目录，不执行安装");
    println!("  -k, --keep       保留释放出来的临时目录");
    println!("  -V, --version    显示版本");
    println!("  -h, --help       显示帮助");
    println!();
    println!("其余参数会原样传给 install.sh，例如：");
    println!("  light-installer --prefix \"$HOME/.local\" -y");
    println!("  light-installer --uninstall --prefix \"$HOME/.local\" -y");
}
