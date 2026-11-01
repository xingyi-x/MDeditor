/* ══════════════ convert：把 PDF / Word / Excel / PPT / HTML 等转换成 Markdown ══════════════
 *
 * 设计目标：输出「适合人和 AI 阅读」的干净 Markdown —— 保留标题层级、表格、列表、
 * 图片引用；丢掉页眉页脚、字体样式噪音。
 */
pub mod docx;
pub mod html;
pub mod pdf;
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

/// 统一整理 Markdown：压缩空行、规范标题/列表/表格空行、去掉行尾空白
pub fn tidy(md: &str) -> String {
    let src = md.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = src.split('\n').map(|l| l.to_string()).collect();

    // 去掉行尾空白（保留用于软换行的两个空格）
    for l in lines.iter_mut() {
        let trimmed = l.trim_end();
        if l.len() - trimmed.len() == 2 && !trimmed.is_empty() {
            *l = format!("{}  ", trimmed);
        } else {
            *l = trimmed.to_string();
        }
    }

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
            || t.starts_with("|")
            || t.starts_with("```")
            || t.starts_with("---")
            || t.starts_with("***")
            || t.starts_with("> ")
            || t == ">"
            || t.starts_with("![")
    }
    /// 前后两行属于同一个块（表格 / 列表 / 引用 / 代码块）→ 中间不加空行
    fn same_block(a: &str, b: &str, in_fence: bool) -> bool {
        if in_fence {
            return true;
        }
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

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_fence = false;
    for line in lines.into_iter() {
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
            && !same_block(&prev, &line, in_fence)
        {
            out.push(String::new());
        }
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
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

/// 转义表格单元格
pub fn cell_text(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', "<br>").trim().to_string()
}

/* ────────── .doc（旧版二进制）→ 用 Word/WPS COM 转 docx ────────── */

pub fn doc_to_docx(src: &Path, dst: &Path) -> Result<(), String> {
    let ps = format!(
        "$ErrorActionPreference='Stop'\n\
         $src = '{}'\n\
         $dst = '{}'\n\
         $app = $null\n\
         foreach ($prog in @('Word.Application','KWPS.Application','WPS.Application')) {{\n\
         \x20 try {{ $app = New-Object -ComObject $prog; break }} catch {{ }}\n\
         }}\n\
         if (-not $app) {{ exit 3 }}\n\
         $app.Visible = $false\n\
         $app.DisplayAlerts = 0\n\
         $doc = $app.Documents.Open($src, $false, $true)\n\
         $doc.SaveAs([ref]$dst, [ref]16)\n\
         $doc.Close([ref]0)\n\
         $app.Quit()\n\
         exit 0\n",
        src.to_string_lossy().replace('\'', "''"),
        dst.to_string_lossy().replace('\'', "''"),
    );
    let script = std::env::temp_dir().join(format!("mdeditor_doc2docx_{}.ps1", now_id()));
    fs::write(&script, ps).map_err(|e| format!("无法写临时脚本：{}", e))?;

    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.to_string_lossy(),
        ])
        .creation_flags_no_window()
        .status();
    let _ = fs::remove_file(&script);

    match status {
        Ok(st) if st.success() && dst.exists() => Ok(()),
        Ok(_) => Err("无法转换 .doc（旧版二进制格式）：本机未安装 Word/WPS，请用 Word 另存为 .docx 后重试".into()),
        Err(e) => Err(format!("调用 PowerShell 失败：{}", e)),
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
        "md" | "markdown" => plain::text(data),
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
    if opts.front_matter {
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
    // 不引入 chrono：UTC+8 本地时间的简易计算
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem / 60) % 60
    )
}

/// 天数 → 年月日（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
