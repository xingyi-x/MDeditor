/* ══════════════ 命令行模式：文件 / 目录 → Markdown ══════════════
 *
 *   mdeditor.exe --to-md <文件或目录> [选项]
 *
 * 因为是 GUI 子系统的 exe（双击不弹黑框），命令行模式下需要主动附加到父控制台输出。
 */
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
  mdeditor.exe --to-md D:\\docs --r --d D:\\docs\\md
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

fn emit(s: &str) {
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
                emit(USAGE);
                std::process::exit(0);
            }
            "--version" => {
                emit(&format!("MDeditor {}", env!("CARGO_PKG_VERSION")));
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
    for f in &files {
        let out_path = target_path(f, &o, files.len() == 1);
        match convert::convert_file_to_string(f, &out_path, &opts) {
            Err(e) => {
                failed += 1;
                emit(&format!("[失败] {} — {}", f.display(), e));
            }
            Ok(mut md) => {
                if opts.front_matter {
                    md = convert::front_matter_of(f, &md);
                }
                if o.stdout {
                    emit(&md);
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
                            emit(&out_path.to_string_lossy());
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

fn target_path(f: &Path, o: &Opts, single: bool) -> PathBuf {
    // -o 只对单文件模式有意义：批量时若每个文件都往同一个路径写，会互相覆盖
    if single {
        if let Some(out) = &o.out {
            return PathBuf::from(out);
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
    if base.exists() && !o.force {
        // 避免覆盖：自动改名
        let stem = base
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for i in 1..999 {
            let cand = base.with_file_name(format!("{}_{}.md", stem, i));
            if !cand.exists() {
                return cand;
            }
        }
    }
    base
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

    pub fn write(s: &str) {
        unsafe {
            let h = get(STD_OUTPUT_HANDLE);
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

    pub fn flush() {
        // 附加的控制台在进程退出后自动释放，这里无需额外处理
    }
}
