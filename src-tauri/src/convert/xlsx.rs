/* ══════════════ xlsx / xlsm → Markdown 表格 ══════════════ */
use std::collections::HashMap;
use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::{read_entry, Archive};

/// 单张表最多输出的行数（避免超大表把 Markdown 撑爆）
const MAX_ROWS: usize = 2000;

pub fn convert(data: &[u8]) -> Result<String, String> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("不是有效的 .xlsx 文件：{}", e))?;
    let shared = read_shared_strings(&mut ar);
    let rels = read_rels(&mut ar, "xl/_rels/workbook.xml.rels");
    let sheets = read_sheets(&mut ar, &rels);

    let mut out = String::new();
    for (name, target) in sheets {
        let xml = match read_entry(&mut ar, &target) {
            Some(x) => x,
            None => continue,
        };
        let grid = parse_sheet(&xml, &shared);
        if grid.is_empty() {
            continue;
        }
        out.push_str(&format!("# {}\n\n", name));
        out.push_str(&super::docx::render_table(grid));
        out.push('\n');
    }
    if out.trim().is_empty() {
        return Err(" workbook 里没有找到任何工作表数据".into());
    }
    Ok(out)
}

fn read_rels(ar: &mut Archive, name: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(xml) = read_entry(ar, name) {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) | Ok(Event::Start(e))
                    if e.local_name().as_ref() == b"Relationship" =>
                {
                    let id = super::xml_attr(&e, "Id");
                    let target = super::xml_attr(&e, "Target");
                    if let (Some(i), Some(t)) = (id, target) {
                        map.insert(i, t);
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    map
}

fn read_sheets(ar: &mut Archive, rels: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut sheets: Vec<(String, String)> = Vec::new();
    if let Some(xml) = read_entry(ar, "xl/workbook.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.local_name().as_ref() == b"sheet" => {
                    let name = super::xml_attr(&e, "name").unwrap_or_else(|| "Sheet".into());
                    let rid = super::xml_attr(&e, "r:id").unwrap_or_default();
                    if let Some(target) = rels.get(&rid) {
                        let path = if target.starts_with('/') {
                            format!("xl{}", target)
                        } else {
                            format!("xl/{}", target)
                        };
                        sheets.push((name, path));
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    if sheets.is_empty() {
        // 兜底：直接列出所有 sheetN.xml
        let mut names: Vec<String> = Vec::new();
        for i in 0..ar.len() {
            if let Ok(f) = ar.by_index(i) {
                let n = f.name().to_string();
                if n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml") {
                    names.push(n);
                }
            }
        }
        names.sort();
        for (i, n) in names.into_iter().enumerate() {
            sheets.push((format!("Sheet{}", i + 1), n));
        }
    }
    sheets
}

fn read_shared_strings(ar: &mut Archive) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if let Some(xml) = read_entry(ar, "xl/sharedStrings.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        let mut cur = String::new();
        let mut in_si = false;
        let mut in_rph = 0i32;
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.local_name().as_ref() {
                    b"si" => {
                        in_si = true;
                        cur.clear();
                    }
                    b"rPh" => in_rph += 1,
                    _ => {}
                },
                Ok(Event::End(e)) => match e.local_name().as_ref() {
                    b"si" => {
                        if in_si {
                            v.push(cur.trim().to_string());
                        }
                        in_si = false;
                    }
                    b"rPh" => in_rph -= 1,
                    _ => {}
                },
                Ok(Event::Text(t)) => {
                    if in_si && in_rph == 0 {
                        cur.push_str(&t.unescape().unwrap_or_default());
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    v
}

fn col_of(cell_ref: &str) -> usize {
    let mut n = 0usize;
    for c in cell_ref.chars() {
        if c.is_ascii_alphabetic() {
            n = n * 26 + (c.to_ascii_uppercase() as u8 - b'A' + 1) as usize;
        } else {
            break;
        }
    }
    n.saturating_sub(1)
}

fn clean_number(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Ok(f) = t.parse::<f64>() {
        if f.fract() == 0.0 && f.abs() < 1e15 {
            return format!("{}", f as i64);
        }
    }
    t.to_string()
}

fn parse_sheet(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_col = 0usize;
    let mut cell_type = String::new();
    let mut cell_val = String::new();
    let mut inline = String::new();
    let mut in_v = false;
    let mut in_is = false;
    let mut in_t = false;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"row" => {
                    cur = Vec::new();
                }
                b"c" => {
                    cell_type = super::xml_attr(&e, "t").unwrap_or_default();
                    cell_val.clear();
                    inline.clear();
                    cur_col = super::xml_attr(&e, "r")
                        .map(|s| col_of(&s))
                        .unwrap_or(cur.len());
                }
                b"v" => in_v = true,
                b"is" => in_is = true,
                b"t" => in_t = true,
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"row" => {
                    if rows.len() < MAX_ROWS {
                        rows.push(std::mem::take(&mut cur));
                    }
                    if rows.len() >= MAX_ROWS {
                        break;
                    }
                }
                b"c" => {
                    let value = if !inline.is_empty() {
                        inline.trim().to_string()
                    } else {
                        match cell_type.as_str() {
                            "s" => cell_val
                                .parse::<usize>()
                                .ok()
                                .and_then(|i| shared.get(i).cloned())
                                .unwrap_or_default(),
                            "b" => {
                                if cell_val == "1" {
                                    "TRUE".to_string()
                                } else {
                                    "FALSE".to_string()
                                }
                            }
                            "e" => cell_val.trim().to_string(),
                            _ => clean_number(&cell_val),
                        }
                    };
                    while cur.len() < cur_col {
                        cur.push(String::new());
                    }
                    cur.push(super::cell_text(&value));
                }
                b"v" => in_v = false,
                b"is" => in_is = false,
                b"t" => in_t = false,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let s = t.unescape().unwrap_or_default();
                if in_v {
                    cell_val.push_str(&s);
                } else if in_is && in_t {
                    inline.push_str(&s);
                }
            }
            _ => {}
        }
        buf.clear();
    }

    // 去掉尾部全空的行与列
    while rows.last().map(|r| r.iter().all(|c| c.is_empty())).unwrap_or(false) {
        rows.pop();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for r in rows.iter_mut() {
        while r.len() < cols {
            r.push(String::new());
        }
    }
    // 删掉全空的尾列
    if cols > 0 {
        let mut keep = cols;
        while keep > 0 && rows.iter().all(|r| r[keep - 1].is_empty()) {
            keep -= 1;
        }
        if keep < cols {
            for r in rows.iter_mut() {
                r.truncate(keep);
            }
        }
    }
    rows
}
