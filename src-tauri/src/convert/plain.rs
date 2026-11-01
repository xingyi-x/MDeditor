/* ══════════════ 纯文本 / CSV / Markdown 直通 ══════════════ */

pub fn text(data: &[u8]) -> String {
    let s = decode(data);
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    // 折叠连续空行
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0;
    for line in s.split('\n') {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}

pub fn delimited(data: &[u8], tab: bool) -> String {
    let s = decode(data);
    let sep = if tab { '\t' } else { ',' };
    let rows = parse_rows(&s, sep);
    if rows.is_empty() {
        return String::new();
    }
    super::docx::render_table(rows)
}

fn decode(data: &[u8]) -> String {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&data[3..]).into_owned();
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode(&data[2..]).0.into_owned();
    }
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(data).0.into_owned(),
    }
}

/// 带引号的 CSV 解析
fn parse_rows(s: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            '\r' => {}
            '\n' if in_quotes => field.push('\n'),
            '\n' => {
                cur.push(super::cell_text(&field));
                field.clear();
                rows.push(std::mem::take(&mut cur));
                if rows.len() >= 2000 {
                    break;
                }
            }
            c if c == sep && !in_quotes => {
                cur.push(super::cell_text(&field));
                field.clear();
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !cur.is_empty() {
        cur.push(super::cell_text(&field));
        rows.push(cur);
    }
    // 去掉尾部空行
    while rows.last().map(|r| r.iter().all(|c| c.is_empty())).unwrap_or(false) {
        rows.pop();
    }
    rows
}
