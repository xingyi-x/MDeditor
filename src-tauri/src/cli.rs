/* ══════════════ 命令行模式：文件 / 目录 → Markdown ══════════════
 *
 *   mdeditor.exe --to-md <文件或目录> [选项]
 *
 * 因为是 GUI 子系统的 exe（双击不弹黑框），命令行模式下需要主动附加到父控制台输出。
 */
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::convert;

const USAGE: &str = "\
MDeditor — 文档转 Markdown（本地离线，为 AI 阅读优化）

用法:
  mdeditor.exe --to-md <文件或目录> [选项]

选项:
  -o <文件>          输出 .md 路径（单文件模式，默认与源文件同名同目录）
  -d <目录>          输出到指定目录（批量模式默认与源文件同目录）
  -r                目录模式时递归子目录
  --force           覆盖已存在的 .md
  --stdout          只把结果打印到标准输出，不写文件（仅单文件）
  --no-images       不提取图片（PDF/Word 图片对 AI 通常无意义）
  --front-matter    在开头写入 YAML 元信息（标题/来源/时间）
  --quiet           静默：只输出生成的文件路径
  -h, --help        显示本帮助
  --version         显示版本

支持格式:
  pdf  docx  doc(需本机装 Word/WPS)  xlsx  xlsm  pptx  html  htm
  txt  csv  tsv  rtf  md  markdown

示例:
  mdeditor.exe --to-md 报告.pdf
  mdeditor.exe --to-md D:\\docs -r -d D:\\docs\\md
  mdeditor.exe --to-md 论文.docx --stdout > 论文.md
  mdeditor.exe --to-md 手册.pdf --no-images --front-matter
";

pub fn maybe_run_cli() -> Option<i32> {
    // args_os + 有损转换：命令行里出现非 UTF-8 的路径时 args() 会 panic
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let hit = args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--to-md" | "--convert" | "--conv" | "-c" | "--help" | "-h" | "--version"
        )
    });
    if !hit {
        return None;
    }
    #[cfg(windows)]
    {
        con::attach();
        con::set_utf8();
    }
    let code = run(args);
    #[cfg(windows)]
    {
        con::flush();
    }
    Some(code)
}

/// 错误 / 进度 / 提示一律写 stderr，避免混进 --stdout 重定向的输出文件
fn emit(s: &str) {
    #[cfg(windows)]
    {
        con::write_err(s);
        con::write_err("\n");
    }
    #[cfg(not(windows))]
    {
        eprintln!("{}", s);
    }
}

/// 正常数据输出写 stdout：仅 --stdout 的转换结果、帮助/版本、--quiet 的路径
fn emit_out(s: &str) {
    #[cfg(windows)]
    {
        con::write(s);
        con::write("\n");
    }
    #[cfg(not(windows))]
    {
        println!("{}", s);
    }
}

struct Opts {
    inputs: Vec<String>,
    out: Option<String>,
    out_dir: Option<String>,
    recursive: bool,
    force: bool,
    stdout: bool,
    images: bool,
    front_matter: bool,
    quiet: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            inputs: Vec::new(),
            out: None,
            out_dir: None,
            recursive: false,
            force: false,
            stdout: false,
            images: true,
            front_matter: false,
            quiet: false,
        }
    }
}

fn parse_args(args: Vec<String>) -> Opts {
    let mut o = Opts::default();
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--to-md" | "--convert" | "--conv" | "-c" => {
                // 后面的所有位置参数都是输入
                i += 1;
                while i < args.len() && !args[i].starts_with('-') {
                    o.inputs.push(args[i].clone());
                    i += 1;
                }
                continue;
            }
            "-o" | "--out" => {
                o.out = args.get(i + 1).cloned();
                i += 2;
                continue;
            }
            "-d" | "--out-dir" | "--dir" => {
                o.out_dir = args.get(i + 1).cloned();
                i += 2;
                continue;
            }
            "-r" | "--recursive" => o.recursive = true,
            "--force" | "-f" => o.force = true,
            "--stdout" => o.stdout = true,
            "--no-images" => o.images = false,
            "--front-matter" => o.front_matter = true,
            "--quiet" | "-q" => o.quiet = true,
            "-h" | "--help" => {
                emit_out(USAGE);
                std::process::exit(0);
            }
            "--version" => {
                emit_out(&format!("MDeditor {}", env!("CARGO_PKG_VERSION")));
                std::process::exit(0);
            }
            other => {
                if !other.starts_with('-') {
                    o.inputs.push(other.to_string());
                }
            }
        }
        i += 1;
    }
    o
}

fn run(args: Vec<String>) -> i32 {
    let o = parse_args(args);
    if o.inputs.is_empty() {
        emit(USAGE);
        return 1;
    }

    // --stdout 承诺"不写文件"，所以顺带关掉图片导出（图片必然要落盘）
    let opts = convert::Options {
        images: o.images && !o.stdout,
        front_matter: o.front_matter,
    };

    // 收集待转换文件
    let mut files: Vec<PathBuf> = Vec::new();
    for input in &o.inputs {
        let p = Path::new(input);
        if p.is_dir() {
            collect_files(p, o.recursive, &mut files);
        } else if p.is_file() {
            files.push(p.to_path_buf());
        } else {
            emit(&format!("[跳过] 找不到：{}", input));
        }
    }
    if files.is_empty() {
        emit("没有找到可转换的文件");
        return 1;
    }

    if o.stdout && files.len() > 1 {
        emit("--stdout 只支持单个文件，批量请去掉该选项");
        return 1;
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    // 本次运行已分配的输出路径：避免 -d 拍平目录时同名文件互相静默覆盖
    let mut used: HashSet<PathBuf> = HashSet::new();
    for f in &files {
        let out_path = target_path(f, &o, files.len() == 1, &mut used);
        // pdf-extract 遇到缺字会直接 println!：转换期间把标准输出截到临时文件，
        // 否则 `--stdout` 输出的 Markdown 前面会混进成百上千行诊断
        #[cfg(windows)]
        let capture = con::capture_stdout();
        let result = convert::convert_file(f, &out_path, &opts);
        #[cfg(windows)]
        let captured = capture.map(|c| c.finish()).unwrap_or_default();
        #[cfg(not(windows))]
        let captured = String::new();
        match result {
            Err(e) => {
                failed += 1;
                emit(&format!("[失败] {} — {}", f.display(), e));
            }
            Ok(mut conv) => {
                conv.warnings.extend(summarize_captured(&captured));
                // front matter 只对转换出来的格式有意义；原样透传的 md/markdown 不加
                if opts.front_matter && !convert::is_passthrough_ext(&convert::ext_of(f)) {
                    conv.md = convert::front_matter_of(f, &conv.md, &conv.meta);
                }
                // 提示一律走 stderr：转换成功但结果需要核对，不影响退出码
                if !o.quiet {
                    for w in &conv.warnings {
                        emit(&format!("[提示] {} — {}", f.display(), w));
                    }
                }
                let md = conv.md;
                if o.stdout {
                    emit_out(&md);
                    ok += 1;
                    continue;
                }
                if let Some(parent) = out_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                }
                match std::fs::write(&out_path, md.as_bytes()) {
                    Ok(_) => {
                        ok += 1;
                        if o.quiet {
                            emit_out(&out_path.to_string_lossy());
                        } else {
                            emit(&format!(
                                "[完成] {} → {}（{} 字符）",
                                f.display(),
                                out_path.display(),
                                md.chars().count()
                            ));
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        emit(&format!("[失败] 写入 {} 出错：{}", out_path.display(), e));
                    }
                }
            }
        }
    }

    if !o.quiet && !o.stdout {
        emit(&format!("\n共 {} 个文件，成功 {}，失败 {}", files.len(), ok, failed));
    }
    if failed > 0 {
        1
    } else {
        0
    }
}

/// 依赖库（pdf-extract）在转换时直接 println! 的诊断，聚合成几条提示。
/// 典型的是 `missing char 149 in unicode map {…}`：字体的 ToUnicode 表缺字，
/// 库会退回按字体编码解码（实测项目符号 • 仍能正确输出），逐条打印只会刷屏。
fn summarize_captured(text: &str) -> Vec<String> {
    let mut codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut other = 0usize;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("missing char ") {
            let code = rest.split_whitespace().next().unwrap_or("?").to_string();
            *codes.entry(code).or_insert(0) += 1;
        } else {
            other += 1;
        }
    }
    let mut out = Vec::new();
    if !codes.is_empty() {
        let total: usize = codes.values().sum();
        let detail = codes
            .iter()
            .take(8)
            .map(|(c, n)| format!("{} ×{}", c, n))
            .collect::<Vec<_>>()
            .join("，");
        out.push(format!(
            "PDF 字体的 Unicode 映射表缺字 {} 处（字符码 {}），已按字体编码回退，请核对项目符号等特殊字符",
            total, detail
        ));
    }
    if other > 0 {
        out.push(format!("PDF 解析库输出了 {} 行诊断信息，已从标准输出移除", other));
    }
    out
}

fn target_path(f: &Path, o: &Opts, single: bool, used: &mut HashSet<PathBuf>) -> PathBuf {
    // -o 只对单文件模式有意义：批量时若每个文件都往同一个路径写，会互相覆盖
    if single {
        if let Some(out) = &o.out {
            let p = PathBuf::from(out);
            used.insert(p.clone());
            return p;
        }
    }
    let name = format!(
        "{}.md",
        f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
    );
    let base: PathBuf = match &o.out_dir {
        Some(d) => PathBuf::from(d).join(name),
        None => f.with_extension("md"),
    };
    let mut chosen = base.clone();
    // 目标已存在或本次已分配过（拍平目录同名）：自动改名，后缀统一为 -1/-2…
    if (chosen.exists() || used.contains(&chosen)) && !o.force {
        let stem = base
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for i in 1..999 {
            let cand = base.with_file_name(format!("{}-{}.md", stem, i));
            if !cand.exists() && !used.contains(&cand) {
                chosen = cand;
                break;
            }
        }
    }
    used.insert(chosen.clone());
    chosen
}

fn collect_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            if recursive {
                let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if !name.starts_with('.') {
                    collect_files(&p, recursive, out);
                }
            }
            continue;
        }
        let ext = p
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "md" || ext == "markdown" {
            continue; // 不把 md 再转一遍
        }
        if convert::is_supported(&ext) {
            out.push(p);
        }
    }
}

/* ────────── Windows 控制台输出 ────────── */

#[cfg(windows)]
mod con {
    use std::ffi::c_void;

    const STD_OUTPUT_HANDLE: i32 = -11;
    const STD_ERROR_HANDLE: i32 = -12;

    unsafe extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
        fn GetStdHandle(nStdHandle: i32) -> *mut c_void;
        fn SetStdHandle(nStdHandle: i32, hHandle: *mut c_void) -> i32;
        fn WriteFile(
            hFile: *mut c_void,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: u32,
            lpNumberOfBytesWritten: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
        fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        fn CreateFileA(
            lpFileName: *const u8,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut c_void,
        ) -> *mut c_void;
    }

    const INVALID: *mut c_void = -1isize as *mut c_void;

    fn get(which: i32) -> *mut c_void {
        unsafe {
            let h = GetStdHandle(which);
            if h.is_null() || h == INVALID { std::ptr::null_mut() } else { h }
        }
    }

    /// 附加到父进程的控制台（GUI 子系统 exe 默认没有控制台）。
    /// 关键：cmd 做了 `> 文件` 重定向时，AttachConsole 会把标准句柄换成控制台句柄，
    /// 导致输出丢失 —— 所以先备份、attach 之后再还原。
    pub fn attach() {
        unsafe {
            let ho = get(STD_OUTPUT_HANDLE);
            let he = get(STD_ERROR_HANDLE);
            // ATTACH_PARENT_PROCESS = (DWORD)-1
            AttachConsole(0xFFFF_FFFF);
            if !ho.is_null() {
                SetStdHandle(STD_OUTPUT_HANDLE, ho);
            }
            if !he.is_null() {
                SetStdHandle(STD_ERROR_HANDLE, he);
            }
            // 若仍然没有可用句柄，退而求其次直接打开当前控制台
            if get(STD_OUTPUT_HANDLE).is_null() {
                let h = CreateFileA(
                    b"CONOUT$\0".as_ptr(),
                    0x4000_0000, // GENERIC_WRITE
                    0x0000_0002, // FILE_SHARE_WRITE
                    std::ptr::null_mut(),
                    3,           // OPEN_EXISTING
                    0,
                    std::ptr::null_mut(),
                );
                if !h.is_null() && h != INVALID {
                    SetStdHandle(STD_OUTPUT_HANDLE, h);
                }
            }
        }
    }

    pub fn set_utf8() {
        unsafe {
            SetConsoleOutputCP(65001);
        }
    }

    fn write_handle(h: *mut c_void, s: &str) {
        unsafe {
            if h.is_null() {
                return;
            }
            let b = s.as_bytes();
            if b.is_empty() {
                return;
            }
            let mut written: u32 = 0;
            WriteFile(h, b.as_ptr(), b.len() as u32, &mut written, std::ptr::null_mut());
        }
    }

    pub fn write(s: &str) {
        write_handle(get(STD_OUTPUT_HANDLE), s);
    }

    pub fn write_err(s: &str) {
        write_handle(get(STD_ERROR_HANDLE), s);
    }

    pub fn flush() {
        // 附加的控制台在进程退出后自动释放，这里无需额外处理
    }

    /// 转换期间把进程的标准输出句柄临时指向一个临时文件。
    /// Rust 标准库每次写 stdout 都会重新取 GetStdHandle，所以替换句柄就能截走依赖库的 println!。
    /// 命令行模式是单线程、逐个文件串行转换，这里的进程级替换不会和别的转换互相干扰；
    /// 真正的 Markdown 在还原句柄之后才输出。
    pub struct Capture {
        saved: *mut c_void,
        path: std::path::PathBuf,
        file: Option<std::fs::File>,
    }

    pub fn capture_stdout() -> Option<Capture> {
        use std::os::windows::io::AsRawHandle;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "mdeditor-stdout-{}-{}.log",
            std::process::id(),
            stamp
        ));
        let file = std::fs::File::create(&path).ok()?;
        let _ = std::io::Write::flush(&mut std::io::stdout());
        unsafe {
            let saved = GetStdHandle(STD_OUTPUT_HANDLE);
            if SetStdHandle(STD_OUTPUT_HANDLE, file.as_raw_handle() as *mut c_void) == 0 {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return None;
            }
            Some(Capture { saved, path, file: Some(file) })
        }
    }

    impl Capture {
        /// 还原标准输出，取回截获的内容；临时文件随即删除
        pub fn finish(mut self) -> String {
            self.restore();
            let bytes = std::fs::read(&self.path).unwrap_or_default();
            let _ = std::fs::remove_file(&self.path);
            String::from_utf8_lossy(&bytes).into_owned()
        }

        fn restore(&mut self) {
            if let Some(file) = self.file.take() {
                let _ = std::io::Write::flush(&mut std::io::stdout());
                unsafe {
                    SetStdHandle(STD_OUTPUT_HANDLE, self.saved);
                }
                drop(file);
            }
        }
    }

    impl Drop for Capture {
        // 转换中途 panic 或提前返回时也要把标准输出还回去
        fn drop(&mut self) {
            if self.file.is_some() {
                self.restore();
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
