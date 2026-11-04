/* ══════════════ pptx → Markdown（按页输出标题 + 要点 + 表格 + 备注） ══════════════ */
use std::collections::HashMap;
use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::{read_entry, Archive};

pub fn convert(data: &[u8]) -> Result<String, String> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("不是有效的 .pptx 文件：{}", e))?;

    let files = collect_slide_files(&mut ar);
    if files.is_empty() {
        return Err("没有找到任何幻灯片".into());
    }
    // 优先按 presentation.xml 的 sldIdLst 顺序（PowerPoint 里调整过顺序后文件名不再代表顺序）
    let ordered = slide_order(&mut ar, &files);

    let mut out = String::new();
    for (seq, name) in ordered.iter().enumerate() {
        let xml = match read_entry(&mut ar, name) {
            Some(x) => x,
            None => continue,
        };
        let slide = parse_slide(&xml);
        let heading = if slide.title.is_empty() {
            format!("幻灯片 {}", seq + 1)
        } else {
            slide.title.clone()
        };
        out.push_str(&format!("## {}\n\n", heading));
        for (lvl, text) in &slide.bullets {
            out.push_str(&format!("{}- {}\n", "  ".repeat((*lvl).min(4)), text));
        }
        if !slide.bullets.is_empty() {
            out.push('\n');
        }
        for t in &slide.tables {
            out.push_str(t);
            out.push('\n');
        }
        if let Some(notes) = notes_text(&mut ar, name) {
            if !notes.is_empty() {
                out.push_str(&format!("> 备注：{}\n\n", notes));
            }
        }
        out.push('\n');
    }
    if out.trim().is_empty() {
        return Err("没有找到任何幻灯片".into());
    }
    Ok(out)
}

/// 列出 zip 里所有 ppt/slides/slideN.xml，附带文件名里的数字用于回退排序
fn collect_slide_files(ar: &mut Archive) -> Vec<(usize, String)> {
    let mut v: Vec<(usize, String)> = Vec::new();
    for i in 0..ar.len() {
        if let Ok(f) = ar.by_index(i) {
            let n = f.name().to_string();
            if let Some(rest) = n.strip_prefix("ppt/slides/slide") {
                if let Some(num) = rest.strip_suffix(".xml") {
                    if let Ok(k) = num.parse::<usize>() {
                        v.push((k, n));
                    }
                }
            }
        }
    }
    v
}

/// 读 *.rels，返回 rId → Target
fn parse_rels(xml: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut r = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(t)) =
                    (super::xml_attr(&e, "Id"), super::xml_attr(&e, "Target"))
                {
                    m.insert(id, t);
                }
            }
            _ => {}
        }
        buf.clear();
    }
    m
}

/// 读 presentation.xml 里 p:sldIdLst/p:sldId 的 r:id（保持文档中的顺序）
fn parse_sld_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut r = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if e.local_name().as_ref() == b"sldId" =>
            {
                if let Some(rid) = super::xml_attr(&e, "r:id") {
                    ids.push(rid);
                }
            }
            _ => {}
        }
        buf.clear();
    }
    ids
}

/// 把 rels 的 Target 规范化成 zip 内路径；base_dir 是 rels 文件所在目录
fn normalize_rel_target(base_dir: &str, t: &str) -> String {
    let t = t.replace('\\', "/");
    if t.starts_with('/') {
        return t.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<String> = base_dir
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    for seg in t.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s.to_string()),
        }
    }
    parts.join("/")
}

/// 按 presentation.xml 的 sldId 顺序解析出 slide 路径；解析失败则回退到文件名数字排序
fn slide_order(ar: &mut Archive, files: &[(usize, String)]) -> Vec<String> {
    if let (Some(pres), Some(rels_xml)) = (
        read_entry(ar, "ppt/presentation.xml"),
        read_entry(ar, "ppt/_rels/presentation.xml.rels"),
    ) {
        let rid_map = parse_rels(&rels_xml);
        let mut ordered: Vec<String> = Vec::new();
        for rid in parse_sld_ids(&pres) {
            if let Some(t) = rid_map.get(&rid) {
                let path = normalize_rel_target("ppt", t);
                if files.iter().any(|(_, n)| n == &path) {
                    ordered.push(path);
                }
            }
        }
        if ordered.len() == files.len() && !ordered.is_empty() {
            return ordered;
        }
    }
    let mut f = files.to_vec();
    f.sort_by_key(|x| x.0);
    f.into_iter().map(|(_, n)| n).collect()
}

/* ────────── 备注 ────────── */

/// 找 slide 的 rels 里 notesSlide 关系的 Target
fn find_notes_target(rels: &str) -> Option<String> {
    let mut r = Reader::from_str(rels);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let ty = super::xml_attr(&e, "Type").unwrap_or_default().to_lowercase();
                if ty.contains("notesslide") {
                    return super::xml_attr(&e, "Target");
                }
            }
            _ => {}
        }
        buf.clear();
    }
    None
}

/// 读该幻灯片对应的 notesSlide，提取备注文本（排除页号等占位符）
fn notes_text(ar: &mut Archive, slide: &str) -> Option<String> {
    let fname = slide.rsplit('/').next()?;
    let rels_name = format!("ppt/slides/_rels/{}.rels", fname);
    let rels = read_entry(ar, &rels_name)?;
    let target = find_notes_target(&rels)?;
    let path = normalize_rel_target("ppt/slides", &target);
    let xml = read_entry(ar, &path)?;
    Some(parse_notes(&xml))
}

fn parse_notes(xml: &str) -> String {
    let mut r = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut sp_depth = 0i32;
    let mut skip_depth = 0i32; // >0 表示处于要忽略的占位符 sp（页号/日期/页脚）
    let mut in_t = false;
    let mut in_fld_skip = false; // slidenum 字段
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"sp" => sp_depth += 1,
                b"ph" => {
                    let ty = super::xml_attr(&e, "type").unwrap_or_default();
                    if matches!(ty.as_str(), "sldNum" | "dt" | "ftr" | "hdr") {
                        skip_depth = sp_depth;
                    }
                }
                b"fld" => {
                    let ty = super::xml_attr(&e, "type").unwrap_or_default().to_lowercase();
                    if ty.contains("slidenum") {
                        in_fld_skip = true;
                    }
                }
                b"t" => in_t = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"ph" => {
                    let ty = super::xml_attr(&e, "type").unwrap_or_default();
                    if matches!(ty.as_str(), "sldNum" | "dt" | "ftr" | "hdr") {
                        skip_depth = sp_depth;
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"sp" => {
                    if skip_depth >= sp_depth {
                        skip_depth = 0;
                    }
                    sp_depth -= 1;
                }
                b"t" => in_t = false,
                b"fld" => in_fld_skip = false,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_t && skip_depth == 0 && !in_fld_skip {
                    let s = t.unescape().unwrap_or_default();
                    if !s.trim().is_empty() {
                        texts.push(s.to_string());
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    texts.join(" ").trim().to_string()
}

/* ────────── 幻灯片正文 ────────── */

struct Slide {
    title: String,
    bullets: Vec<(usize, String)>,
    tables: Vec<String>,
}

fn parse_slide(xml: &str) -> Slide {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut title = String::new();
    let mut bullets: Vec<(usize, String)> = Vec::new();
    let mut tables: Vec<String> = Vec::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"sp" => {
                    let (is_title, paras) = parse_shape(&mut r, &mut buf);
                    let texts: Vec<(usize, String)> = paras
                        .into_iter()
                        .filter(|(_, t)| !t.trim().is_empty())
                        .collect();
                    if is_title && title.is_empty() {
                        title = texts.first().map(|(_, t)| t.trim().to_string()).unwrap_or_default();
                        bullets.extend(texts.into_iter().skip(1));
                    } else {
                        bullets.extend(texts);
                    }
                }
                b"tbl" => {
                    // p:graphicFrame/a:tbl 里的表格文字
                    let md = parse_a_table(&mut r, &mut buf);
                    if !md.is_empty() {
                        tables.push(md);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    Slide { title, bullets, tables }
}

fn parse_shape(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> (bool, Vec<(usize, String)>) {
    let mut is_title = false;
    let mut paras: Vec<(usize, String)> = Vec::new();
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"sp" {
                    break;
                }
            }
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"ph" => {
                    if let Some(t) = super::xml_attr(&e, "type") {
                        if t == "title" || t == "ctrTitle" {
                            is_title = true;
                        }
                    }
                }
                b"p" => paras.push(parse_para(r, buf)),
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"ph" {
                    if let Some(t) = super::xml_attr(&e, "type") {
                        if t == "title" || t == "ctrTitle" {
                            is_title = true;
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    (is_title, paras)
}

fn parse_para(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> (usize, String) {
    let mut lvl = 0usize;
    let mut text = String::new();
    let mut in_t = false;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::End(e)) => {
                let ln = e.local_name().as_ref().to_vec();
                if ln.as_slice() == b"p" {
                    break;
                }
                if ln.as_slice() == b"t" {
                    in_t = false;
                }
            }
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"pPr" | b"lvl" => {
                    if let Some(v) = super::xml_attr(&e, "lvl") {
                        lvl = v.parse().unwrap_or(0);
                    }
                }
                b"t" => in_t = true,
                b"br" => text.push(' '),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"pPr" | b"lvl" => {
                    if let Some(v) = super::xml_attr(&e, "lvl") {
                        lvl = v.parse().unwrap_or(0);
                    }
                }
                b"br" => text.push(' '),
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_t {
                    text.push_str(&t.unescape().unwrap_or_default());
                }
            }
            _ => {}
        }
        buf.clear();
    }
    (lvl, text.trim().to_string())
}

/* ────────── a:tbl 表格 ────────── */

fn parse_a_table(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut tbl_depth = 1i32;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"tbl" => tbl_depth += 1,
                b"tr" => rows.push(parse_a_row(r, buf)),
                _ => {}
            },
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
    super::docx::render_table(rows)
}

fn parse_a_row(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Vec<String> {
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
                    cells.push(parse_a_cell(r, buf));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    cells
}

fn parse_a_cell(r: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> String {
    let mut texts: Vec<String> = Vec::new();
    let mut tc_depth = 1i32;
    let mut in_t = false;
    loop {
        match r.read_event_into(buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"tc" => tc_depth += 1,
                b"t" => in_t = true,
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"tc" => {
                    tc_depth -= 1;
                    if tc_depth <= 0 {
                        break;
                    }
                }
                b"t" => in_t = false,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_t {
                    texts.push(t.unescape().unwrap_or_default().to_string());
                }
            }
            _ => {}
        }
        buf.clear();
    }
    super::cell_text(&texts.join(""))
}
