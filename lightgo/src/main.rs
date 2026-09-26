use serde::Deserialize;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize)]
struct Manifest {
    package: Package,
    #[serde(default)]
    ffi: Vec<Ffi>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    entry: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Ffi {
    name: String,
    manifest: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoManifest {
    package: CargoPackage,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
}

#[derive(Clone)]
struct BuildOptions {
    manifest_path: Option<PathBuf>,
    profile: String,
    target_dir: Option<PathBuf>,
    port: Option<i64>,
    cargo_args: Vec<String>,
    quiet: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lightGo 错误：{}", error);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-V") {
        println!("lightGo {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_help();
        return Ok(());
    }

    let raw_command = args.remove(0);
    if raw_command == "web" || raw_command == "网页" {
        if args.is_empty() {
            return Err("用法：lightGo web start|stop".into());
        }
        let action = args.remove(0);
        return web_command(&action, &args);
    }
    let command = match raw_command.as_str() {
        "build" | "构建" => "build",
        "run" | "运行" => "run",
        "clean" | "清理" => "clean",
        "delete" | "删除" => "delete",
        "new" | "新建" => "new",
        other => other,
    }
    .to_string();
    if command == "new" {
        return create_project(&args);
    }
    if command == "delete" {
        return delete_project(&args);
    }

    let options = parse_options(&args)?;
    let manifest_path = options
        .manifest_path
        .clone()
        .map_or_else(find_manifest, Ok)?;
    match command.as_str() {
        "build" => {
            let output = build_project(&manifest_path, &options)?;
            if !options.quiet {
                println!("构建完成：{}", output.display());
            }
            Ok(())
        }
        "run" => {
            let output = build_project(&manifest_path, &options)?;
            let status = Command::new(&output)
                .status()
                .map_err(|e| format!("运行失败：{}", e))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("程序退出状态：{}", status))
            }
        }
        "clean" => clean_project(&manifest_path, &options),
        other => Err(format!("未知命令：{}", other)),
    }
}

fn web_command(action: &str, args: &[String]) -> Result<(), String> {
    let options = parse_options(args)?;
    let manifest_path = options
        .manifest_path
        .clone()
        .map_or_else(find_manifest, Ok)?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    let state_dir = base.join(".lightGo");
    let pid_path = state_dir.join("web.pid");
    match action {
        "start" | "启动" => {
            let port = options.port.unwrap_or(8080);
            if !(1..=65535).contains(&port) {
                return Err("端口必须在 1 到 65535 之间".into());
            }
            if let Ok(value) = fs::read_to_string(&pid_path) {
                let pid = value.trim();
                let alive = Command::new("kill")
                    .args(["-0", pid])
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);
                if alive {
                    return Err(format!("lightWeb 已经在运行，PID：{}", pid));
                }
            }
            let output = build_project(&manifest_path, &options)?;
            fs::create_dir_all(&state_dir)
                .map_err(|e| format!("创建 Web 状态目录失败：{}", e))?;
            let log_path = state_dir.join("web.log");
            let log = fs::File::create(&log_path)
                .map_err(|e| format!("创建 Web 日志失败：{}", e))?;
            let child = Command::new(&output)
                .current_dir(base)
                .env("LIGHT_WEB_PORT", port.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
                .stderr(Stdio::from(log))
                .spawn()
                .map_err(|e| format!("启动 lightWeb 失败：{}", e))?;
            fs::write(&pid_path, child.id().to_string())
                .map_err(|e| format!("写入 Web PID 失败：{}", e))?;
            println!("lightWeb 已启动：http://127.0.0.1:{}", port);
            println!("停止服务：lightGo web stop");
            Ok(())
        }
        "stop" | "停止" => {
            let value = fs::read_to_string(&pid_path)
                .map_err(|_| "lightWeb 当前没有运行".to_string())?;
            let pid = value.trim();
            if pid.is_empty() {
                return Err("Web PID 文件无效".into());
            }
            let status = Command::new("kill")
                .args(["-TERM", pid])
                .status()
                .map_err(|e| format!("停止 lightWeb 失败：{}", e))?;
            let _ = fs::remove_file(&pid_path);
            if status.success() {
                println!("lightWeb 已停止");
                Ok(())
            } else {
                Err("lightWeb 进程未能停止".into())
            }
        }
        other => Err(format!("未知 Web 命令：{}", other)),
    }
}

fn parse_options(args: &[String]) -> Result<BuildOptions, String> {
    let mut options = BuildOptions {
        manifest_path: None,
        profile: "release".to_string(),
        target_dir: None,
        port: None,
        cargo_args: Vec::new(),
        quiet: false,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--manifest-path" => {
                index += 1;
                let value = args.get(index).ok_or("--manifest-path 缺少路径")?;
                options.manifest_path = Some(PathBuf::from(value));
            }
            "--release" => options.profile = "release".to_string(),
            "--debug" => options.profile = "debug".to_string(),
            "--profile" => {
                index += 1;
                options.profile = args.get(index).ok_or("--profile 缺少名称")?.clone();
            }
            "--target-dir" => {
                index += 1;
                options.target_dir = Some(PathBuf::from(
                    args.get(index).ok_or("--target-dir 缺少路径")?,
                ));
            }
            "--port" => {
                index += 1;
                let value = args.get(index).ok_or("--port 缺少数值")?;
                options.port = Some(value.parse::<i64>().map_err(|_| "--port 必须是整数")?);
            }
            "--offline" => options.cargo_args.push("--offline".to_string()),
            "--locked" => options.cargo_args.push("--locked".to_string()),
            "--frozen" => {
                options.cargo_args.push("--locked".to_string());
                options.cargo_args.push("--offline".to_string());
            }
            "-j" | "--jobs" => {
                options.cargo_args.push(args[index].clone());
                index += 1;
                let value = args.get(index).ok_or("--jobs 缺少数值")?;
                options.cargo_args.push(value.clone());
            }
            "--features" => {
                options.cargo_args.push("--features".to_string());
                index += 1;
                options.cargo_args.push(args.get(index).ok_or("--features 缺少值")?.clone());
            }
            "--no-default-features" => options.cargo_args.push("--no-default-features".to_string()),
            "--all-features" => options.cargo_args.push("--all-features".to_string()),
            "--config" => {
                options.cargo_args.push("--config".to_string());
                index += 1;
                options.cargo_args.push(args.get(index).ok_or("--config 缺少值")?.clone());
            }
            "--color" => {
                options.cargo_args.push("--color".to_string());
                index += 1;
                options.cargo_args.push(args.get(index).ok_or("--color 缺少值")?.clone());
            }
            "--timings" => options.cargo_args.push("--timings".to_string()),
            "-q" | "--quiet" => {
                options.quiet = true;
                options.cargo_args.push("--quiet".to_string());
            }
            "-v" | "--verbose" => options.cargo_args.push("--verbose".to_string()),
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("未知参数：{}", other)),
        }
        index += 1;
    }
    Ok(options)
}

fn find_manifest() -> Result<PathBuf, String> {
    let mut directory = env::current_dir().map_err(|e| format!("获取当前目录失败：{}", e))?;
    loop {
        let candidate = directory.join("lightGo.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !directory.pop() {
            break;
        }
    }
    Err("当前目录及父目录中找不到 lightGo.toml".into())
}

fn load_manifest(path: &Path) -> Result<(Manifest, PathBuf), String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    let manifest: Manifest = toml::from_str(&text)
        .map_err(|e| format!("解析 {} 失败：{}", path.display(), e))?;
    if manifest.package.name.trim().is_empty() {
        return Err("package.name 不能为空".into());
    }
    if manifest.package.version.trim().is_empty() {
        return Err("package.version 不能为空".into());
    }
    let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    Ok((manifest, base))
}

fn build_project(manifest_path: &Path, options: &BuildOptions) -> Result<PathBuf, String> {
    let (manifest, base) = load_manifest(manifest_path)?;
    let profile = if options.profile == "dev" {
        "debug"
    } else {
        options.profile.as_str()
    };
    let project_target = options
        .target_dir
        .clone()
        .map(|path| resolve_path(&base, &path))
        .unwrap_or_else(|| base.join("target"));
    let link_runtime = manifest.ffi.is_empty();
    let mut native_libs = Vec::new();

    for ffi in &manifest.ffi {
        let cargo_manifest = resolve_path(&base, &ffi.manifest);
        let cargo_name = cargo_package_name(&cargo_manifest)?;
        let target_dir = project_target.join(".lightGo").join(&ffi.name);
        let mut cargo = Command::new("cargo");
        cargo.arg("build");
        if profile == "release" {
            cargo.arg("--release");
        } else if profile != "debug" {
            cargo.arg("--profile").arg(profile);
        }
        cargo.arg("--manifest-path").arg(&cargo_manifest);
        cargo.arg("--target-dir").arg(&target_dir);
        for arg in &options.cargo_args {
            cargo.arg(arg);
        }
        let status = cargo
            .status()
            .map_err(|e| format!("执行 cargo 失败：{}", e))?;
        if !status.success() {
            return Err(format!("构建 FFI {} 失败", ffi.name));
        }
        let library = target_dir
            .join(profile)
            .join(format!("lib{}.a", cargo_name.replace('-', "_")));
        if !library.is_file() {
            return Err(format!("找不到 FFI 静态库：{}", library.display()));
        }
        native_libs.push((ffi.name.clone(), library));
    }

    let entry = resolve_path(&base, &manifest.package.entry);
    if !entry.is_file() {
        return Err(format!("找不到 Light 入口：{}", entry.display()));
    }
    let output = project_target
        .join(profile)
        .join(&manifest.package.name);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建输出目录失败：{}", e))?;
    }

    let lightc = env::var_os("LIGHTC").unwrap_or_else(|| "lightc".into());
    let mut command = Command::new(lightc);
    if !link_runtime {
        command.arg("--no-lightrt");
    }
    if profile == "debug" {
        command.arg("--debug");
    }
    command.arg(&entry).arg("-o").arg(&output);
    for (name, library) in native_libs {
        command.arg("--native").arg(format!("{}={}", name, library.display()));
    }
    let status = command
        .status()
        .map_err(|e| format!("执行 lightc 失败：{}", e))?;
    if !status.success() {
        return Err("lightc 编译失败".into());
    }
    Ok(output)
}

fn cargo_package_name(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    let manifest: CargoManifest = toml::from_str(&text)
        .map_err(|e| format!("解析 {} 失败：{}", path.display(), e))?;
    Ok(manifest.package.name)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn clean_project(manifest_path: &Path, options: &BuildOptions) -> Result<(), String> {
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    let project_target = options
        .target_dir
        .clone()
        .map(|path| resolve_path(base, &path))
        .unwrap_or_else(|| base.join("target"));
    for path in [project_target, base.join(".lightGo").join("target")] {
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("清理 {} 失败：{}", path.display(), e))?;
        }
    }
    Ok(())
}

fn delete_project(args: &[String]) -> Result<(), String> {
    let mut project_path: Option<PathBuf> = None;
    let mut assume_yes = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--yes" | "-y" | "--force" => assume_yes = true,
            value if value.starts_with('-') => return Err(format!("未知参数：{}", value)),
            value => {
                if project_path.is_some() {
                    return Err("删除命令只能指定一个项目目录".into());
                }
                project_path = Some(PathBuf::from(value));
            }
        }
        index += 1;
    }
    let requested = project_path.unwrap_or_else(|| PathBuf::from("."));
    let target = fs::canonicalize(&requested)
        .map_err(|e| format!("找不到项目目录 {}：{}", requested.display(), e))?;
    if !target.is_dir() {
        return Err(format!("不是项目目录：{}", target.display()));
    }
    if target.parent().is_none() {
        return Err("拒绝删除根目录".into());
    }
    if !target.join("lightGo.toml").is_file() {
        return Err(format!("目录中没有 lightGo.toml：{}", target.display()));
    }
    if !assume_yes {
        print!("确认删除项目 {} 吗？输入 y 确认：", target.display());
        io::stdout().flush().map_err(|e| format!("显示确认提示失败：{}", e))?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|e| format!("读取确认失败：{}", e))?;
        if !matches!(answer.trim(), "y" | "Y" | "是") {
            println!("已取消删除");
            return Ok(());
        }
    }
    fs::remove_dir_all(&target)
        .map_err(|e| format!("删除项目失败：{}", e))?;
    println!("项目已删除：{}", target.display());
    Ok(())
}

fn create_project(args: &[String]) -> Result<(), String> {
    let name = args.first().ok_or("用法：lightGo 新建 <项目名>")?;
    let root = PathBuf::from(name);
    let package_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("项目名无效")?;
    fs::create_dir_all(root.join("src"))
        .map_err(|e| format!("创建项目失败：{}", e))?;
    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.2.1\"\nentry = \"src/main.light\"\n",
        package_name
    );
    fs::write(root.join("lightGo.toml"), manifest)
        .map_err(|e| format!("写入 lightGo.toml 失败：{}", e))?;
    fs::write(root.join("src/main.light"), "打印 \"你好，LightGo！\"\n")
        .map_err(|e| format!("写入入口失败：{}", e))?;
    println!("项目已创建：{}", root.display());
    Ok(())
}

fn print_help() {
    println!("用法：lightGo <构建|运行|清理|删除|新建> [选项]");
    println!("Web 命令：lightGo web start|stop 或 lightGo 网页 启动|停止 [--port 端口]");
    println!("常用选项：--release --debug --profile <名称> --target-dir <目录>");
    println!("Cargo 选项：--offline --locked --frozen -j/--jobs --features --all-features");
    println!("删除项目：lightGo 删除 <目录> [--yes]");
    println!("其他选项：--manifest-path <路径> -q/--quiet -v/--verbose");
    println!("英文兼容命令：build、run、clean、delete、new、web");
}
