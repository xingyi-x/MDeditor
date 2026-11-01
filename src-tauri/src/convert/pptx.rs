/* ══════════════ pptx → Markdown（按页输出标题 + 要点） ══════════════ */
use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::read_entry;

pub fn convert(data: &[u8]) -> Result<String, String> {
    let mut ar = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("不是有效的 .pptx 文件：{}", e))?;

    let mut slides: Vec<(usize, String)> = Vec::new();
    for i in 0..ar.len() {
        if let Ok(f) = ar.by_index(i) {
            let n = f.name().to_string();
            if let Some(rest) = n.strip_prefix("ppt/slides/slide") {
                if let Some(num) = rest.strip_suffix(".xml") {
                    if let Ok(k) = num.parse::<usize>() {
                        slides.push((k, n));
                    }
                }
            }
        }
    }
    slides.sort_by_key(|x| x.0);

    let mut out = String::new();
    for (idx, name) in slides {
        let xml = match read_entry(&mut ar, &name) {
            Some(x) => x,
            None => continue,
        };
        let (title, bullets, notes) = parse_slide(&xml);
        let heading = if title.is_empty() {
            format!("幻灯片 {}", idx)
        } else {
            title
        };
        out.push_str(&format!("## {}\n\n", heading));
        for (lvl, text) in bullets {
            out.push_str(&format!("{}- {}\n", "  ".repeat(lvl.min(4)), text));
        }
        if !notes.is_empty() {
            out.push_str(&format!("\n> 备注：{}\n", notes));
        }
        out.push('\n');
    }
    if out.trim().is_empty() {
        return Err("没有找到任何幻灯片".into());
    }
    Ok(out)
}

fn parse_slide(xml: &str) -> (String, Vec<(usize, String)>, String) {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut title = String::new();
    let mut bullets: Vec<(usize, String)> = Vec::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"sp" {
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
            }
            _ => {}
        }
        buf.clear();
    }
    (title, bullets, String::new())
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
