//! LightLang Windows 图形安装程序。
//!
//! 素材（MinGW 构建的 light/lightc/lightGo、liblightrt.a、示例与 Skill）在编译期内嵌进本文件。
//! 双击运行会打开图形安装界面；也可以用命令行：
//!   light-setup.exe --install "C:\Program Files\LightLang"
//!   light-setup.exe --uninstall "C:\Program Files\LightLang"
//!   light-setup.exe --check
//!
//! 只在 Windows 上提供图形界面；其他平台编译出的可执行文件仅提示不支持。

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::ptr;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{CreateFontW, UpdateWindow, HFONT};
    use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        REG_VALUE_TYPE,
    };
    use windows_sys::Win32::UI::Controls::EM_SETSEL;
    use windows_sys::Win32::System::Console::AttachConsole;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));
    const APP_NAME: &str = "LightLang";
    const UNINSTALL_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\LightLang";
    const PATH_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment";

    // 控件 ID
    const ID_EDIT: usize = 1001;
    const ID_BROWSE_PF: usize = 1002;
    const ID_BROWSE_LOCAL: usize = 1003;
    const ID_INSTALL: usize = 1004;
    const ID_UNINSTALL: usize = 1005;
    const ID_EXIT: usize = 1006;
    const ID_STATUS: usize = 1007;
    const ID_TITLE: usize = 1008;
    const ID_INFO: usize = 1009;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn program_files() -> String {
        let mut buffer = vec![0u16; 260];
        unsafe {
            let len = GetEnvironmentVariableW(wide("ProgramFiles").as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32);
            if len == 0 || len as usize >= buffer.len() {
                return "C:\\Program Files".to_string();
            }
            buffer.truncate(len as usize);
            String::from_utf16_lossy(&buffer)
        }
    }

    fn local_app_data() -> String {
        let mut buffer = vec![0u16; 260];
        unsafe {
            let len = GetEnvironmentVariableW(wide("LOCALAPPDATA").as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32);
            if len == 0 || len as usize >= buffer.len() {
                return "C:\\Users\\Public".to_string();
            }
            buffer.truncate(len as usize);
            String::from_utf16_lossy(&buffer)
        }
    }

    fn read_text(hwnd: HWND) -> String {
        let len = unsafe { GetWindowTextLengthW(hwnd) } as usize;
        if len == 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len + 1];
        unsafe {
            SendMessageW(hwnd, WM_GETTEXT, (len + 1) as WPARAM, buffer.as_mut_ptr() as LPARAM);
        }
        String::from_utf16_lossy(&buffer[..len])
    }

    fn set_text(hwnd: HWND, text: &str) {
        unsafe {
            SendMessageW(hwnd, WM_SETTEXT, 0, wide(text).as_ptr() as LPARAM);
        }
    }

    fn info_box(owner: HWND, text: &str, kind: u32) {
        unsafe {
            MessageBoxW(owner, wide(text).as_ptr(), wide(APP_NAME).as_ptr(), kind);
        }
    }

    fn status(hwnd: HWND, text: &str) {
        unsafe {
            let child = GetDlgItem(hwnd, ID_STATUS as i32);
            if !child.is_null() {
                SendMessageW(child, WM_SETTEXT, 0, wide(text).as_ptr() as LPARAM);
                UpdateWindow(child);
            }
            UpdateWindow(hwnd);
        }
    }

    // ---------------- 安装与卸载 ----------------

    fn extract_payload(dest: &Path) -> Result<(), String> {
        if PAYLOAD.is_empty() {
            return Err("该安装程序没有内嵌安装素材，请用 scripts/package-win.sh 重新构建".into());
        }
        let mut reader = zip::ZipArchive::new(Cursor::new(PAYLOAD)).map_err(|e| format!("读取安装素材失败：{}", e))?;
        for index in 0..reader.len() {
            let mut entry = reader.by_index(index).map_err(|e| format!("读取压缩条目失败：{}", e))?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let target = dest.join(&rel);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|e| format!("创建目录失败：{}", e))?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{}", e))?;
            }
            let mut out = fs::File::create(&target).map_err(|e| format!("写入文件失败：{}", e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败：{}", e))?;
        }
        Ok(())
    }

    fn read_machine_path() -> Option<String> {
        unsafe {
            let mut key: HKEY = ptr::null_mut();
            let status = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(PATH_KEY).as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut key,
            );
            if status != 0 {
                return None;
            }
            let mut data = vec![0u16; 4096];
            let mut size = (data.len() * 2) as u32;
            let mut kind: REG_VALUE_TYPE = REG_SZ;
            let code = RegQueryValueExW(
                key,
                wide("Path").as_ptr(),
                ptr::null_mut(),
                &mut kind,
                data.as_mut_ptr() as *mut u8,
                &mut size,
            );
            RegCloseKey(key);
            if code != 0 {
                return None;
            }
            let used = (size as usize / 2).saturating_sub(1);
            data.truncate(used);
            Some(String::from_utf16_lossy(&data))
        }
    }

    fn write_machine_path(value: &str) -> Result<(), String> {
        unsafe {
            let mut key: HKEY = ptr::null_mut();
            let status = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(PATH_KEY).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            );
            if status != 0 {
                return Err(format!("无法打开 PATH 注册表项（错误 {}），请以管理员身份运行", status));
            }
            let mut data = wide(value);
            let bytes = (data.len() * 2) as u32;
            let code = RegSetValueExW(
                key,
                wide("Path").as_ptr(),
                0,
                REG_EXPAND_SZ,
                data.as_mut_ptr() as *const u8,
                bytes,
            );
            RegCloseKey(key);
            if code != 0 {
                return Err(format!("写入 PATH 失败（错误 {}），请以管理员身份运行", code));
            }
            // 通知系统环境变量已变化
            let mut message = wide("Environment");
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                message.as_mut_ptr() as LPARAM,
                SMTO_ABORTIFHUNG,
                2000,
                ptr::null_mut(),
            );
            Ok(())
        }
    }

    fn add_to_path(dir: &Path) -> Result<bool, String> {
        let entry = dir.to_string_lossy().to_string();
        let current = read_machine_path().unwrap_or_default();
        let already = current
            .split(';')
            .any(|item| item.trim_end_matches('\\').eq_ignore_ascii_case(entry.trim_end_matches('\\')));
        if already {
            return Ok(false);
        }
        let updated = if current.trim().is_empty() { entry } else { format!("{};{}", current, entry) };
        write_machine_path(&updated)?;
        Ok(true)
    }

    fn remove_from_path(dir: &Path) -> Result<bool, String> {
        let entry = dir.to_string_lossy().trim_end_matches('\\').to_lowercase();
        let Some(current) = read_machine_path() else {
            return Ok(false);
        };
        let kept: Vec<&str> = current
            .split(';')
            .filter(|item| !item.trim().is_empty())
            .filter(|item| item.trim_end_matches('\\').to_lowercase() != entry)
            .collect();
        if kept.len() == current.split(';').filter(|item| !item.trim().is_empty()).count() {
            return Ok(false);
        }
        write_machine_path(&kept.join(";"))?;
        Ok(true)
    }

    fn write_uninstall_entry(dir: &Path) {
        unsafe {
            let mut key: HKEY = ptr::null_mut();
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(UNINSTALL_KEY).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            ) != 0
            {
                return;
            }
            let exe = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let set = |key: HKEY, name: &str, value: &str, kind: u32| {
                let mut data = wide(value);
                RegSetValueExW(key, wide(name).as_ptr(), 0, kind, data.as_mut_ptr() as *const u8, (data.len() * 2) as u32);
            };
            set(key, "DisplayName", &format!("{} {}", APP_NAME, VERSION), REG_SZ);
            set(key, "DisplayVersion", VERSION, REG_SZ);
            set(key, "Publisher", "LightLang", REG_SZ);
            set(key, "InstallLocation", &dir.to_string_lossy(), REG_SZ);
            set(key, "UninstallString", &format!("\"{}\" --uninstall", exe), REG_SZ);
            set(key, "NoModify", "1", REG_SZ);
            set(key, "NoRepair", "1", REG_SZ);
            RegCloseKey(key);
        }
    }

    fn remove_uninstall_entry() {
        unsafe {
            let mut key: HKEY = ptr::null_mut();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(UNINSTALL_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) == 0 {
                RegDeleteValueW(key, wide("DisplayName").as_ptr());
                RegCloseKey(key);
            }
        }
    }

    fn do_install(dir: &Path) -> Result<String, String> {
        extract_payload(dir)?;
        let bin = dir.join("bin");
        for tool in ["light.exe", "lightc.exe", "lightGo.exe"] {
            if !bin.join(tool).is_file() {
                return Err(format!("安装不完整：缺少 {}", tool));
            }
        }
        let appended = add_to_path(&bin)?;
        write_uninstall_entry(dir);
        Ok(if appended {
            format!("安装完成，并已把 {} 加入系统 PATH。", bin.display())
        } else {
            format!("安装完成（PATH 中已存在 {}）。", bin.display())
        })
    }

    fn do_uninstall(dir: &Path) -> Result<String, String> {
        let mut removed = false;
        for relative in ["bin", "lib", "share"] {
            let target = dir.join(relative);
            if target.exists() && fs::remove_dir_all(&target).is_ok() {
                removed = true;
            }
        }
        for relative in ["README.md", "LICENSE"] {
            let target = dir.join(relative);
            if target.is_file() && fs::remove_file(&target).is_ok() {
                removed = true;
            }
        }
        let path_cleaned = remove_from_path(&dir.join("bin")).unwrap_or(false);
        remove_uninstall_entry();
        if !removed && !path_cleaned {
            return Err(format!("{} 下没有找到已安装的文件", dir.display()));
        }
        Ok("卸载完成。".to_string())
    }

    fn is_installed(dir: &Path) -> Option<String> {
        let marker = dir.join("share").join("light-lang").join("VERSION");
        if let Ok(text) = fs::read_to_string(marker) {
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        if dir.join("bin").join("lightc.exe").is_file() {
            return Some("未知".to_string());
        }
        None
    }

    /// 权限不足时用 UAC 重新以管理员身份启动
    fn relaunch_elevated(args: &str) -> bool {
        let exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return false,
        };
        unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                wide("runas").as_ptr(),
                wide(&exe.to_string_lossy()).as_ptr(),
                wide(args).as_ptr(),
                ptr::null(),
                1,
            ) as isize > 32
        }
    }

    // ---------------- 图形界面 ----------------

    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_CREATE => {
                let instance = GetModuleHandleW(ptr::null());
                let title_font = CreateFontW(
                    -20, 0, 0, 0, 700, 0, 0, 0,
                    134,
                    0,
                    0,
                    0,
                    0,
                    wide("Microsoft YaHei").as_ptr(),
                );
                let info_font = CreateFontW(
                    -14, 0, 0, 0, 400, 0, 0, 0,
                    134,
                    0,
                    0,
                    0,
                    0,
                    wide("Microsoft YaHei").as_ptr(),
                );

                let create = |class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: usize, font: HFONT| -> HWND {
                    let handle = CreateWindowExW(
                        0,
                        wide(class).as_ptr(),
                        wide(text).as_ptr(),
                        WS_CHILD | WS_VISIBLE | style,
                        x,
                        y,
                        w,
                        h,
                        hwnd,
                        id as isize as *mut _,
                        instance,
                        ptr::null(),
                    );
                    if !handle.is_null() && !font.is_null() {
                        SendMessageW(handle, WM_SETFONT, font as WPARAM, 1);
                    }
                    handle
                };

                create("STATIC", &format!("{} {} 安装程序", APP_NAME, VERSION), 0, 20, 16, 480, 30, ID_TITLE, title_font);
                create(
                    "STATIC",
                    "安装内容：\n  lightc.exe   Light 编译器\n  light.exe    交互运行器（支持逐句执行）\n  lightGo.exe  项目构建工具\n  liblightrt.a 运行时静态库、示例与 Skill\n\n提示：lightc 编译 Light 程序时需要本机已安装 LLVM / clang。",
                    0,
                    20,
                    52,
                    480,
                    110,
                    ID_INFO,
                    info_font,
                );
                create("STATIC", "安装目录：", 0, 20, 178, 80, 20, 0, info_font);
                let edit = create("EDIT", "", WS_BORDER | (ES_AUTOHSCROLL as u32), 100, 175, 320, 26, ID_EDIT, info_font);
                SendMessageW(edit, EM_SETSEL, 0, -1);
                create("BUTTON", "程序目录", BS_PUSHBUTTON as u32, 424, 174, 76, 28, ID_BROWSE_PF, info_font);
                create("BUTTON", "用户目录", BS_PUSHBUTTON as u32, 424, 204, 76, 28, ID_BROWSE_LOCAL, info_font);

                create("BUTTON", "安装", BS_DEFPUSHBUTTON as u32, 20, 250, 100, 32, ID_INSTALL, info_font);
                create("BUTTON", "卸载", BS_PUSHBUTTON as u32, 132, 250, 100, 32, ID_UNINSTALL, info_font);
                create("BUTTON", "退出", BS_PUSHBUTTON as u32, 244, 250, 100, 32, ID_EXIT, info_font);
                create("STATIC", "就绪", 0, 20, 296, 480, 40, ID_STATUS, info_font);
                0
            }
            WM_COMMAND => {
                let id = (wparam & 0xFFFF) as usize;
                let control = GetDlgItem(hwnd, id as i32);
                match id {
                    ID_BROWSE_PF => {
                        let path = PathBuf::from(program_files()).join(APP_NAME);
                        set_text(control, &path.to_string_lossy());
                        status(hwnd, &format!("已选择：{}", path.display()));
                    }
                    ID_BROWSE_LOCAL => {
                        let path = PathBuf::from(local_app_data()).join("Programs").join(APP_NAME);
                        set_text(control, &path.to_string_lossy());
                        status(hwnd, &format!("已选择：{}", path.display()));
                    }
                    ID_INSTALL => {
                        let dir = PathBuf::from(read_text(control));
                        if dir.as_os_str().is_empty() {
                            info_box(hwnd, "请先选择安装目录。", MB_ICONWARNING);
                            return 0;
                        }
                        if let Some(version) = is_installed(&dir) {
                            let answer = unsafe {
                                MessageBoxW(
                                    hwnd,
                                    wide(&format!("{} 已安装（版本 {}），要覆盖安装吗？", dir.display(), version)).as_ptr(),
                                    wide(APP_NAME).as_ptr(),
                                    MB_YESNO | MB_ICONQUESTION,
                                )
                            };
                            if answer != IDYES {
                                return 0;
                            }
                        }
                        status(hwnd, "正在安装，请稍候……");
                        match do_install(&dir) {
                            Ok(message) => {
                                status(hwnd, &message);
                                info_box(hwnd, &format!("{}\n\n{}", message, "重新打开命令行后即可直接使用 light、lightc、lightGo。"), MB_ICONINFORMATION);
                            }
                            Err(error) => {
                                status(hwnd, "安装失败");
                                info_box(hwnd, &error, MB_ICONERROR);
                                if error.contains("管理员") {
                                    relaunch_elevated(&format!("--install \"{}\"", dir.display()));
                                }
                            }
                        }
                    }
                    ID_UNINSTALL => {
                        let dir = PathBuf::from(read_text(control));
                        let answer = unsafe {
                            MessageBoxW(
                                hwnd,
                                wide(&format!("确定要卸载 {} 吗？", dir.display())).as_ptr(),
                                wide(APP_NAME).as_ptr(),
                                MB_YESNO | MB_ICONQUESTION,
                            )
                        };
                        if answer != IDYES {
                            return 0;
                        }
                        status(hwnd, "正在卸载，请稍候……");
                        match do_uninstall(&dir) {
                            Ok(message) => {
                                status(hwnd, &message);
                                info_box(hwnd, &message, MB_ICONINFORMATION);
                            }
                            Err(error) => {
                                status(hwnd, "卸载失败");
                                info_box(hwnd, &error, MB_ICONERROR);
                            }
                        }
                    }
                    ID_EXIT => unsafe {
                        PostQuitMessage(0);
                    },
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                unsafe {
                    PostQuitMessage(0);
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn run_gui() -> i32 {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(ptr::null());
            let class = wide("LightLangSetupWindow");
            let window_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hIcon: LoadIconW(ptr::null_mut(), IDI_APPLICATION),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: 16usize as *mut core::ffi::c_void,
                lpszClassName: class.as_ptr(),
                hIconSm: ptr::null_mut(),
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&window_class) == 0 {
                info_box(ptr::null_mut(), "注册窗口失败。", MB_ICONERROR);
                return 1;
            }
            let hwnd = CreateWindowExW(
                0u32,
                class.as_ptr(),
                wide(&format!("{} {} 安装程序", APP_NAME, VERSION)).as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                540,
                400,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if hwnd.is_null() {
                info_box(ptr::null_mut(), "创建窗口失败。", MB_ICONERROR);
                return 1;
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);

            // 已有安装时预填目录
            let default_dir = PathBuf::from(program_files()).join(APP_NAME);
            let target = if is_installed(&default_dir).is_some() {
                default_dir
            } else {
                let local = PathBuf::from(local_app_data()).join("Programs").join(APP_NAME);
                if is_installed(&local).is_some() {
                    local
                } else {
                    default_dir
                }
            };
            let edit = GetDlgItem(hwnd, ID_EDIT as i32);
            if !edit.is_null() {
                set_text(edit, &target.to_string_lossy());
            }
            status(
                hwnd,
                &match is_installed(&target) {
                    Some(version) => format!("检测到已安装版本 {}，可直接覆盖更新。", version),
                    None => "尚未检测到已安装的 LightLang。".to_string(),
                },
            );

            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            0
        }
    }

    fn help_text() -> String {
        format!(
            "LightLang {} Windows 图形安装程序\n\n用法：\n  light-setup.exe                     打开图形安装界面\n  light-setup.exe --install <目录>    直接安装\n  light-setup.exe --uninstall <目录>  直接卸载\n  light-setup.exe --check [目录]     查看安装与版本信息\n  light-setup.exe --version           显示版本",
            VERSION
        )
    }

    /// 命令行模式的输出：有控制台则打印，否则用消息框显示
    fn emit(text: &str, attach: i32, error: bool) {
        if attach != 0 {
            if error {
                eprintln!("{}", text);
            } else {
                println!("{}", text);
            }
        } else {
            info_box(
                ptr::null_mut(),
                text,
                if error { MB_ICONERROR } else { MB_ICONINFORMATION },
            );
        }
    }

    pub fn main() -> i32 {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            return run_gui();
        }
        let attach = unsafe { AttachConsole(u32::MAX) };
        match args.first().map(|s| s.as_str()) {
            None => run_gui(),
            Some("-h") | Some("--help") => {
                emit(&help_text(), attach, false);
                0
            }
            Some("-V") | Some("--version") => {
                emit(&format!("light-setup {}", VERSION), attach, false);
                0
            }
            Some("--check") => {
                let dir = args
                    .get(1)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(program_files()).join(APP_NAME));
                match is_installed(&dir) {
                    Some(version) => {
                        emit(&format!("已安装：{}（版本 {}）", dir.display(), version), attach, false);
                        0
                    }
                    None => {
                        emit(&format!("未安装：{}", dir.display()), attach, true);
                        1
                    }
                }
            }
            Some("--install") => match args.get(1) {
                Some(dir) => match do_install(&PathBuf::from(dir)) {
                    Ok(message) => {
                        emit(&message, attach, false);
                        0
                    }
                    Err(error) => {
                        emit(&format!("安装失败：{}", error), attach, true);
                        if error.contains("管理员") {
                            relaunch_elevated(&format!("--install \"{}\"", dir));
                            return 0;
                        }
                        1
                    }
                },
                None => {
                    emit("--install 需要目录参数", attach, true);
                    2
                }
            },
            Some("--uninstall") => match args.get(1) {
                Some(dir) => match do_uninstall(&PathBuf::from(dir)) {
                    Ok(message) => {
                        emit(&message, attach, false);
                        0
                    }
                    Err(error) => {
                        emit(&format!("卸载失败：{}", error), attach, true);
                        1
                    }
                },
                None => {
                    emit("--uninstall 需要目录参数", attach, true);
                    2
                }
            },
            Some(other) => {
                emit(&format!("未知参数：{}\n\n{}", other, help_text()), attach, true);
                2
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("light-setup 只支持 Windows；请在 Windows 上运行，或使用 Linux 的 light-installer。");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    std::process::exit(windows::main());
}
