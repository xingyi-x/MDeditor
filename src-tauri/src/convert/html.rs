/* ══════════════ HTML → Markdown ══════════════
 * 手写标签扫描器：标题 / 段落 / 列表 / 表格 / 代码块 / 链接 / 图片 / 引用
 */
pub fn convert(data: &[u8]) -> Result<String, String> {
    let text = decode_html(data);
    Ok(html_to_md(&text))
}

/* ────────── 编码识别 ────────── */

pub fn decode_html(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode(&bytes[2..]).0.into_owned();
    }
    if let Some(enc) = detect_charset(bytes) {
        return enc.decode(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

fn detect_charset(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let head_len = bytes.len().min(4096);
    let head = String::from_utf8_lossy(&bytes[..head_len]);
    let lower = head.to_lowercase();
    let idx = lower.find("charset=")?;
    let rest = &lower[idx + 8..];
    let end = rest
        .find(|c: char| c == '"' || c == '\'' || c == '>' || c == ';' || c == ' ' || c == '/')
        .unwrap_or(rest.len().min(20));
    let label = rest[..end].trim();
    if label.is_empty() {
        return None;
    }
    encoding_rs::Encoding::for_label(label.as_bytes())
}

/* ────────── 实体解码 ────────── */

fn entity_char(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let (radix, digits) = if let Some(h) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            (16u32, h)
        } else {
            (10u32, num)
        };
        return u32::from_str_radix(digits, radix)
            .ok()
            .and_then(char::from_u32);
    }
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "ldquo" => '“',
        "rdquo" => '”',
        "lsquo" => '‘',
        "rsquo" => '’',
        "times" => '×',
        "deg" => '°',
        "yen" => '¥',
        _ => return None,
    };
    Some(c)
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        if bytes[i] == b'&' {
            if let Some(rel) = s[i..].find(';') {
                let ent = &s[i + 1..i + rel];
                if !ent.is_empty() && ent.len() < 12 {
                    if let Some(c) = entity_char(ent) {
                        out.push(c);
                        i += rel + 1;
                        continue;
                    }
                }
            }
        }
        let len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        out.push_str(&s[i..i + len]);
        i += len;
    }
    out
}

/* ────────── 标签扫描 ────────── */

struct Tag {
    name: String,
    closing: bool,
    selfclose: bool,
    attrs: String,
    end: usize,
}

fn parse_tag(s: &str, i: usize) -> Option<Tag> {
    let bytes = s.as_bytes();
    if bytes[i] != b'<' {
        return None;
    }
    if i + 1 < s.len() && bytes[i + 1] == b'!' {
        return None;
    }
    let mut j = i + 1;
    let closing = j < s.len() && bytes[j] == b'/';
    if closing {
        j += 1;
    }
    let start = j;
    while j < s.len()
        && !bytes[j].is_ascii_whitespace()
        && bytes[j] != b'>'
        && bytes[j] != b'/'
    {
        j += 1;
    }
    let name = s[start..j].to_lowercase();
    if name.is_empty() {
        return None;
    }
    let attr_start = j;
    while j < s.len() && bytes[j] != b'>' {
        if bytes[j] == b'"' || bytes[j] == b'\'' {
            let q = bytes[j];
            j += 1;
            while j < s.len() && bytes[j] != q {
                j += 1;
            }
        }
        j += 1;
    }
    let attrs = s[attr_start..j.min(s.len())].to_string();
    let selfclose = attrs.trim_end().ends_with('/');
    Some(Tag { name, closing, selfclose, attrs, end: (j + 1).min(s.len()) })
}

fn attr_of(attrs: &str, key: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let kb = key.as_bytes();
    let mut j = 0usize;
    while j + kb.len() + 1 <= attrs.len() {
        if bytes[j..j + kb.len()].eq_ignore_ascii_case(kb) && bytes[j + kb.len()] == b'=' {
            let rest = attrs[j + kb.len() + 1..].trim_start();
            return attr_value(rest);
        }
        j += 1;
    }
    None
}

fn attr_value(rest: &str) -> Option<String> {
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"').unwrap_or(stripped.len());
        return Some(decode_entities(&stripped[..end]));
    }
    if let Some(stripped) = rest.strip_prefix('\'') {
        let end = stripped.find('\'').unwrap_or(stripped.len());
        return Some(decode_entities(&stripped[..end]));
    }
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '>')
        .unwrap_or(rest.len());
    Some(decode_entities(rest[..end].trim()))
}

/// 从 from 开始查找 </tag（忽略大小写），返回起始下标
fn find_close_tag(html: &str, from: usize, tag: &str) -> Option<usize> {
    let bytes = html.as_bytes();
    let pat = format!("</{}", tag).into_bytes();
    let mut j = from;
    while j + pat.len() <= html.len() {
        if bytes[j..j + pat.len()].eq_ignore_ascii_case(&pat) {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn ensure_blank(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if out.ends_with("\n\n") {
        return;
    }
    if out.ends_with('\n') {
        out.push('\n');
        return;
    }
    out.push_str("\n\n");
}

fn ensure_line(out: &mut String) {
    if out.is_empty() || out.ends_with('\n') {
        return;
    }
    out.push('\n');
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    let bytes = s.as_bytes();
    while i < s.len() {
        if bytes[i] == b'<' {
            if let Some(t) = parse_tag(s, i) {
                i = t.end;
                continue;
            }
        }
        let len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        out.push_str(&s[i..i + len]);
        i += len;
    }
    out
}

/// 忽略大小写的子串查找
fn find_ci(s: &str, from: usize, pat: &str) -> Option<usize> {
    let b = s.as_bytes();
    let p = pat.as_bytes();
    if p.is_empty() {
        return None;
    }
    let mut j = from;
    while j + p.len() <= s.len() {
        if b[j..j + p.len()].eq_ignore_ascii_case(p) {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// 把 <table>...</table> 片段转成 Markdown 表格
fn table_to_md(fragment: &str) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut pos = 0usize;
    while let Some(rs) = find_ci(fragment, pos, "<tr") {
        let re = find_ci(fragment, rs, "</tr").map(|x| x + 4);
        let row_end = re.unwrap_or(fragment.len());
        let row = &fragment[rs..row_end];
        let mut cells: Vec<String> = Vec::new();
        let mut p = 0usize;
        loop {
            let td = find_ci(row, p, "<td");
            let th = find_ci(row, p, "<th");
            let cs = match (td, th) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                _ => None,
            };
            let cs = match cs {
                Some(v) => v,
                None => break,
            };
            let ce = find_ci(row, cs, "</td").or_else(|| find_ci(row, cs, "</th"));
            let cell_end = ce.unwrap_or(row.len());
            let cell = &row[cs..cell_end];
            let gt = cell.find('>').map(|x| x + 1).unwrap_or(0);
            let text = collapse_ws(&decode_entities(&strip_tags(&cell[gt..])));
            cells.push(super::cell_text(&text));
            p = cell_end + 4;
            if p >= row.len() {
                break;
            }
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
        pos = row_end + 1;
        if pos >= fragment.len() {
            break;
        }
    }
    super::docx::render_table(rows)
}

pub fn html_to_md(html: &str) -> String {
    let mut out = String::new();
    let mut title: Option<String> = None;
    let mut i = 0usize;
    let bytes = html.as_bytes();

    let mut skipping: Option<String> = None;
    let mut in_pre = false;
    let mut in_title = false;
    let mut list_depth = 0usize;
    let mut link_stack: Vec<String> = Vec::new();
    let mut quote_depth = 0usize;

    while i < html.len() {
        // <!DOCTYPE ...> / <![CDATA[...]]> 之类声明：整段跳过
        if bytes[i] == b'<' && i + 1 < html.len() && bytes[i + 1] == b'!' {
            if let Some(rel) = html[i..].find('>') {
                i += rel + 1;
                continue;
            }
        }
        // 注释
        if html[i..].starts_with("<!--") {
            if let Some(rel) = html[i..].find("-->") {
                i += rel + 3;
                continue;
            }
            break;
        }
        if bytes[i] == b'<' {
            if let Some(tag) = parse_tag(html, i) {
                i = tag.end;
                let name = tag.name.as_str();

                // 跳过区域的结束判断
                if let Some(skip_name) = skipping.clone() {
                    if tag.closing && name == skip_name {
                        skipping = None;
                        in_title = false;
                    }
                    continue;
                }

                if !tag.closing && matches!(name, "script" | "style" | "noscript" | "svg" | "iframe" | "form" | "select") {
                    if !tag.selfclose {
                        skipping = Some(name.to_string());
                    }
                    continue;
                }
                if !tag.closing && name == "head" && !tag.selfclose {
                    skipping = Some("head".to_string());
                    continue;
                }
                if name == "title" {
                    if !tag.closing {
                        in_title = true;
                    } else {
                        in_title = false;
                    }
                    continue;
                }

                if tag.selfclose || matches!(name, "br" | "img" | "hr" | "meta" | "link" | "input" | "wbr" | "col" | "area" | "source") {
                    match name {
                        "br" => {
                            if in_pre {
                                out.push('\n');
                            } else {
                                ensure_line(&mut out);
                            }
                        }
                        "hr" => {
                            ensure_blank(&mut out);
                            out.push_str("---\n\n");
                        }
                        "img" => {
                            let src = attr_of(&tag.attrs, "src").unwrap_or_default();
                            if !src.is_empty() {
                                let alt = attr_of(&tag.attrs, "alt").unwrap_or_default();
                                out.push_str(&format!("![{}]({})", collapse_ws(&alt), src));
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                if tag.closing {
                    match name {
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => ensure_blank(&mut out),
                        "p" | "div" | "section" | "article" | "tr" | "table" | "ul" | "ol"
                        | "blockquote" | "figure" | "figcaption" | "header" | "footer" | "main"
                        | "nav" | "aside" | "dl" | "dd" | "dt" => {
                            if name == "ul" || name == "ol" {
                                list_depth = list_depth.saturating_sub(1);
                            }
                            if name == "blockquote" {
                                quote_depth = quote_depth.saturating_sub(1);
                            }
                            ensure_blank(&mut out);
                        }
                        "li" => ensure_line(&mut out),
                        "a" => {
                            let href = link_stack.pop().unwrap_or_default();
                            out.push_str(&format!("]({})", href));
                        }
                        "strong" | "b" => out.push_str("**"),
                        "em" | "i" => out.push('*'),
                        "del" | "s" | "strike" => out.push_str("~~"),
                        "code" | "kbd" | "samp" | "tt" => {
                            if !in_pre {
                                out.push('`');
                            }
                        }
                        "pre" => {
                            in_pre = false;
                            out.push_str("\n```\n\n");
                        }
                        _ => {}
                    }
                    continue;
                }

                // 开始标签
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        ensure_blank(&mut out);
                        let level = name[1..].parse::<usize>().unwrap_or(1);
                        out.push_str(&"#".repeat(level));
                        out.push(' ');
                    }
                    "p" | "div" | "section" | "article" | "figure" | "figcaption" | "tr"
                    | "header" | "footer" | "main" | "nav" | "aside" => ensure_blank(&mut out),
                    "li" => {
                        ensure_line(&mut out);
                        let indent = "  ".repeat(list_depth.saturating_sub(1));
                        out.push_str(&format!("{}- ", indent));
                    }
                    "ul" | "ol" => {
                        ensure_blank(&mut out);
                        list_depth += 1;
                    }
                    "blockquote" => {
                        ensure_blank(&mut out);
                        quote_depth += 1;
                        out.push_str("> ");
                    }
                    "pre" => {
                        ensure_blank(&mut out);
                        out.push_str("```\n");
                        in_pre = true;
                    }
                    "code" | "kbd" | "samp" | "tt" => {
                        if !in_pre {
                            out.push('`');
                        }
                    }
                    "strong" | "b" => out.push_str("**"),
                    "em" | "i" => out.push('*'),
                    "del" | "s" | "strike" => out.push_str("~~"),
                    "a" => {
                        let href = attr_of(&tag.attrs, "href").unwrap_or_default();
                        link_stack.push(href);
                        out.push('[');
                    }
                    "table" => {
                        ensure_blank(&mut out);
                        if let Some(rel) = find_close_tag(html, i, "table") {
                            let end = (rel + 8).min(html.len());
                            out.push_str(&table_to_md(&html[i..rel]));
                            i = end;
                            ensure_blank(&mut out);
                        }
                    }
                    "td" | "th" | "tbody" | "thead" | "tfoot" | "caption" => {}
                    _ => {}
                }
                continue;
            }
            i += 1;
            continue;
        }

        // 文本
        let start = i;
        while i < html.len() && bytes[i] != b'<' {
            i += 1;
        }
        let raw = &html[start..i];
        if in_pre {
            out.push_str(&decode_entities(raw));
        } else {
            let text = collapse_ws(&decode_entities(raw));
            if in_title && title.is_none() && !text.trim().is_empty() {
                title = Some(text.trim().to_string());
            }
            if skipping.is_some() {
                continue;
            }
            out.push_str(&text);
        }
    }

    if let Some(t) = title {
        if !out.trim_start().starts_with('#') {
            out = format!("# {}\n\n{}", t, out);
        }
    }
    let _ = quote_depth;
    out
}
