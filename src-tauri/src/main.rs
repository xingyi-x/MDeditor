// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod convert;
mod pdfexport;

use base64::Engine as _;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(debug_assertions)]
use tauri::Manager;

// ---------- 数据结构 ----------

#[derive(Serialize, Clone)]
struct EntryInfo {
    name: String,
    path: String,
    is_dir: bool,
    ext: String,
    size: u64,
    modified_ms: i64,
}

#[derive(Serialize, Clone)]
struct SearchHit {
    path: String,
    name: String,
    line: u64,
    column: u64,
    text: String,
}

struct AppState {
    startup_file: Mutex<Option<String>>,
    /// --pdf-out <路径>：启动后自动把当前文档导出成 PDF 再退出（自测 / 批处理用）
    pdf_out: Mutex<Option<String>>,
}

fn to_string_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn stat_entry(p: &Path) -> Option<EntryInfo> {
    let meta = fs::metadata(p).ok()?;
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned());
    let ext = p
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(EntryInfo {
        name,
        path: p.to_string_lossy().into_owned(),
        is_dir: meta.is_dir(),
        ext,
        size: meta.len(),
        modified_ms,
    })
}

// ---------- 编码处理 ----------

fn decode_bytes(bytes: &[u8]) -> String {
    // UTF-8 BOM
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // UTF-16 LE / BE BOM
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        return cow.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        return cow.into_owned();
    }
    // 严格 UTF-8，失败则回退 GBK（中文环境常见）
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (cow, _, _) = encoding_rs::GBK.decode(bytes);
            cow.into_owned()
        }
    }
}

// ---------- 命令：文件读写 ----------

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    let bytes = fs::read(&path).map_err(to_string_err)?;
    Ok(decode_bytes(&bytes))
}

#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(&path, content.as_bytes()).map_err(to_string_err)
}

#[tauri::command]
fn read_image_data_url(path: String) -> Result<String, String> {
    let bytes = fs::read(&path).map_err(to_string_err)?;
    let ext = Path::new(&path)
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

#[tauri::command]
fn write_base64_file(path: String, data: String) -> Result<(), String> {
    // 兼容 data:image/png;base64,xxxx 形式
    let payload = match data.split_once(",") {
        Some((head, rest)) if head.contains("base64") => rest.to_string(),
        _ => data,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(to_string_err)?;
    if let Some(parent) = Path::new(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, bytes).map_err(to_string_err)
}

// ---------- 命令：目录 ----------

#[tauri::command]
fn list_dir(path: String) -> Result<Vec<EntryInfo>, String> {
    let mut out: Vec<EntryInfo> = Vec::new();
    for entry in fs::read_dir(&path).map_err(to_string_err)? {
        let entry = entry.map_err(to_string_err)?;
        if let Some(info) = stat_entry(&entry.path()) {
            out.push(info);
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

#[tauri::command]
fn path_stat(path: String) -> Result<Option<EntryInfo>, String> {
    Ok(stat_entry(Path::new(&path)))
}

#[tauri::command]
fn path_exists(path: String) -> Result<bool, String> {
    Ok(Path::new(&path).exists())
}

#[tauri::command]
fn create_dir(path: String) -> Result<(), String> {
    fs::create_dir_all(&path).map_err(to_string_err)
}

#[tauri::command]
fn remove_path(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(to_string_err)
    } else {
        fs::remove_file(p).map_err(to_string_err)
    }
}

#[tauri::command]
fn rename_path(from: String, to: String) -> Result<(), String> {
    // Windows 上 fs::rename 遇到已存在的目标会失败，先给出可读的提示，
    // 否则用户只看到一句系统错误码。
    if Path::new(&to).exists() {
        return Err(format!("目标已存在：{}", to));
    }
    fs::rename(&from, &to).map_err(to_string_err)
}

#[tauri::command]
fn home_dir() -> String {
    std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into())
}

#[tauri::command]
fn sep() -> String {
    std::path::MAIN_SEPARATOR.to_string()
}

// ---------- 命令：查找 md 文件 ----------

fn is_junk_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | "node_modules" | ".idea" | ".vscode" | "__pycache__" | "target"
            | "dist" | ".venv" | "venv" | "$RECYCLE.BIN" | "System Volume Information"
    ) || name.starts_with('.')
}

#[tauri::command]
fn find_markdown_files(root: String, max_depth: u32) -> Result<Vec<EntryInfo>, String> {
    let mut out: Vec<EntryInfo> = Vec::new();
    fn walk(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<EntryInfo>) {
        if depth > max_depth || out.len() > 5000 {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        for p in items {
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if p.is_dir() {
                if !is_junk_dir(&name) {
                    walk(&p, depth + 1, max_depth, out);
                }
            } else {
                let ext = p
                    .extension()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ext == "md" || ext == "markdown" {
                    if let Some(info) = stat_entry(&p) {
                        out.push(info);
                    }
                }
            }
        }
    }
    walk(Path::new(&root), 0, max_depth.max(1).min(12), &mut out);
    Ok(out)
}

// ---------- 命令：全文搜索 ----------

#[tauri::command]
fn search_in_files(
    root: String,
    query: String,
    case_sensitive: bool,
    max_hits: u32,
) -> Result<Vec<SearchHit>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits: Vec<SearchHit> = Vec::new();
    let needle_lower = query.to_lowercase();
    fn walk(
        dir: &Path,
        depth: u32,
        state: &mut (u32, u32), // (已扫描文件数, 上限)
        query: &str,
        needle_lower: &str,
        case_sensitive: bool,
        max_hits: u32,
        hits: &mut Vec<SearchHit>,
    ) {
        if depth > 8 || hits.len() >= max_hits as usize || state.0 >= state.1 {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        for p in items {
            if hits.len() >= max_hits as usize {
                return;
            }
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if p.is_dir() {
                if !is_junk_dir(&name) {
                    walk(&p, depth + 1, state, query, needle_lower, case_sensitive, max_hits, hits);
                }
                continue;
            }
            let ext = p
                .extension()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ext != "md" && ext != "markdown" && ext != "txt" {
                continue;
            }
            state.0 += 1;
            if state.0 > state.1 {
                return;
            }
            let content = match fs::read(&p) {
                Ok(b) => decode_bytes(&b),
                Err(_) => continue,
            };
            for (i, line) in content.lines().enumerate() {
                let found = if case_sensitive {
                    line.contains(query)
                } else {
                    line.to_lowercase().contains(needle_lower)
                };
                if found {
                    let col = if case_sensitive {
                        line.find(query).map(|c| c as u64).unwrap_or(0)
                    } else {
                        line.to_lowercase().find(needle_lower).map(|c| c as u64).unwrap_or(0)
                    };
                    hits.push(SearchHit {
                        path: p.to_string_lossy().into_owned(),
                        name: name.clone(),
                        line: (i + 1) as u64,
                        column: col + 1,
                        text: line.chars().take(200).collect(),
                    });
                    if hits.len() >= max_hits as usize {
                        return;
                    }
                }
            }
        }
    }
    let mut state = (0u32, 3000u32);
    walk(
        Path::new(&root),
        0,
        &mut state,
        &query,
        &needle_lower,
        case_sensitive,
        max_hits.max(1).min(500),
        &mut hits,
    );
    Ok(hits)
}

// ---------- 命令：打开与定位 ----------

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let target = if p.is_dir() {
        path.clone()
    } else {
        match p.parent() {
            Some(_) => {
                // 用 explorer /select, 选中文件
                let _ = std::process::Command::new("explorer")
                    .arg(format!("/select,{}", path))
                    .spawn();
                return Ok(());
            }
            None => path.clone(),
        }
    };
    std::process::Command::new("explorer")
        .arg(target)
        .spawn()
        .map_err(to_string_err)?;
    Ok(())
}

#[tauri::command]
fn open_with_default_app(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // 这个路径来自 Markdown 正文里的链接，属于不可信输入。
        // cmd /C start 会把引号外的 & | ^ 当命令分隔符，不处理就等于任意命令执行。
        if path.contains('"') || path.contains('\r') || path.contains('\n') {
            return Err("链接包含非法字符".into());
        }
        let lower = path.to_lowercase();
        let is_url = lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:");
        if !is_url {
            if lower.contains("://") {
                return Err("不支持的外部链接协议".into());
            }
            if !Path::new(&path).exists() {
                return Err(format!("文件不存在：{}", path));
            }
        }
        // 整体加引号：cmd 只在引号外识别命令分隔符
        let arg = format!("\"{}\"", path);
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &arg])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(to_string_err)?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(to_string_err)?;
    }
    Ok(())
}

#[tauri::command]
fn get_startup_file(state: tauri::State<AppState>) -> Option<String> {
    state.startup_file.lock().unwrap().clone()
}

#[tauri::command]
fn get_pdf_target(state: tauri::State<AppState>) -> Option<String> {
    state.pdf_out.lock().unwrap().clone()
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ---------- 命令：文档 → Markdown ----------

#[tauri::command]
fn convert_to_md(
    input: String,
    output: String,
    images: bool,
    front_matter: bool,
    overwrite: bool,
) -> Result<String, String> {
    let opts = convert::Options { images, front_matter };
    let (path, _) = convert::convert_and_write(
        Path::new(&input),
        Path::new(&output),
        &opts,
        overwrite,
    )?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn can_convert(path: String) -> bool {
    let ext = Path::new(&path)
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    convert::is_supported(&ext)
}

#[tauri::command]
fn convert_supported_exts() -> Vec<String> {
    convert::SUPPORTED_EXTS.iter().map(|s| s.to_string()).collect()
}

// ---------- 命令：窗口打印导出 PDF ----------

/// 走系统打印对话框（真实打印机 / 用户自选 PDF）。导出的成品 PDF 请用 `export_pdf`。
#[tauri::command]
fn print_current_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(to_string_err)
}

/// 直接生成 PDF 文件：无打印对话框、无浏览器页眉页脚。
#[tauri::command]
async fn export_pdf(
    window: tauri::WebviewWindow,
    path: String,
    opts: pdfexport::PdfOptions,
) -> Result<String, String> {
    pdfexport::export(&window, path.clone(), opts).await?;
    Ok(path)
}


#[tauri::command]
fn set_window_title(window: tauri::WebviewWindow, title: String) -> Result<(), String> {
    window.set_title(&title).map_err(to_string_err)
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------- 命令：把 .md / .markdown 关联到本程序 ----------

/// 注册表写入项：键路径 / 值名（None = 默认值）/ 数据
struct RegEntry {
    key: String,
    name: Option<String>,
    data: String,
}

/// 优先用 reg.exe；被环境拦掉时退回 PowerShell（两者都隐藏窗口）
fn write_reg(entries: &[RegEntry]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut last_err = String::new();
        for e in entries {
            let mut args: Vec<String> = vec![
                "add".into(),
                e.key.clone(),
                "/f".into(),
                "/d".into(),
                e.data.clone(),
            ];
            match &e.name {
                Some(n) => {
                    args.push("/v".into());
                    args.push(n.clone());
                    args.push("/t".into());
                    args.push("REG_SZ".into());
                }
                None => args.push("/ve".into()),
            }
            let out = std::process::Command::new("reg")
                .args(&args)
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            match out {
                Ok(o) if o.status.success() => {}
                Ok(o) => {
                    last_err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    break;
                }
                Err(err) => {
                    last_err = err.to_string();
                    break;
                }
            }
        }
        if last_err.is_empty() {
            return Ok(());
        }

        // 兜底：PowerShell。注意默认值必须用 .NET 的 SetValue("") 写，
        // Set-ItemProperty -Name '(default)' 会在多数版本上新建一个名为 "(default)" 的字面值，
        // 默认值仍是空的，文件关联等于没写。
        let mut ps = String::from("$ErrorActionPreference='Stop';");
        for e in entries {
            let path = format!("HKEY_CURRENT_USER\\{}", e.key.trim_start_matches("HKCU\\"));
            let path = path.replace('\'', "''");
            ps.push_str(&format!(
                "$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('{}');",
                path
            ));
            let data = e.data.replace('\'', "''");
            match &e.name {
                Some(n) => {
                    let n = n.replace('\'', "''");
                    ps.push_str(&format!(
                        "$k.SetValue('{}','{}',[Microsoft.Win32.RegistryValueKind]::String);",
                        n, data
                    ));
                }
                None => ps.push_str(&format!(
                    "$k.SetValue('','{}',[Microsoft.Win32.RegistryValueKind]::String);",
                    data
                )),
            }
        }
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(to_string_err)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "reg.exe: {}; powershell: {}",
                last_err,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = entries;
        Err("仅 Windows 支持".into())
    }
}

/// 在当前用户下注册：Applications\MDeditor.exe + ProgID + OpenWithProgids。
/// 不需要管理员权限，注册后 MDeditor 会出现在「打开方式」/「其他应用」列表里。
#[tauri::command]
fn register_md_assoc() -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅 Windows 支持".into());
    }
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(to_string_err)?;
        let exe = exe.to_string_lossy().into_owned();
        let cmd = format!("\"{}\" \"%1\"", exe);
        let icon = format!("\"{}\",0", exe);
        let app_key = r"HKCU\Software\Classes\Applications\MDeditor.exe".to_string();

        let mut entries = vec![
            // 「打开方式」候选程序
            RegEntry { key: format!(r"{}\shell\open\command", app_key), name: None, data: cmd.clone() },
            RegEntry { key: format!(r"{}\DefaultIcon", app_key), name: None, data: icon.clone() },
            RegEntry { key: format!(r"{}\SupportedTypes", app_key), name: Some(".md".into()), data: String::new() },
            RegEntry { key: format!(r"{}\SupportedTypes", app_key), name: Some(".markdown".into()), data: String::new() },
            // Windows 11「默认应用」按应用页面所需的正式注册
            RegEntry {
                key: r"HKCU\Software\RegisteredApplications".into(),
                name: Some("MDeditor".into()),
                data: r"Software\Classes\Applications\MDeditor.exe\Capabilities".into(),
            },
            RegEntry {
                key: format!(r"{}\Capabilities", app_key),
                name: Some("ApplicationName".into()),
                data: "MDeditor".into(),
            },
            RegEntry {
                key: format!(r"{}\Capabilities", app_key),
                name: Some("ApplicationDescription".into()),
                data: "本地 Markdown 阅读与排版编辑器".into(),
            },
            RegEntry {
                key: format!(r"{}\Capabilities\FileAssociations", app_key),
                name: Some(".md".into()),
                data: "MDeditor.md".into(),
            },
            RegEntry {
                key: format!(r"{}\Capabilities\FileAssociations", app_key),
                name: Some(".markdown".into()),
                data: "MDeditor.markdown".into(),
            },
        ];
        for ext in [".md", ".markdown"] {
            let prog = format!("MDeditor{}", ext);
            entries.push(RegEntry { key: format!(r"HKCU\Software\Classes\{}", prog), name: None, data: "Markdown 文档".into() });
            entries.push(RegEntry { key: format!(r"HKCU\Software\Classes\{}\shell\open\command", prog), name: None, data: cmd.clone() });
            entries.push(RegEntry { key: format!(r"HKCU\Software\Classes\{}\DefaultIcon", prog), name: None, data: icon.clone() });
            entries.push(RegEntry { key: format!(r"HKCU\Software\Classes\{}\OpenWithProgids", ext), name: Some(prog), data: String::new() });
        }
        write_reg(&entries)?;
        Ok(exe)
    }
}

/// 关联状态：已注册返回 true
#[tauri::command]
fn md_assoc_status() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
    #[cfg(target_os = "windows")]
    {
        // 三条路径任一命中即视为已关联：
        // ① 设置页按钮写入的 Applications 键 ② 设置页写入的 ProgID
        // ③ 安装包 APP_ASSOCIATE 写入的 ProgID（中文名，installer.nsi 里就是它）
        let keys = [
            r"HKCU\Software\Classes\Applications\MDeditor.exe\shell\open\command",
            r"HKCU\Software\Classes\MDeditor.md\shell\open\command",
            r"HKCU\Software\Classes\Markdown 文档\shell\open\command",
        ];
        for key in keys {
            if let Ok(o) = std::process::Command::new("reg").args(["query", key]).output() {
                if o.status.success() {
                    return true;
                }
            }
        }
        // reg.exe 被策略拦掉时再试 PowerShell（只查一条，够用）
        let ps = format!(
            "Test-Path 'Microsoft.PowerShell.Core\\Registry::HKEY_CURRENT_USER\\Software\\Classes\\Applications\\MDeditor.exe\\shell\\open\\command'"
        );
        if let Ok(o) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output()
        {
            return String::from_utf8_lossy(&o.stdout).trim() == "True";
        }
        false
    }
}

/// 打开 Windows 的「默认应用」设置页，让用户把 .md 的默认程序切成本程序
/// （Win10 之后不允许程序直接改写默认程序，必须用户确认）
#[tauri::command]
fn open_default_apps_settings() -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅 Windows 支持".into());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // registeredAppUser 与 HKCU\Software\RegisteredApplications 下的值名一致。
        // Windows 11 会直接打开 MDeditor 自己的默认关联页；旧版 Windows 会回退到默认应用首页。
        std::process::Command::new("explorer.exe")
            .arg("ms-settings:defaultapps?registeredAppUser=MDeditor")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(to_string_err)?;
        Ok(())
    }
}

fn main() {
    // 命令行转换模式：mdeditor.exe --to-md <文件> [选项]
    if let Some(code) = cli::maybe_run_cli() {
        std::process::exit(code);
    }

    // 启动参数里可能带有 .md 文件路径（文件关联双击打开）
    // 用 args_os：路径含非法 Unicode 时 args() 会 panic，release 下直接静默退出
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let startup_file = args
        .iter()
        .find(|a| !a.starts_with('-') && Path::new(a).is_file())
        .cloned();

    // --pdf-out <路径> 或 --pdf-out=<路径>：无界面交互地把文档导出成 PDF
    let pdf_out = args
        .iter()
        .position(|a| a == "--pdf-out")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--pdf-out=").map(|s| s.to_string()))
        });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            startup_file: Mutex::new(startup_file),
            pdf_out: Mutex::new(pdf_out),
        })
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                if let Some(w) = _app.get_webview_window("main") {
                    let _ = w.open_devtools();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            write_text_file,
            read_image_data_url,
            write_base64_file,
            list_dir,
            path_stat,
            path_exists,
            create_dir,
            remove_path,
            rename_path,
            home_dir,
            sep,
            find_markdown_files,
            search_in_files,
            reveal_in_explorer,
            open_with_default_app,
            get_startup_file,
            get_pdf_target,
            quit_app,
            convert_to_md,
            can_convert,
            convert_supported_exts,
            print_current_window,
            export_pdf,
            set_window_title,
            get_app_version,
            register_md_assoc,
            md_assoc_status,
            open_default_apps_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MDeditor");
}
