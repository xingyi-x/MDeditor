/* ══════════════ convert：把 PDF / Word / Excel / PPT / HTML 等转换成 Markdown ══════════════
 *
 * 设计目标：输出「适合人和 AI 阅读」的干净 Markdown —— 保留标题层级、表格、列表、
 * 图片引用；丢掉页眉页脚、字体样式噪音。
 */
pub mod docx;
pub mod html;
pub mod pdf;
mod pdf_image;
pub mod plain;
pub mod pptx;
pub mod rtf;
pub mod xlsx;

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

pub type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

/// 支持转换的扩展名（小写）
pub const SUPPORTED_EXTS: &[&str] = &[
    "pdf", "docx", "doc", "xlsx", "xlsm", "pptx", "html", "htm", "txt", "csv", "tsv", "rtf", "md",
    "markdown",
];

pub fn is_supported(ext: &str) -> bool {
    SUPPORTED_EXTS.contains(&ext)
}

/// 是否为「原样透传」的输入：.md / .markdown 不整理内容、不额外加 front matter
pub fn is_passthrough_ext(ext: &str) -> bool {
    matches!(ext, "md" | "markdown")
}

pub fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/* ────────── 选项与上下文 ────────── */

#[derive(Clone, Debug)]
pub struct Options {
    /// 是否提取图片到 <名称>_assets/
    pub images: bool,
    /// 是否在开头写 YAML front matter
    pub front_matter: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { images: true, front_matter: false }
    }
}

pub struct Ctx {
    pub opts: Options,
    pub assets_dir: PathBuf,
    pub assets_rel: String,
    /// zip 内路径（如 word/media/image1.png）→ md 中的相对引用
    pub media: HashMap<String, String>,
}

impl Ctx {
    pub fn new(out_md: &Path, opts: &Options) -> Ctx {
        let stem = out_md
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "doc".to_string());
        let dir = out_md
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Ctx {
            opts: opts.clone(),
            assets_dir: dir.join(format!("{}_assets", stem)),
            assets_rel: format!("{}_assets", stem),
            media: HashMap::new(),
        }
    }
    pub fn img(&self, zip_path: &str) -> Option<String> {
        self.media.get(zip_path).cloned()
    }
}

/* ────────── zip / XML 小工具 ────────── */

pub fn read_entry(ar: &mut Archive, name: &str) -> Option<String> {
    let mut s = String::new();
    ar.by_name(name).ok()?.read_to_string(&mut s).ok()?;
    Some(s)
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tif" | "tiff")
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    if cleaned.is_empty() { "image".to_string() } else { cleaned }
}

/// 把 zip 里 prefix 目录下的图片导出到 assets_dir，返回 (zip内路径 → 相对引用)
pub fn extract_media(
    ar: &mut Archive,
    prefix: &str,
    assets_dir: &Path,
    assets_rel: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let _ = fs::create_dir_all(assets_dir);
    for i in 0..ar.len() {
        let mut f = match ar.by_index(i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if !f.is_file() {
            continue;
        }
        let name = f.name().to_string();
        if !name.starts_with(prefix) {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        if !is_image_ext(&ext) {
            continue;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() || buf.is_empty() {
            continue;
        }
        let base = safe_name(name.rsplit('/').next().unwrap_or(&name));
        let out_path = assets_dir.join(&base);
        if fs::write(&out_path, &buf).is_ok() {
            map.insert(name, format!("{}/{}", assets_rel, base));
        }
    }
    map
}

/// 读取 XML 元素的属性（自动忽略命名空间前缀：w:val / val 都能取到）
pub fn xml_attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    let kb = key.as_bytes();
    for a in e.attributes().flatten() {
        let k = a.key.as_ref();
        let local = k.rsplit(|&c| c == b':').next().unwrap_or(k);
        if k == kb || local == kb {
            let v = String::from_utf8_lossy(&a.value).into_owned();
            return Some(
                quick_xml::escape::unescape(&v)
                    .map(|c| c.into_owned())
                    .unwrap_or(v),
            );
        }
    }
    None
}

/* ────────── 文本收尾整理 ────────── */

/// 统一整理 Markdown：压缩空行、规范标题/列表/表格空行、去掉行尾空白。
/// 代码块（``` 或 ~~~ 围栏，含缩进、长度 ≥3）内部及围栏行本身原样输出，不做任何整理。
pub fn tidy(md: &str) -> String {
    let src = md.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = src.split('\n').map(|l| l.to_string()).collect();

    fn is_list_line(s: &str) -> bool {
        let t = s.trim_start();
        t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") || {
            let b = t.as_bytes();
            let mut i = 0;
            while i < b.len() && b[i].is_ascii_digit() { i += 1; }
            i > 0 && i + 1 < t.len() && (t.as_bytes()[i] == b'.' || t.as_bytes()[i] == b')') && t.as_bytes()[i + 1] == b' '
        }
    }
    fn is_block_start(s: &str) -> bool {
        let t = s.trim_start();
        t.starts_with('#')
            || t.starts_with('|')
            || t.starts_with("```")
            || t.starts_with("~~~")
            || t.starts_with("---")
            || t.starts_with("***")
            || t.starts_with("> ")
            || t == ">"
            || t.starts_with("![")
    }
    /// 前后两行属于同一个块（表格 / 列表 / 引用）→ 中间不加空行
    fn same_block(a: &str, b: &str) -> bool {
        let (x, y) = (a.trim_start(), b.trim_start());
        if x.starts_with('|') && y.starts_with('|') {
            return true;
        }
        if is_list_line(x) && is_list_line(y) {
            return true;
        }
        // 引用块：`>`（框内空行）也是引用的一部分，不能在这里断开
        if x.starts_with('>') && y.starts_with('>') {
            return true;
        }
        false
    }
    /// 识别围栏行：允许缩进、``` 或 ~~~、长度 ≥3；返回 (字符, 长度, 是否纯围栏行)
    fn fence_marker(s: &str) -> Option<(u8, usize, bool)> {
        let t = s.trim_start();
        let b = t.as_bytes();
        let ch = *b.first()?;
        if ch != b'`' && ch != b'~' {
            return None;
        }
        let mut n = 0;
        while n < b.len() && b[n] == ch {
            n += 1;
        }
        if n < 3 {
            return None;
        }
        let pure = t[n..].chars().all(|c| c.is_whitespace());
        Some((ch, n, pure))
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut fence: Option<(u8, usize)> = None; // 当前围栏的（字符, 长度）
    for line in lines {
        // 围栏行 / 围栏内部：原样输出，不做任何整理
        if let Some((ch, n, pure)) = fence_marker(&line) {
            match fence {
                None => {
                    // 进入围栏：前面补一个空行（不改围栏行本身），随后原样输出
                    if out.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                        out.push(String::new());
                    }
                    fence = Some((ch, n));
                    out.push(line);
                    continue;
                }
                Some((fch, flen)) => {
                    // 关闭围栏需同字符、长度 ≥ 起始、且该行为纯围栏行
                    if ch == fch && n >= flen && pure {
                        fence = None;
                    }
                    out.push(line);
                    continue;
                }
            }
        }
        if fence.is_some() {
            out.push(line);
            continue;
        }

        // 以下整理仅作用于普通文本行
        let mut line = line;
        {
            let trimmed = line.trim_end();
            if line.len() - trimmed.len() == 2 && !trimmed.is_empty() {
                line = format!("{}  ", trimmed);
            } else {
                line = trimmed.to_string();
            }
        }
        // 折叠多余空行
        if line.trim().is_empty() {
            if out.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
                continue;
            }
            out.push(String::new());
            continue;
        }
        // 列表项之间不留空行
        if is_list_line(&line) {
            while out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
                if out.len() >= 2 && is_list_line(&out[out.len() - 2]) {
                    out.pop();
                } else {
                    break;
                }
            }
        }
        // 块级元素前补一个空行（同一块内部不补）
        let prev = out
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        if is_block_start(&line)
            && !prev.is_empty()
            && out.last().map(|l| !l.trim().is_empty()).unwrap_or(false)
            && !same_block(&prev, &line)
        {
            out.push(String::new());
        }
        out.push(line);
    }

    let mut s = out.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s.push('\n');
    s
}

/// 转义表格单元格（先 trim 再替换，避免末尾换行留下多余 <br>）
pub fn cell_text(s: &str) -> String {
    let t = s.trim();
    t.replace('|', "\\|").replace('\n', "<br>")
}

/* ────────── .doc（旧版二进制）→ 用 Word/WPS COM 转 docx ────────── */

pub fn doc_to_docx(src: &Path, dst: &Path) -> Result<(), String> {
    // 脚本里完全不拼接路径：src/dst 通过环境变量传入。
    // 这样既避免 PowerShell 单引号把 Unicode 智能引号（‘ ’ ‚ ‛）也当引号导致注入，
    // 也避免中文路径拼进脚本后被 PowerShell 5.1 按 GBK 解码而乱码。
    let ps = r#"$ErrorActionPreference='Stop'
$src = $env:MDEDITOR_SRC
$dst = $env:MDEDITOR_DST
$app = $null
$doc = $null
$code = 1
try {
    foreach ($prog in @('Word.Application','KWPS.Application','WPS.Application')) {
        try { $app = New-Object -ComObject $prog; break } catch { }
    }
    if (-not $app) {
        $code = 3
    } else {
        $app.Visible = $false
        $app.DisplayAlerts = 0
        $doc = $app.Documents.Open($src, $false, $true)
        $doc.SaveAs([ref]$dst, [ref]16)
        $code = 0
    }
} catch {
    try { [Console]::Error.WriteLine($_.Exception.Message) } catch { }
    $code = 1
} finally {
    if ($doc) { try { $doc.Close([ref]0) } catch { } }
    if ($app) { try { $app.Quit() } catch { } }
}
exit $code
"#;
    // PowerShell 5.1 的 -File 对无 BOM 脚本按系统 ANSI(GBK) 解码；加 UTF-8 BOM 保证按 UTF-8 读取
    let mut content = Vec::with_capacity(ps.len() + 3);
    content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    content.extend_from_slice(ps.as_bytes());
    let script = std::env::temp_dir().join(format!("mdeditor_doc2docx_{}.ps1", now_id()));
    fs::write(&script, &content).map_err(|e| format!("无法写临时脚本：{}", e))?;

    let script_path = script.to_string_lossy().into_owned();
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script_path.as_str(),
        ])
        .env("MDEDITOR_SRC", src.as_os_str())
        .env("MDEDITOR_DST", dst.as_os_str())
        .creation_flags_no_window()
        .output();
    let _ = fs::remove_file(&script);

    match output {
        Err(e) => Err(format!("调用 PowerShell 失败：{}", e)),
        Ok(o) => {
            let code = o.status.code();
            if o.status.success() && dst.exists() {
                Ok(())
            } else if code == Some(3) {
                // COM 创建失败 = 本机未安装 Word/WPS
                Err("无法转换 .doc：本机未安装 Word/WPS（COM 组件不可用），请用 Word 另存为 .docx 后重试".into())
            } else {
                // 脚本有 stderr / 非零退出 = 转换过程失败，不能笼统归为「未安装」
                let detail = String::from_utf8_lossy(&o.stderr);
                let detail = detail.trim();
                if detail.is_empty() {
                    Err(format!("无法转换 .doc：Word/WPS 转换失败（退出码 {:?}）", code))
                } else {
                    Err(format!("无法转换 .doc：Word/WPS 转换失败（退出码 {:?}）：{}", code, detail))
                }
            }
        }
    }
}

#[cfg(windows)]
trait CmdNoWindow {
    fn creation_flags_no_window(&mut self) -> &mut std::process::Command;
}
#[cfg(windows)]
impl CmdNoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut std::process::Command {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}
#[cfg(not(windows))]
trait CmdNoWindow {
    fn creation_flags_no_window(&mut self) -> &mut std::process::Command;
}
#[cfg(not(windows))]
impl CmdNoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut std::process::Command {
        self
    }
}

fn now_id() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/* ────────── 对外主入口 ────────── */

/// 把文件转换为 Markdown 字符串（不写盘；图片会写到 out_md 旁边的 _assets 目录）
pub fn convert_bytes(ext: &str, data: &[u8], out_md: &Path, opts: &Options) -> Result<String, String> {
    // .md / .markdown 原样透传：保留行尾空格（Markdown 硬换行）与所有换行，不经过 tidy
    if is_passthrough_ext(ext) {
        return Ok(plain::passthrough(data));
    }
    let md = match ext {
        "docx" => {
            let mut ctx = Ctx::new(out_md, opts);
            docx::convert(data, &mut ctx)?
        }
        "xlsx" | "xlsm" => xlsx::convert(data)?,
        "pptx" => pptx::convert(data)?,
        "pdf" => {
            let mut ctx = Ctx::new(out_md, opts);
            pdf::convert(data, &mut ctx)?
        }
        "html" | "htm" => html::convert(data)?,
        "rtf" => rtf::convert(data)?,
        "txt" | "text" | "log" => plain::text(data),
        "csv" | "tsv" => plain::delimited(data, ext == "tsv"),
        other => return Err(format!("不支持的文件类型：.{}", other)),
    };
    Ok(tidy(&md))
}

/// 转换文件 → Markdown 文本（.doc 走 COM）
pub fn convert_file_to_string(input: &Path, out_md: &Path, opts: &Options) -> Result<String, String> {
    let ext = ext_of(input);
    if !is_supported(&ext) {
        return Err(format!(
            "不支持的文件类型：.{}（支持：{}）",
            ext,
            SUPPORTED_EXTS.join(" / ")
        ));
    }
    if ext == "doc" {
        let tmp = std::env::temp_dir().join(format!("mdeditor_{}.docx", now_id()));
        doc_to_docx(input, &tmp)?;
        let data = fs::read(&tmp).map_err(|e| format!("读取转换结果失败：{}", e))?;
        let _ = fs::remove_file(&tmp);
        let mut ctx = Ctx::new(out_md, opts);
        let md = docx::convert(&data, &mut ctx)?;
        return Ok(tidy(&md));
    }
    let data = fs::read(input).map_err(|e| format!("读取失败：{}", e))?;
    convert_bytes(&ext, &data, out_md, opts)
}

/// 转换文件并写入 out_md；返回 (输出路径, Markdown 文本)
pub fn convert_and_write(
    input: &Path,
    out_md: &Path,
    opts: &Options,
    overwrite: bool,
) -> Result<(PathBuf, String), String> {
    if out_md.exists() && !overwrite {
        return Err(format!("目标已存在：{}（加 --force 覆盖）", out_md.display()));
    }
    let mut md = convert_file_to_string(input, out_md, opts)?;
    // front matter 只对「转换出来」的格式有意义；.md / .markdown 原样透传，不改变原文
    if opts.front_matter && !is_passthrough_ext(&ext_of(input)) {
        md = front_matter_of(input, &md);
    }
    if let Some(parent) = out_md.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(out_md, md.as_bytes()).map_err(|e| format!("写入失败：{}", e))?;
    Ok((out_md.to_path_buf(), md))
}

/// 在 Markdown 开头加 YAML 元信息
pub fn front_matter_of(input: &Path, md: &str) -> String {
    let title = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let now = chrono_like_now();
    // YAML 双引号字符串：反斜杠本身也要转义，否则 Windows 路径里的 \W 会被当成非法转义
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let src = input.to_string_lossy().replace('\\', "/");
    format!(
        "---\ntitle: \"{}\"\nsource: \"{}\"\nconverted: {}\n---\n\n{}",
        esc(&title),
        esc(&src),
        now,
        md
    )
}

fn chrono_like_now() -> String {
    // 用 chrono 读取本机时区，输出带真实 UTC 偏移的 ISO 8601。
    // 旧实现把偏移写死成 +08:00，跨时区使用或夏令时切换时会给出错误时间。
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}
