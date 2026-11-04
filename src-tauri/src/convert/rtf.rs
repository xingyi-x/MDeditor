/* ══════════════ RTF → Markdown（够用的简化解析） ══════════════ */

const IGNORE_DEST: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "shppict", "nonshppict",
    "listtable", "listoverridetable", "revtbl", "docvar", "panose", "falt", "ffdeftext",
    "datafield", "themedata", "colorschememapping", "latentstyles", "datastore", "creatim",
    "revtim", "printim", "buptim", "protusertbl", "xe", "tc", "*",
];

/// 一位十六进制字符 → 数值（按字节判断，避免字符串切片越过 UTF-8 边界）
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// \uN 之后按 \ucN 跳过 N 个替代字符：可能是普通 ?，也可能是 \'hh 形式（跳过 \' + 两位十六进制）
fn skip_fallback(i: &mut usize, s: &str, count: i32) {
    let bytes = s.as_bytes();
    let mut n = count.max(0);
    while n > 0 && *i < s.len() {
        if bytes[*i] == b'\\' && *i + 1 < s.len() && bytes[*i + 1] == b'\'' {
            *i += 2;
            while *i < s.len() && bytes[*i].is_ascii_hexdigit() {
                *i += 1;
            }
        } else {
            let len = s[*i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            *i += len;
        }
        n -= 1;
    }
}

pub fn convert(data: &[u8]) -> Result<String, String> {
    let raw = String::from_utf8_lossy(data);
    let enc = detect_encoding(&raw);
    let s: &str = raw.as_ref();
    let bytes = s.as_bytes();

    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    // 栈元素：(bold, italic, uc)，uc 随分组压栈/出栈以恢复作用域
    let mut stack: Vec<(bool, bool, i32)> = Vec::new();
    let mut uc: i32 = 1; // \ucN 默认 1
    let mut high_surrogate: Option<u32> = None; // \uN 的高代理（UTF-16 代理对）
    let mut depth: i32 = 0;
    let mut skip_to: Option<i32> = None;
    let mut i = 0usize;

    let flush_pending = |pending: &mut Vec<u8>, out: &mut String, enc: &'static encoding_rs::Encoding| {
        if pending.is_empty() {
            return;
        }
        out.push_str(enc.decode(pending).0.as_ref());
        pending.clear();
    };

    while i < s.len() {
        let c = bytes[i];
        match c {
            b'{' => {
                flush_pending(&mut pending, &mut out, enc);
                depth += 1;
                stack.push((bold, italic, uc));
                i += 1;
            }
            b'}' => {
                flush_pending(&mut pending, &mut out, enc);
                if let Some((b, it, u)) = stack.pop() {
                    if b != bold {
                        out.push_str("**");
                    }
                    if it != italic {
                        out.push('*');
                    }
                    bold = b;
                    italic = it;
                    uc = u;
                }
                depth -= 1;
                if let Some(d) = skip_to {
                    if depth < d {
                        skip_to = None;
                    }
                }
                i += 1;
            }
            b'\\' => {
                // ① 忽略目标组（如 {\fonttbl …\'cb\'ce…}）：整段跳过。
                //    必须先于 \'hh 判断，否则被忽略组里的十六进制字节会混进正文。
                if skip_to.is_some() {
                    if i + 1 < s.len() && bytes[i + 1] == b'\'' {
                        i += 2;
                        while i < s.len() && bytes[i].is_ascii_hexdigit() {
                            i += 1;
                        }
                        continue;
                    }
                    i += 1; // 跳过反斜杠
                    if i >= s.len() {
                        break;
                    }
                    if !bytes[i].is_ascii_alphabetic() {
                        // 控制符：按 UTF-8 字符跳过，保证 i 落在字符边界
                        let len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                        i += len;
                        continue;
                    }
                    while i < s.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < s.len() && bytes[i] == b'-' {
                        i += 1;
                    }
                    while i < s.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i < s.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    continue;
                }

                // ② \'hh 十六进制字节：按字节读取，只在 ASCII 范围判断，避免 UTF-8 边界 panic
                if i + 1 < s.len() && bytes[i + 1] == b'\'' {
                    let h1 = if i + 2 < s.len() { hex_val(bytes[i + 2]) } else { None };
                    let h2 = if i + 3 < s.len() { hex_val(bytes[i + 3]) } else { None };
                    if let (Some(a), Some(b)) = (h1, h2) {
                        pending.push((a << 4) | b);
                        i += 4;
                    } else {
                        i += 2;
                    }
                    continue;
                }

                i += 1;
                if i >= s.len() {
                    break;
                }
                if bytes[i] == b'*' {
                    // \* 表示该组是要忽略的目标
                    skip_to = Some(depth);
                    i += 1;
                    continue;
                }
                if !bytes[i].is_ascii_alphabetic() {
                    match bytes[i] {
                        b'\n' | b'\r' => {}
                        b'\\' | b'{' | b'}' => {
                            flush_pending(&mut pending, &mut out, enc);
                            out.push(bytes[i] as char);
                        }
                        b'-' | b'_' | b':' | b'|' | b'~' => {}
                        _ => {}
                    }
                    i += 1;
                    continue;
                }
                // 控制字（字母均为 ASCII，切片一定落在字符边界）
                let start = i;
                while i < s.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = &s[start..i];
                let mut digits_start = i;
                let mut negative = false;
                if i < s.len() && bytes[i] == b'-' {
                    negative = true;
                    digits_start = i + 1;
                }
                while i < s.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let param: Option<i32> = if i > digits_start {
                    s[digits_start..i].parse::<i32>().ok().map(|v| if negative { -v } else { v })
                } else {
                    None
                };
                // 参数后的分隔符（空格）
                if i < s.len() && bytes[i] == b' ' {
                    i += 1;
                }

                // ③ \uN：输出 Unicode 字符，再按 \ucN 跳过替代字符；处理 UTF-16 代理对
                if word == "u" {
                    flush_pending(&mut pending, &mut out, enc);
                    if let Some(n) = param {
                        let n16 = if n < 0 { (n + 65536) as u32 } else { n as u32 };
                        if (0xD800..=0xDBFF).contains(&n16) {
                            high_surrogate = Some(n16);
                        } else if (0xDC00..=0xDFFF).contains(&n16) {
                            if let Some(hi) = high_surrogate.take() {
                                let cp = 0x10000 + (((hi - 0xD800) << 10) | (n16 - 0xDC00));
                                if let Some(ch) = char::from_u32(cp) {
                                    out.push(ch);
                                }
                            } else {
                                out.push('\u{FFFD}');
                            }
                        } else {
                            if high_surrogate.take().is_some() {
                                out.push('\u{FFFD}');
                            }
                            if let Some(ch) = char::from_u32(n16) {
                                out.push(ch);
                            }
                        }
                    }
                    skip_fallback(&mut i, s, uc);
                    continue;
                }

                if word == "*" || IGNORE_DEST.contains(&word) {
                    skip_to = Some(depth);
                    continue;
                }

                match word {
                    "uc" => {
                        if let Some(v) = param {
                            uc = v.max(0);
                        }
                    }
                    "par" | "sect" | "page" | "row" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push_str("\n\n");
                    }
                    "line" | "softline" | "nestcell" | "cell" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('\n');
                    }
                    "tab" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('\t');
                    }
                    "emdash" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('—');
                    }
                    "endash" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('–');
                    }
                    "lquote" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('“');
                    }
                    "rquote" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('”');
                    }
                    "ldblquote" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('“');
                    }
                    "rdblquote" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push('”');
                    }
                    "bullet" => {
                        flush_pending(&mut pending, &mut out, enc);
                        out.push_str("• ");
                    }
                    "b" => {
                        flush_pending(&mut pending, &mut out, enc);
                        let new = param.map(|v| v != 0).unwrap_or(true);
                        if new != bold {
                            out.push_str("**");
                        }
                        bold = new;
                    }
                    "i" => {
                        flush_pending(&mut pending, &mut out, enc);
                        let new = param.map(|v| v != 0).unwrap_or(true);
                        if new != italic {
                            out.push('*');
                        }
                        italic = new;
                    }
                    "pard" | "plain" => {
                        flush_pending(&mut pending, &mut out, enc);
                        if bold {
                            out.push_str("**");
                            bold = false;
                        }
                        if italic {
                            out.push('*');
                            italic = false;
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => {
                i += 1;
            }
            _ => {
                if skip_to.is_some() {
                    let len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    i += len;
                    continue;
                }
                flush_pending(&mut pending, &mut out, enc);
                let len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                out.push_str(&s[i..i + len]);
                i += len;
            }
        }
    }
    flush_pending(&mut pending, &mut out, enc);
    if bold {
        out.push_str("**");
    }
    if italic {
        out.push('*');
    }
    Ok(out.replace('\t', " "))
}

fn detect_encoding(s: &str) -> &'static encoding_rs::Encoding {
    if let Some(idx) = s.find("\\ansicpg") {
        let rest = &s[idx + 8..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or_else(|| rest.char_indices().nth(6).map(|(i, _)| i).unwrap_or(rest.len()));
        if let Ok(cp) = rest[..end].parse::<u32>() {
            match cp {
                936 | 10008 => return encoding_rs::GBK,
                950 => return encoding_rs::BIG5,
                932 => return encoding_rs::SHIFT_JIS,
                949 => return encoding_rs::EUC_KR,
                65001 => return encoding_rs::UTF_8,
                1251 => return encoding_rs::WINDOWS_1251,
                _ => return encoding_rs::WINDOWS_1252,
            }
        }
    }
    encoding_rs::WINDOWS_1252
}
