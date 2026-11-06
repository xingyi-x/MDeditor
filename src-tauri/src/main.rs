// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod convert;
mod pdfexport;

use base64::Engine as _;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

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

/// 关闭流程的裁决权放在 Rust 侧。
///
/// 教训：前端一旦注册 `window.onCloseRequested`，Tauri 会把原生关闭拦下来，改由前端调用
/// `window.destroy()` 才真正关窗；而 `capabilities/default.json` 默认不授予
/// `core:window:allow-destroy`（`core:window:default` 的权限清单里没有它），destroy 被 ACL
/// 拒绝后，窗口就再也关不掉 —— 点 ✕ 无反应、Alt+F4 无效，只能结束进程。
///
/// 现在改为：Rust 拦截 `CloseRequested` 并广播事件，前端只负责决定「直接退 / 先询问」，
/// 最终统一走 `quit_app`（`app.exit`，不受 ACL 限制）。
#[derive(Default)]
struct CloseGuard {
    /// 已确认可以关闭（前端答复完毕或强放行）
    allowed: AtomicBool,
    /// 上一次关闭请求时刻：1.5 秒内再次请求直接放行，保证任何情况下都能退出
    last_req: Mutex<Option<std::time::Instant>>,
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

/// Tauri 2 里未标 async 的命令运行在主线程。PDF 转换、全文搜索、大图 base64、
/// 注册表子进程都可能耗时数秒，主线程被占住时整个窗口「未响应」，连提示都刷不出来。
/// 统一把这类同步工作挪到阻塞线程池。
async fn on_worker<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String> + Send + 'static,
    R: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("后台任务异常终止：{}", e))?
}

/// 写盘命令的路径守卫。不限制用户正常编辑的目录（否则会破坏另存为 / 粘贴图片），
/// 只挡住 XSS 最典型的落点：把可执行文件写进「启动」文件夹。
fn guarded_write_path(path: &str, check_ext: bool) -> Result<(), String> {
    let norm = path.to_lowercase().replace('/', "\\");
    if norm.contains("\\startup\\") || norm.contains("\\programs\\startup") {
        return Err("不允许写入系统启动目录".into());
    }
    if check_ext {
        const BLOCKED: &[&str] = &[
            "exe", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr", "dll",
            "msi", "com", "pif", "hta", "reg", "lnk", "jar",
        ];
        let ext = Path::new(path)
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if BLOCKED.contains(&ext.as_str()) {
            return Err(format!("不允许写入可执行文件类型：.{}", ext));
        }
    }
    Ok(())
}

fn detect_eol(bytes: &[u8]) -> String {
    let crlf = bytes.windows(2).filter(|w| *w == b"\r\n").count();
    let lf = bytes.iter().filter(|&&b| b == b'\n').count().saturating_sub(crlf);
    let cr = bytes.iter().filter(|&&b| b == b'\r').count().saturating_sub(crlf);
    if crlf >= lf && crlf >= cr {
        "\r\n"
    } else if cr > lf {
        "\r"
    } else {
        "\n"
    }
    .to_string()
}

/// 读取时一并返回原编码、BOM 与换行风格，保存时按原样还原，
/// 避免「打开 GBK/UTF-16 文件 → 静默改写成无 BOM UTF-8 + LF」。
#[derive(Serialize, Clone)]
struct TextFileInfo {
    text: String,
    encoding: String,
    bom: bool,
    eol: String,
}

fn decode_bytes_meta(bytes: &[u8]) -> (String, String, bool, String) {
    let eol = detect_eol(bytes);
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
            "utf-8".into(),
            true,
            eol,
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        return (cow.into_owned(), "utf-16le".into(), true, eol);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        return (cow.into_owned(), "utf-16be".into(), true, eol);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), "utf-8".into(), false, eol),
        Err(_) => {
            // 只在坏字节占比很高时才回退 GBK：一个偶然的坏字节不该让整篇 UTF-8
            // 中文被当成 GBK 解成乱码，再被自动保存永久写回磁盘。
            let mut bad = 0usize;
            let mut i = 0usize;
            while i < bytes.len() {
                match std::str::from_utf8(&bytes[i..]) {
                    Ok(_) => break,
                    Err(e) => {
                        i += e.valid_up_to();
                        let skip = e.error_len().unwrap_or(1);
                        bad += skip;
                        i += skip;
                    }
                }
            }
            if bad * 20 <= bytes.len() {
                (String::from_utf8_lossy(bytes).into_owned(), "utf-8".into(), false, eol)
            } else {
                let (cow, _, _) = encoding_rs::GBK.decode(bytes);
                (cow.into_owned(), "gbk".into(), false, eol)
            }
        }
    }
}

fn decode_bytes(bytes: &[u8]) -> String {
    decode_bytes_meta(bytes).0
}

fn encode_text(content: &str, enc: &str, bom: bool, eol: &str) -> Result<Vec<u8>, String> {
    let normalized = content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', eol);
    let mut out: Vec<u8> = match enc {
        "utf-16le" => {
            let (cow, _, bad) = encoding_rs::UTF_16LE.encode(&normalized);
            if bad {
                return Err("当前内容包含 UTF-16LE 无法表示的字符".into());
            }
            cow.into_owned()
        }
        "utf-16be" => {
            let (cow, _, bad) = encoding_rs::UTF_16BE.encode(&normalized);
            if bad {
                return Err("当前内容包含 UTF-16BE 无法表示的字符".into());
            }
            cow.into_owned()
        }
        "gbk" => {
            let (cow, _, bad) = encoding_rs::GBK.encode(&normalized);
            if bad {
                return Err("当前内容包含 GBK 无法表示的字符，请另存为 UTF-8".into());
            }
            cow.into_owned()
        }
        _ => normalized.into_bytes(),
    };
    if bom {
        let prefix: &[u8] = match enc {
            "utf-16le" => &[0xFF, 0xFE],
            "utf-16be" => &[0xFE, 0xFF],
            _ => &[0xEF, 0xBB, 0xBF],
        };
        let mut v = prefix.to_vec();
        v.extend_from_slice(&out);
        out = v;
    }
    Ok(out)
}

/// 原子写入：先写同目录的 .mdeditor-tmp，再 rename 覆盖。
/// 崩溃或断电时不会留下半截文档；rename 失败时退回普通写入。
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut os = path.as_os_str().to_os_string();
    os.push(".mdeditor-tmp");
    let tmp = PathBuf::from(os);
    fs::write(&tmp, data)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ---------- 命令：文件读写 ----------

#[tauri::command]
async fn read_text_file(path: String) -> Result<TextFileInfo, String> {
    on_worker(move || {
        let bytes = fs::read(&path).map_err(to_string_err)?;
        let (text, encoding, bom, eol) = decode_bytes_meta(&bytes);
        Ok(TextFileInfo { text, encoding, bom, eol })
    })
    .await
}

#[tauri::command]
async fn write_text_file(
    path: String,
    content: String,
    encoding: Option<String>,
    bom: Option<bool>,
    eol: Option<String>,
) -> Result<(), String> {
    on_worker(move || {
        guarded_write_path(&path, false)?;
        let enc = encoding.unwrap_or_else(|| "utf-8".into());
        let has_bom = bom.unwrap_or(false);
        let line_end = eol.filter(|s| s == "\n" || s == "\r\n" || s == "\r")
            .unwrap_or_else(|| "\n".into());
        let data = encode_text(&content, &enc, has_bom, &line_end)?;
        atomic_write(Path::new(&path), &data).map_err(to_string_err)
    })
    .await
}

#[tauri::command]
async fn read_image_data_url(path: String) -> Result<String, String> {
    on_worker(move || {
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
    })
    .await
}

#[tauri::command]
async fn write_base64_file(path: String, data: String) -> Result<(), String> {
    on_worker(move || {
        guarded_write_path(&path, true)?;
        // 兼容 data:image/png;base64,xxxx 形式
        let payload = match data.split_once(",") {
            Some((head, rest)) if head.contains("base64") => rest.to_string(),
            _ => data,
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(to_string_err)?;
        atomic_write(Path::new(&path), &bytes).map_err(to_string_err)
    })
    .await
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

/// 外部修改检测用：返回文件最后修改时间（Unix 毫秒）；文件不存在返回 Ok(None)。
#[tauri::command]
async fn file_mtime(path: String) -> Result<Option<u64>, String> {
    on_worker(move || {
        match fs::metadata(&path) {
            Ok(m) => Ok(m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
}

#[tauri::command]
fn create_dir(path: String) -> Result<(), String> {
    fs::create_dir_all(&path).map_err(to_string_err)
}

#[tauri::command]
fn remove_path(path: String) -> Result<(), String> {
    guarded_write_path(&path, false)?;
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("路径不存在：{}", path));
    }
    // 送进系统回收站而不是永久删除。失败时明确报错，绝不回退成 fs::remove_*，
    // 否则用户会在「以为可还原」的前提下被永久删除。
    trash::delete(p).map_err(|e| format!("移入回收站失败（文件未被删除）：{}", e))
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
async fn find_markdown_files(root: String, max_depth: u32) -> Result<Vec<EntryInfo>, String> {
    on_worker(move || find_markdown_files_sync(root, max_depth)).await
}

fn find_markdown_files_sync(root: String, max_depth: u32) -> Result<Vec<EntryInfo>, String> {
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
async fn search_in_files(
    root: String,
    query: String,
    case_sensitive: bool,
    max_hits: u32,
) -> Result<Vec<SearchHit>, String> {
    on_worker(move || search_in_files_sync(root, query, case_sensitive, max_hits)).await
}

fn search_in_files_sync(
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
                    // column 给前端展示用，必须按**字符数**算；str::find 返回的是字节偏移，
                    // 中文行会偏大。to_lowercase 可能改变字节长度，切片要防越界。
                    let byte_col = if case_sensitive {
                        line.find(query).unwrap_or(0)
                    } else {
                        line.to_lowercase().find(needle_lower).unwrap_or(0)
                    };
                    let col = line
                        .get(..byte_col)
                        .map(|s| s.chars().count() as u64)
                        .unwrap_or(0);
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
async fn open_with_default_app(path: String) -> Result<(), String> {
    on_worker(move || open_with_default_app_sync(path)).await
}

fn open_with_default_app_sync(path: String) -> Result<(), String> {
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
        // cmd 会在引号内展开 %VAR%（启用延迟展开时还有 !VAR!），
        // 含这些字符的路径改走 explorer，完全不经过命令行解释器。
        if path.contains('%') || path.contains('!') {
            std::process::Command::new("explorer.exe")
                .arg(&path)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(to_string_err)?;
            return Ok(());
        }
        // 旧实现先手工加引号再交给 .args()：Rust 会把参数里的 " 转义成 \"，
        // cmd.exe 不认识 \"，含空格路径的引号配对会错位。
        // raw_arg 把整段尾部命令原样追加，引号由我们自己控制。
        let tail = format!("/C start \"\" \"{}\"", path);
        std::process::Command::new("cmd")
            .raw_arg(tail)
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
fn quit_app(app: tauri::AppHandle, code: Option<i32>) {
    // 标记为「已允许关闭」再退出：即使 app.exit 之后还有一次 CloseRequested，
    // 也不会被拦截成第二个提示。--pdf-out 自动化导出用 code 回报成功/失败。
    if let Some(guard) = app.try_state::<CloseGuard>() {
        guard.allowed.store(true, Ordering::SeqCst);
    }
    app.exit(code.unwrap_or(0));
}

// ---------- 命令：文档 → Markdown ----------

/// 转换结果：输出路径 + 非致命提示（为空表示没有需要提醒的问题）
#[derive(Serialize, Clone)]
struct ConvertResult {
    path: String,
    warnings: Vec<String>,
}

#[tauri::command]
async fn convert_to_md(
    input: String,
    output: String,
    images: bool,
    front_matter: bool,
    overwrite: bool,
) -> Result<ConvertResult, String> {
    on_worker(move || {
        let opts = convert::Options { images, front_matter };
        let (path, conv) = convert::convert_and_write(
            Path::new(&input),
            Path::new(&output),
            &opts,
            overwrite,
        )?;
        Ok(ConvertResult {
            path: path.to_string_lossy().into_owned(),
            warnings: conv.warnings,
        })
    })
    .await
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
            // CurrentUser.CreateSubKey 接收的是**相对 HKCU 的子键路径**，
            // 再拼上 "HKEY_CURRENT_USER\" 会真的创建 HKCU\HKEY_CURRENT_USER\…，
            // 兜底分支看似成功、关联实际没写进去。
            let path = e.key
                .trim_start_matches("HKCU\\")
                .trim_start_matches("HKEY_CURRENT_USER\\")
                .replace('\'', "''");
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
async fn register_md_assoc() -> Result<String, String> {
    on_worker(register_md_assoc_sync).await
}

fn register_md_assoc_sync() -> Result<String, String> {
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
async fn md_assoc_status() -> bool {
    on_worker(|| Ok(md_assoc_status_sync())).await.unwrap_or(false)
}

fn md_assoc_status_sync() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // 三条路径任一命中即视为已关联：
        // ① 设置页按钮写入的 Applications 键 ② 设置页写入的 ProgID
        // ③ 安装包 APP_ASSOCIATE 写入的 ProgID（中文名，installer.nsi 里就是它）
        let keys = [
            r"HKCU\Software\Classes\Applications\MDeditor.exe\shell\open\command",
            r"HKCU\Software\Classes\MDeditor.md\shell\open\command",
            r"HKCU\Software\Classes\Markdown 文档\shell\open\command",
        ];
        for key in keys {
            // 不隐藏窗口的话，每次打开「设置」都会闪一个黑色控制台
            if let Ok(o) = std::process::Command::new("reg")
                .args(["query", key])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                if o.status.success() {
                    return true;
                }
            }
        }
        // reg.exe 被策略拦掉时再试 PowerShell（只查一条，够用）
        let ps = "Test-Path 'Microsoft.PowerShell.Core\\Registry::HKEY_CURRENT_USER\\Software\\Classes\\Applications\\MDeditor.exe\\shell\\open\\command'";
        if let Ok(o) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            return String::from_utf8_lossy(&o.stdout).trim() == "True";
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
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

/// 单实例：Windows 命名互斥体。句柄随进程一直持有，第二个实例拿到
/// ERROR_ALREADY_EXISTS 后提示用户并直接退出，避免两个窗口同时写同一文档。
/// CLI 转换在 main() 开头就已经 exit，不受影响；--pdf-out 自动化导出显式跳过。
#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    let name: Vec<u16> = "Local\\MDeditor.SingleInstance\0".encode_utf16().collect();
    let title: Vec<u16> = "MDeditor\0".encode_utf16().collect();
    let text: Vec<u16> = "MDeditor 已在运行，请切换到已有窗口继续编辑。\0".encode_utf16().collect();
    unsafe {
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                // CreateMutexW 成功但对象已存在时，GetLastError 仍是 ERROR_ALREADY_EXISTS
                let exists = GetLastError() == ERROR_ALREADY_EXISTS;
                // HANDLE 没有 Drop 实现，句柄会一直保持到进程退出；此处不做任何关闭动作
                let _ = handle;
                if !exists {
                    return true;
                }
                let _ = MessageBoxW(
                    None,
                    PCWSTR(text.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    MB_OK | MB_ICONINFORMATION,
                );
                false
            }
            // 互斥体创建失败时放行，避免把用户彻底锁在应用外面
            Err(_) => true,
        }
    }
}

#[cfg(not(windows))]
fn acquire_single_instance() -> bool {
    true
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
    // --pdf-out <路径> 或 --pdf-out=<路径>：无界面交互地把文档导出成 PDF
    let pdf_value = args
        .iter()
        .position(|a| a == "--pdf-out")
        .and_then(|i| args.get(i + 1).cloned());
    let pdf_out = pdf_value.clone().or_else(|| {
        args.iter()
            .find_map(|a| a.strip_prefix("--pdf-out=").map(|s| s.to_string()))
    });

    // `mdeditor.exe --pdf-out out.pdf in.md` 里的 out.pdf 也是个存在的路径，
    // 但它是**输出**而不是要打开的文档，必须排除，否则启动后先打开一个 PDF。
    let startup_file = args
        .iter()
        .find(|a| {
            !a.starts_with('-')
                && pdf_value.as_deref() != Some(a.as_str())
                && Path::new(a).is_file()
        })
        .cloned();

    // --pdf-out 是无界面自动化导出，必须允许与常驻 GUI 实例并存
    let automation = args
        .iter()
        .any(|a| a == "--pdf-out" || a.starts_with("--pdf-out="));
    if !automation && !acquire_single_instance() {
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            startup_file: Mutex::new(startup_file),
            pdf_out: Mutex::new(pdf_out),
        })
        .manage(CloseGuard::default())
        // 关闭裁决：默认拦下并交给前端询问；前端答完调用 quit_app 才真正退出。
        // 1.5 秒内重复请求关闭则直接放行，避免前端异常时把用户锁死在应用里。
        .on_window_event(|window, event| {
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let guard = window.state::<CloseGuard>();
            if guard.allowed.load(Ordering::SeqCst) {
                return;
            }
            let now = std::time::Instant::now();
            let repeated = match guard.last_req.lock() {
                Ok(mut last) => {
                    let r = last
                        .map(|t| now.duration_since(t).as_millis() < 1500)
                        .unwrap_or(false);
                    *last = Some(now);
                    r
                }
                // 互斥锁中毒：宁可放行也不要卡住关闭
                Err(_) => true,
            };
            if repeated {
                guard.allowed.store(true, Ordering::SeqCst);
                return;
            }
            api.prevent_close();
            let _ = window.emit("app:close-requested", ());
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
            file_mtime,
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
