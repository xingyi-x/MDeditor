/* ══════════════ RTF → Markdown（够用的简化解析） ══════════════ */

const IGNORE_DEST: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "shppict", "nonshppict",
    "listtable", "listoverridetable", "revtbl", "docvar", "panose", "falt", "ffdeftext",
    "datafield", "themedata", "colorschememapping", "latentstyles", "datastore", "creatim",
    "revtim", "printim", "buptim", "protusertbl", "xe", "tc", "*",
];

pub fn convert(data: &[u8]) -> Result<String, String> {
    let raw = String::from_utf8_lossy(data);
    let enc = detect_encoding(&raw);
    let s: &str = raw.as_ref();
    let bytes = s.as_bytes();

    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    let mut stack: Vec<(bool, bool)> = Vec::new();
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
                stack.push((bold, italic));
                i += 1;
            }
            b'}' => {
                flush_pending(&mut pending, &mut out, enc);
                if let Some((b, it)) = stack.pop() {
                    if b != bold {
                        out.push_str("**");
                    }
                    if it != italic {
                        out.push('*');
                    }
                    bold = b;
                    italic = it;
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
                // \'hh 十六进制字节
                if i + 1 < s.len() && bytes[i + 1] == b'\'' && i + 3 < s.len() {
                    if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                        pending.push(v);
                        i += 4;
                        continue;
                    }
                }
                i += 1;
                // 控制符（非字母）
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
                // 控制字
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

                if skip_to.is_some() {
                    continue;
                }

                if word == "u" {
                    flush_pending(&mut pending, &mut out, enc);
                    if let Some(n) = param {
                        let n = if n < 0 { n + 65536 } else { n };
                        if let Some(ch) = char::from_u32(n as u32) {
                            out.push(ch);
                        }
                    }
                    // 跳过紧随的替代字符（通常是 ?）
                    if i < s.len() && bytes[i] == b'?' {
                        i += 1;
                    }
                    continue;
                }

                if word == "*" || IGNORE_DEST.contains(&word) {
                    skip_to = Some(depth);
                    continue;
                }

                match word {
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
                    i += 1;
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
            .unwrap_or(rest.len().min(6));
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
