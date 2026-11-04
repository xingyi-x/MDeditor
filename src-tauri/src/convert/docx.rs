/* ══════════════ docx → Markdown ══════════════
 * 直接解析 OOXML（zip + XML），保留：标题层级、列表、表格、图片、链接、粗斜体、代码块。
 */
use std::collections::HashMap;
use std::io::Cursor;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{read_entry, Archive, Ctx};

type Rels = HashMap<String, (String, bool)>;
type NumMap = HashMap<String, HashMap<u8, String>>;

#[derive(Default)]
struct StyleInfo {
    name: String,
    outline: Option<u8>,
}

#[derive(Default)]
struct Run {
    text: String,
    b: bool,
    i: bool,
    mono: bool,
    link: Option<String>,
    img: Option<String>,
}

enum Piece {
    Text(Run),
    Img(String),
    Break,
}

struct Env<'a> {
    rels: &'a Rels,
    styles: &'a HashMap<String, StyleInfo>,
    numbering: &'a NumMap,
    ctx: &'a Ctx,
}

pub fn convert(data: &[u8], ctx: &mut Ctx) -> Result<String, String> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("不是有效的 .docx 文件：{}", e))?;
    let rels = read_rels(&mut ar, "word/_rels/document.xml.rels");
    let styles = read_styles(&mut ar);
    let numbering = read_numbering(&mut ar);
    if ctx.opts.images {
        let m = super::extract_media(&mut ar, "word/media/", &ctx.assets_dir, &ctx.assets_rel);
        ctx.media = m;
    }
    let xml = read_entry(&mut ar, "word/document.xml")
        .ok_or_else(|| "缺少 word/document.xml，可能不是标准 docx".to_string())?;
    let env = Env { rels: &rels, styles: &styles, numbering: &numbering, ctx };
    Ok(parse_body(&xml, &env))
}

/* ────────── 通用小工具 ────────── */

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    super::xml_attr(e, key)
}

fn is_mono(font: &str) -> bool {
    let f = font.to_lowercase();
    [
        "consol", "mono", "courier", "menlo", "inconsolata", "source code", "fira code",
        "cascadia", "dejavu sans mono", "liberation mono", "pragmatapro", "jetbrains",
    ]
    .iter()
    .any(|k| f.contains(k))
}

/* ────────── 关系 / 样式 / 编号 ────────── */

fn read_rels(ar: &mut Archive, name: &str) -> Rels {
    let mut map: Rels = HashMap::new();
    if let Some(xml) = read_entry(ar, name) {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) | Ok(Event::Start(e))
                    if e.local_name().as_ref() == b"Relationship" =>
                {
                    if let Some(id) = attr(&e, "Id") {
                        let target = attr(&e, "Target").unwrap_or_default();
                        let external = attr(&e, "TargetMode")
                            .map(|s| s.eq_ignore_ascii_case("external"))
                            .unwrap_or(false);
                        map.insert(id, (target, external));
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    map
}

fn read_styles(ar: &mut Archive) -> HashMap<String, StyleInfo> {
    let mut map: HashMap<String, StyleInfo> = HashMap::new();
    if let Some(xml) = read_entry(ar, "word/styles.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        let mut cur: Option<String> = None;
        let mut info = StyleInfo::default();
        let mut depth = 0i32;
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.local_name().as_ref() {
                    b"style" => {
                        depth = 1;
                        cur = attr(&e, "w:styleId");
                        info = StyleInfo::default();
                    }
                    b"name" if depth > 0 => {
                        if let Some(v) = attr(&e, "w:val") {
                            info.name = v;
                        }
                    }
                    b"outlineLvl" if depth > 0 => {
                        if let Some(v) = attr(&e, "w:val") {
                            info.outline = v.parse::<u8>().ok();
                        }
                    }
                    _ => {}
                },
                Ok(Event::Empty(e)) if depth > 0 => match e.local_name().as_ref() {
                    b"name" => {
                        if let Some(v) = attr(&e, "w:val") {
                            info.name = v;
                        }
                    }
                    b"outlineLvl" => {
                        if let Some(v) = attr(&e, "w:val") {
                            info.outline = v.parse::<u8>().ok();
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.local_name().as_ref() == b"style" => {
                    if let Some(id) = cur.take() {
                        map.insert(id, std::mem::take(&mut info));
                    }
                    depth = 0;
                }
                _ => {}
            }
            buf.clear();
        }
    }
    map
}

fn read_numbering(ar: &mut Archive) -> NumMap {
    let mut abs: HashMap<String, HashMap<u8, String>> = HashMap::new();
    let mut num2abs: HashMap<String, String> = HashMap::new();
    if let Some(xml) = read_entry(ar, "word/numbering.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        let mut cur_abs: Option<String> = None;
        let mut cur_num: Option<String> = None;
        let mut cur_ilvl: u8 = 0;
        let take_fmt = |e: &BytesStart,
                        cur_abs: &Option<String>,
                        cur_ilvl: u8,
                        abs: &mut HashMap<String, HashMap<u8, String>>| {
            if let (Some(a), Some(v)) = (cur_abs.clone(), attr(e, "w:val")) {
                abs.entry(a).or_default().insert(cur_ilvl, v);
            }
        };
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.local_name().as_ref() {
                    b"abstractNum" => {
                        cur_abs = attr(&e, "w:abstractNumId");
                        cur_ilvl = 0;
                    }
                    b"num" => cur_num = attr(&e, "w:numId"),
                    b"lvl" => {
                        if let Some(v) = attr(&e, "w:ilvl") {
                            cur_ilvl = v.parse().unwrap_or(0);
                        }
                    }
                    b"numFmt" => take_fmt(&e, &cur_abs, cur_ilvl, &mut abs),
                    _ => {}
                },
                Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                    b"lvl" => {
                        if let Some(v) = attr(&e, "w:ilvl") {
                            cur_ilvl = v.parse().unwrap_or(0);
                        }
                    }
                    b"numFmt" => take_fmt(&e, &cur_abs, cur_ilvl, &mut abs),
                    b"abstractNumId" => {
                        if let (Some(n), Some(v)) = (cur_num.clone(), attr(&e, "w:val")) {
                            num2abs.insert(n, v);
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.local_name().as_ref() == b"num" => cur_num = None,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut out: NumMap = HashMap::new();
    for (num, abs_id) in num2abs {
        if let Some(m) = abs.get(&abs_id) {
            out.insert(num, m.clone());
        }
    }
    out
}

/* ────────── 主体 ────────── */

fn parse_body(xml: &str, env: &Env) -> String {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"p" => {
                        if let Some(s) = parse_paragraph(&mut r, &mut buf, env, false) {
                            out.push_str(&s);
                        }
                    }
                    b"tbl" => out.push_str(&parse_table(&mut r, &mut buf, env)),
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

fn parse_paragraph(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    env: &Env,
    plain: bool,
) -> Option<String> {
    let mut style_id = String::new();
    let mut ilvl: usize = 0;
    let mut num_id: Option<String> = None;
    let mut pieces: Vec<Piece> = Vec::new();
    let mut extra = String::new();

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"p" {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"pPr" => {
                        let (s, l, n) = parse_ppr(r, buf);
                        style_id = s;
                        ilvl = l;
                        num_id = n;
                    }
                    b"r" => {
                        let mut run = parse_run(r, buf);
                        let img = run.img.take();
                        if !run.text.trim().is_empty() || run.link.is_some() {
                            pieces.push(Piece::Text(run));
                        }
                        if let Some(v) = img {
                            pieces.push(Piece::Img(v));
                        }
                    }
                    b"hyperlink" => {
                        let rid = attr(&e, "r:id");
                        let anchor = attr(&e, "w:anchor");
                        let txt = parse_hyperlink_text(r, buf);
                        let target = rid
                            .as_ref()
                            .and_then(|id| env.rels.get(id))
                            .map(|(t, _)| t.clone())
                            .or_else(|| anchor.map(|a| format!("#{}", a)));
                        if !txt.is_empty() {
                            pieces.push(Piece::Text(Run { text: txt, link: target, ..Default::default() }));
                        }
                    }
                    b"txbxContent" => {
                        extra.push_str(&parse_until(r, buf, b"txbxContent", env));
                    }
                    b"oMath" | b"oMathPara" => {
                        let t = collect_text(r, buf, &ln);
                        if !t.trim().is_empty() {
                            let mut run = Run::default();
                            run.text = t.trim().to_string();
                            run.mono = true;
                            pieces.push(Piece::Text(run));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"br" | b"cr" => pieces.push(Piece::Break),
                b"tab" => pieces.push(Piece::Text(Run { text: " ".into(), ..Default::default() })),
                b"blip" => {
                    if let Some(v) = attr(&e, "r:embed").or_else(|| attr(&e, "r:link")) {
                        pieces.push(Piece::Img(v));
                    }
                }
                b"imagedata" => {
                    if let Some(v) = attr(&e, "r:id") {
                        pieces.push(Piece::Img(v));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }

    // 组装行内文本：连续的同格式 run 先合并，再统一加标记，避免 **前半****后半**
    let mut inline = String::new();
    let mut imgs: Vec<String> = Vec::new();
    let mut run_buf: Vec<Run> = Vec::new();
    for p in pieces {
        match p {
            Piece::Text(run) => run_buf.push(run),
            Piece::Break => {
                inline.push_str(&render_merged_runs(std::mem::take(&mut run_buf)));
                inline.push_str("  \n");
            }
            Piece::Img(rid) => {
                inline.push_str(&render_merged_runs(std::mem::take(&mut run_buf)));
                if let Some(p) = resolve_img(&rid, env) {
                    imgs.push(p);
                }
            }
        }
    }
    inline.push_str(&render_merged_runs(run_buf));
    if !extra.is_empty() {
        inline.push_str(&extra);
    }

    let text = inline.trim().to_string();
    let mut block = String::new();
    for im in &imgs {
        block.push_str(&format!("![]({})\n\n", im));
    }
    if text.is_empty() {
        return if block.is_empty() { None } else { Some(block) };
    }
    if plain {
        return Some(text);
    }

    let style = env.styles.get(&style_id);
    let name_l = style.map(|s| s.name.to_lowercase()).unwrap_or_default();
    let sid_l = style_id.to_lowercase();

    if let Some(l) = heading_level(&style_id, style) {
        block.push_str(&format!("{} {}\n\n", "#".repeat(l as usize), text));
        return Some(block);
    }
    if let Some(marker) = list_marker(num_id.as_deref(), ilvl, env.numbering) {
        block.push_str(&format!("{}{} {}\n", "  ".repeat(ilvl), marker, text));
        return Some(block);
    }
    if name_l.contains("quote") || sid_l == "quote" || name_l.contains("引用") {
        block.push_str(&format!("> {}\n\n", text.replace('\n', "\n> ")));
        return Some(block);
    }
    if name_l.contains("code")
        || name_l.contains("preformatted")
        || name_l.contains("macro")
        || sid_l.contains("code")
    {
        block.push_str(&format!("```\n{}\n```\n\n", text));
        return Some(block);
    }
    block.push_str(&format!("{}\n\n", text));
    Some(block)
}

fn parse_until(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    end: &[u8],
    env: &Env,
) -> String {
    let mut out = String::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == end {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"p" => {
                        if let Some(s) = parse_paragraph(r, buf, env, false) {
                            out.push_str(&s);
                        }
                    }
                    b"tbl" => out.push_str(&parse_table(r, buf, env)),
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

fn collect_text(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>, end: &[u8]) -> String {
    let mut s = String::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == end {
                    break;
                }
            }
            Ok(Event::Text(t)) => {
                s.push_str(&t.unescape().unwrap_or_default());
            }
            _ => {}
        }
        buf.clear();
    }
    s
}

fn parse_ppr(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> (String, usize, Option<String>) {
    let mut style = String::new();
    let mut ilvl: usize = 0;
    let mut num: Option<String> = None;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"pPr" {
                    break;
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"pStyle" => {
                    if let Some(v) = attr(&e, "w:val") {
                        style = v;
                    }
                }
                b"ilvl" => {
                    if let Some(v) = attr(&e, "w:val") {
                        ilvl = v.parse().unwrap_or(0);
                    }
                }
                b"numId" => {
                    if let Some(v) = attr(&e, "w:val") {
                        num = Some(v);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    (style, ilvl, num)
}

fn parse_run(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Run {
    let mut run = Run::default();
    let mut in_t = false;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                if ln.as_slice() == b"r" {
                    break;
                }
                if ln.as_slice() == b"t" {
                    in_t = false;
                }
            }
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"t" => in_t = true,
                    b"b" => run.b = toggle(&e, true),
                    b"i" => run.i = toggle(&e, true),
                    b"rFonts" => {
                        if let Some(f) = attr(&e, "w:ascii").or_else(|| attr(&e, "w:hAnsi")) {
                            run.mono = is_mono(&f);
                        }
                    }
                    b"br" | b"cr" => run.text.push('\n'),
                    b"tab" => run.text.push(' '),
                    b"blip" => {
                        if let Some(v) = attr(&e, "r:embed").or_else(|| attr(&e, "r:link")) {
                            run.img = Some(v);
                        }
                    }
                    b"imagedata" => {
                        if let Some(v) = attr(&e, "r:id") {
                            run.img = Some(v);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"b" => run.b = toggle(&e, true),
                b"i" => run.i = toggle(&e, true),
                b"rFonts" => {
                    if let Some(f) = attr(&e, "w:ascii").or_else(|| attr(&e, "w:hAnsi")) {
                        run.mono = is_mono(&f);
                    }
                }
                b"br" | b"cr" => run.text.push('\n'),
                b"tab" => run.text.push(' '),
                b"blip" => {
                    if let Some(v) = attr(&e, "r:embed").or_else(|| attr(&e, "r:link")) {
                        run.img = Some(v);
                    }
                }
                b"imagedata" => {
                    if let Some(v) = attr(&e, "r:id") {
                        run.img = Some(v);
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_t {
                    run.text.push_str(&t.unescape().unwrap_or_default());
                }
            }
            _ => {}
        }
        buf.clear();
    }
    run
}

fn toggle(e: &BytesStart, default: bool) -> bool {
    match attr(e, "w:val") {
        Some(v) => !matches!(v.as_str(), "0" | "false" | "off" | "none"),
        None => default,
    }
}

fn parse_hyperlink_text(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> String {
    let mut runs: Vec<Run> = Vec::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"hyperlink" {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"r" {
                    runs.push(parse_run(r, buf));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    render_merged_runs(runs).trim().to_string()
}

/// 两个 run 的格式标记是否完全一致（同格式才合并）
fn same_format(a: &Run, b: &Run) -> bool {
    a.b == b.b && a.i == b.i && a.mono == b.mono && a.link == b.link
}

/// 合并相邻同格式 run 后统一渲染，避免同格式的相邻 run 各自包 **…** 产生 **前半****后半**
fn render_merged_runs(runs: Vec<Run>) -> String {
    let mut out = String::new();
    let mut pending: Option<Run> = None;
    for run in runs {
        if let Some(prev) = pending.as_mut() {
            if same_format(prev, &run) {
                prev.text.push_str(&run.text);
            } else {
                out.push_str(&render_run(prev));
                *prev = run;
            }
        } else {
            pending = Some(run);
        }
    }
    if let Some(prev) = pending.take() {
        out.push_str(&render_run(&prev));
    }
    out
}

fn render_run(run: &Run) -> String {
    if run.text.is_empty() {
        return String::new();
    }
    let t = &run.text;
    let core = t.trim();
    if core.is_empty() {
        return t.clone();
    }
    let lead = &t[..t.len() - t.trim_start().len()];
    let trail = &t[t.trim_end().len()..];
    let mut s = core.to_string();
    if run.mono && !s.contains('`') {
        s = format!("`{}`", s);
    }
    if run.i {
        s = format!("*{}*", s);
    }
    if run.b {
        s = format!("**{}**", s);
    }
    if let Some(link) = &run.link {
        s = format!("[{}]({})", s, link);
    }
    format!("{}{}{}", lead, s, trail)
}

fn resolve_img(rid: &str, env: &Env) -> Option<String> {
    let (target, external) = env.rels.get(rid)?;
    if *external {
        return Some(target.clone());
    }
    let zip_path = if target.starts_with('/') {
        format!("word{}", target)
    } else {
        format!("word/{}", target)
    };
    env.ctx.img(&zip_path)
}

fn heading_level(id: &str, style: Option<&StyleInfo>) -> Option<u8> {
    if let Some(si) = style {
        if let Some(l) = si.outline {
            if l < 9 {
                return Some((l + 1).clamp(1, 6));
            }
        }
    }
    let name = style.map(|s| s.name.as_str()).unwrap_or("");
    if let Some(l) = level_from_name(name) {
        return Some(l);
    }
    level_from_name(id)
}

fn level_from_name(s: &str) -> Option<u8> {
    let low = s.trim().to_lowercase().replace([' ', '_', '-'], "");
    if low.is_empty() {
        return None;
    }
    if low == "title" || low == "titre" || low == "标题" {
        return Some(1);
    }
    if low == "subtitle" || low == "副标题" {
        return Some(2);
    }
    if low.starts_with("heading")
        || low.starts_with("标题")
        || low.starts_with("標題")
    {
        let digits: String = low.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u8>() {
            return Some(n.clamp(1, 6));
        }
        for (i, ch) in ["一", "二", "三", "四", "五", "六", "七", "八", "九"]
            .iter()
            .enumerate()
        {
            if low.contains(ch) {
                return Some(((i + 1) as u8).min(6));
            }
        }
        return Some(1);
    }
    None
}

fn list_marker(num_id: Option<&str>, ilvl: usize, numbering: &NumMap) -> Option<String> {
    let id = num_id?;
    if id == "0" || id.is_empty() {
        return None;
    }
    let fmt = numbering
        .get(id)
        .and_then(|m| m.get(&(ilvl as u8)))
        .cloned()
        .unwrap_or_else(|| "bullet".to_string());
    let marker = if fmt == "bullet" || fmt == "none" || fmt.is_empty() {
        "-"
    } else {
        "1."
    };
    Some(marker.to_string())
}

/* ────────── 表格 ────────── */

/// 单元格解析结果：文本 + 横向跨度 + 是否为纵向合并的延续单元格
struct Cell {
    text: String,
    grid_span: usize,
    vmerge_cont: bool,
}

fn parse_table(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>, env: &Env) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"tbl" {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"tr" {
                    rows.push(parse_row(r, buf, env));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    render_table(rows)
}

fn parse_row(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>, env: &Env) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"tr" {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"tc" {
                    let cell = parse_cell(r, buf, env);
                    // 纵向合并的延续单元格：清空文本但保留占位，避免后续行错列
                    let text = if cell.vmerge_cont { String::new() } else { cell.text };
                    cells.push(text);
                    // gridSpan：补 span-1 个空单元格，保证列对齐
                    for _ in 1..cell.grid_span {
                        cells.push(String::new());
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    cells
}

fn parse_cell(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>, env: &Env) -> Cell {
    let mut parts: Vec<String> = Vec::new();
    let mut grid_span = 1usize;
    let mut vmerge_cont = false;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"tc" {
                    break;
                }
            }
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"p" => {
                        if let Some(s) = parse_paragraph(r, buf, env, true) {
                            if !s.is_empty() {
                                parts.push(s);
                            }
                        }
                    }
                    b"tcPr" => {
                        let (span, cont) = parse_tcpr(r, buf);
                        grid_span = span;
                        vmerge_cont = cont;
                    }
                    b"tbl" => {
                        // 嵌套表格：整体消费掉，降级为把内层文本用 <br> 拼进外层单元格；
                        // 内层的 </w:tc> 不会泄漏到本层循环，避免提前结束外层单元格
                        let inner = parse_table_text(r, buf, env);
                        if !inner.is_empty() {
                            parts.push(inner);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Cell { text: super::cell_text(&parts.join("<br>")), grid_span, vmerge_cont }
}

/// 读取 w:tcPr 内的 gridSpan / vMerge（消费到 </w:tcPr>）
fn parse_tcpr(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> (usize, bool) {
    let mut span = 1usize;
    let mut cont = false;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"tcPr" {
                    break;
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"gridSpan" => {
                    if let Some(v) = attr(&e, "val") {
                        span = v.parse::<usize>().unwrap_or(1).max(1);
                    }
                }
                b"vMerge" => {
                    // restart 表示合并起始（保留内容），无 val 或非 restart 表示延续（清空占位）
                    let is_restart = attr(&e, "val")
                        .map(|v| v.eq_ignore_ascii_case("restart"))
                        .unwrap_or(false);
                    cont = !is_restart;
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    (span, cont)
}

/// 把嵌套 w:tbl 的文本拼成一段（消费到 </w:tbl>），避免内层 </w:tc> 提前关闭外层单元格
fn parse_table_text(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>, env: &Env) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut tbl_depth = 1i32;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                match ln.as_slice() {
                    b"tbl" => tbl_depth += 1,
                    b"p" => {
                        if let Some(s) = parse_paragraph(r, buf, env, true) {
                            if !s.is_empty() {
                                lines.push(s);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"tbl" {
                    tbl_depth -= 1;
                    if tbl_depth <= 0 {
                        break;
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    lines.join("<br>")
}

pub fn render_table(rows: Vec<Vec<String>>) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let mut rows = rows;
    for r in rows.iter_mut() {
        while r.len() < cols {
            r.push(String::new());
        }
    }
    let mut out = String::new();
    out.push_str(&format!("| {} |\n", rows[0].join(" | ")));
    out.push_str("|");
    for _ in 0..cols {
        out.push_str(" --- |");
    }
    out.push('\n');
    for r in rows.iter().skip(1) {
        out.push_str(&format!("| {} |\n", r.join(" | ")));
    }
    out.push('\n');
    out
}
