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
    let styles = read_styles(&mut ar);
    let rels = read_rels(&mut ar, "xl/_rels/workbook.xml.rels");
    let sheets = read_sheets(&mut ar, &rels);

    let mut out = String::new();
    for (name, target) in sheets {
        let xml = match read_entry(&mut ar, &target) {
            Some(x) => x,
            None => continue,
        };
        let (grid, truncated) = parse_sheet(&xml, &shared, &styles);
        if grid.is_empty() {
            continue;
        }
        out.push_str(&format!("# {}\n\n", name));
        out.push_str(&super::docx::render_table(grid));
        if truncated {
            out.push_str(&format!("> … 已截断，仅显示前 {} 行\n", MAX_ROWS));
        }
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

/* ────────── 单元格样式：数字格式 → 日期判定 ──────────
 * xlsx 把日期存成「序列号」+ 单元格样式(s 属性)。只有样式引用了日期 numFmt 时，
 * 那个数字才是日期。styles.xml 里：
 *   · <numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts> 自定义格式码
 *   · <cellXfs><xf numFmtId="14"/>…</cellXfs> 每个 <xf> 的下标就是单元格的 s 值
 * workbook.xml 的 <workbookPr date1904="1"> 决定 1900 / 1904 两套日期系统。
 */

struct Styles {
    /// 自定义 numFmtId → formatCode
    num_fmts: HashMap<u32, String>,
    /// cellXfs 中每个 <xf> 的 numFmtId；下标即单元格 s 属性
    cell_xfs: Vec<u32>,
    /// 是否使用 1904 日期系统
    date1904: bool,
}

impl Default for Styles {
    fn default() -> Self {
        Styles { num_fmts: HashMap::new(), cell_xfs: Vec::new(), date1904: false }
    }
}

impl Styles {
    fn num_fmt_id(&self, xf: usize) -> u32 {
        self.cell_xfs.get(xf).copied().unwrap_or(0)
    }

    /// 该样式是否为日期/时间格式
    fn is_date_fmt(&self, id: u32) -> bool {
        // 内置日期格式（ECMA-376）：14–22 日期，45–47 时间/时分秒
        if (14..=22).contains(&id) || (45..=47).contains(&id) {
            return true;
        }
        // 自定义格式：解析 formatCode，含 y/m/d/h/s 者视为日期时间
        self.num_fmts
            .get(&id)
            .map(|code| format_code_is_date(code))
            .unwrap_or(false)
    }

    /// 把日期序列号格式化成 YYYY-MM-DD[ HH:MM[:SS]]；
    /// 非日期样式、无法解析或年份越界时返回 None（由调用方回退为原数值）。
    fn format_serial(&self, xf: usize, raw: &str) -> Option<String> {
        if !self.is_date_fmt(self.num_fmt_id(xf)) {
            return None;
        }
        let serial: f64 = raw.trim().parse().ok()?;
        if !serial.is_finite() || serial < 0.0 {
            return None;
        }
        let whole = serial.trunc();
        let frac = serial - whole;
        let mut days = whole as i64;
        let mut z = if self.date1904 {
            // 1904 系统：序列号 0 = 1904-01-01，无闰年 bug
            days_from_civil(1904, 1, 1) + days
        } else {
            // 1900 系统：Excel 把 1900 误当闰年，多出虚构的 1900-02-29；
            // 序列号 ≥60 减 1 才能对齐真实公历。
            if whole >= 60.0 {
                days -= 1;
            }
            days_from_civil(1899, 12, 31) + days
        };
        let mut secs = (frac * 86400.0).round() as i64;
        if secs >= 86400 {
            // 浮点误差把 23:59:59.999 顶到次日，顺手进位
            secs -= 86400;
            z += 1;
        }
        let (y, m, d) = civil_from_days(z);
        if !(1..=9999).contains(&y) {
            return None;
        }
        if secs > 0 {
            let hh = secs / 3600;
            let mm = (secs % 3600) / 60;
            let ss = secs % 60;
            if ss == 0 {
                // 秒为 0 时可省略到分钟
                Some(format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm))
            } else {
                Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, hh, mm, ss))
            }
        } else {
            Some(format!("{:04}-{:02}-{:02}", y, m, d))
        }
    }
}

/// 判定 formatCode 是否为日期/时间格式。
/// 先剔除引号字面量、方括号（颜色 / 条件 / 区域设置 `[$-409]`）、反斜杠转义，
/// 再看剩余格式码里是否含 y/m/d/h/s 任一字母。
/// 纯数字格式（0.00 / #,##0 / 0.00E+00 等）不含这些字母（E 不算），自然被排除。
fn format_code_is_date(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut i = 0usize;
    let mut in_quote = false;
    let mut in_bracket = false;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' => in_quote = !in_quote,
            b'[' if !in_quote => in_bracket = true,
            b']' if !in_quote => in_bracket = false,
            b'\\' if !in_quote => i += 1, // 跳过被转义的字面字符
            _ if !in_quote && !in_bracket => {
                if matches!(c.to_ascii_lowercase(), b'y' | b'm' | b'd' | b'h' | b's') {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// 公历日期 → 自 1970-01-01 的天数（Howard Hinnant civil 算法，纯整数，无依赖）
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = m as i64 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 自 1970-01-01 的天数 → 公历 (年, 月, 日)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

fn read_styles(ar: &mut Archive) -> Styles {
    let mut st = Styles::default();
    if let Some(xml) = read_entry(ar, "xl/workbook.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Empty(e)) | Ok(Event::Start(e))
                    if e.local_name().as_ref() == b"workbookPr" =>
                {
                    if let Some(v) = super::xml_attr(&e, "date1904") {
                        st.date1904 = v == "1" || v.eq_ignore_ascii_case("true");
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    if let Some(xml) = read_entry(ar, "xl/styles.xml") {
        let mut r = Reader::from_str(&xml);
        let mut buf = Vec::new();
        let mut in_cell_xfs = false;
        loop {
            match r.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) if e.local_name().as_ref() == b"cellXfs" => {
                    in_cell_xfs = true;
                }
                Ok(Event::End(e)) if e.local_name().as_ref() == b"cellXfs" => {
                    in_cell_xfs = false;
                }
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) => match e.local_name().as_ref() {
                    b"numFmt" => {
                        let id = super::xml_attr(&e, "numFmtId")
                            .and_then(|s| s.parse::<u32>().ok());
                        let code = super::xml_attr(&e, "formatCode");
                        if let (Some(id), Some(code)) = (id, code) {
                            st.num_fmts.insert(id, code);
                        }
                    }
                    // 只收 <cellXfs> 里的 <xf>；<cellStyleXfs> 里的同名元素不能算
                    b"xf" if in_cell_xfs => {
                        st.cell_xfs.push(
                            super::xml_attr(&e, "numFmtId")
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(0),
                        );
                    }
                    _ => {}
                },
                _ => {}
            }
            buf.clear();
        }
    }
    st
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

/// 非日期数值单元格的清理：整数字去掉尾部 .0，其余原样。
/// 日期序列号 → 日期由 Styles::format_serial 处理；styles 无法解析（或不是日期样式）时
/// 它返回 None，这里优雅回退为原数值，表格输出不受影响。
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

/// 返回 (表格行, 是否因 MAX_ROWS 被截断)
fn parse_sheet(xml: &str, shared: &[String], styles: &Styles) -> (Vec<Vec<String>>, bool) {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_col = 0usize;
    let mut cur_xf = 0usize;
    let mut cell_type = String::new();
    let mut cell_val = String::new();
    let mut inline = String::new();
    let mut in_v = false;
    let mut in_is = false;
    let mut in_t = false;
    let mut truncated = false;
    let mut at_limit = false;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"row" => {
                    // 已到行数上限又遇到新行：确认被截断（正好 2000 行时不会误报）
                    if at_limit {
                        truncated = true;
                        break;
                    }
                    cur = Vec::new();
                    // 按 <row r="N"> 补齐中间被省略的空行，尽量还原原始行距
                    if let Some(n) = super::xml_attr(&e, "r").and_then(|s| s.parse::<usize>().ok()) {
                        while rows.len() < n.saturating_sub(1) && rows.len() < MAX_ROWS {
                            rows.push(Vec::new());
                        }
                    }
                }
                b"c" => {
                    cell_type = super::xml_attr(&e, "t").unwrap_or_default();
                    // s = cellXfs 中的样式索引，用于判定日期格式
                    cur_xf = super::xml_attr(&e, "s")
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
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
                        at_limit = true;
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
                            // 数值单元格：先看样式是否为日期格式，是则把序列号转日期；
                            // 否则（含 styles 解析失败）回退为原来的数值清理。
                            _ => styles
                                .format_serial(cur_xf, &cell_val)
                                .unwrap_or_else(|| clean_number(&cell_val)),
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

    // 去掉尾部全空的行与列（中间按 row r 补出来的空行不会被当尾部删掉）
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
    (rows, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles_1900() -> Styles {
        let mut st = Styles::default();
        // s=0 通用格式，s=1 内置日期格式 14 (m/d/yy)
        st.cell_xfs = vec![0, 14];
        st
    }

    #[test]
    fn civil_roundtrip_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1899, 12, 31), -25568);
        assert_eq!(civil_from_days(-25568), (1899, 12, 31));
    }

    #[test]
    fn serial_1900_dates() {
        let st = styles_1900();
        assert_eq!(st.format_serial(1, "1").as_deref(), Some("1900-01-01"));
        assert_eq!(st.format_serial(1, "59").as_deref(), Some("1900-02-28"));
        // 1900 闰年 bug：≥60 减 1
        assert_eq!(st.format_serial(1, "60").as_deref(), Some("1900-02-28"));
        assert_eq!(st.format_serial(1, "61").as_deref(), Some("1900-03-01"));
        assert_eq!(st.format_serial(1, "25569").as_deref(), Some("1970-01-01"));
    }

    #[test]
    fn serial_1900_with_time() {
        let st = styles_1900();
        // 半天 = 12:00:00，秒为 0 省略到分钟
        assert_eq!(st.format_serial(1, "25569.5").as_deref(), Some("1970-01-01 12:00"));
        // 1:02:03
        let v = 25569.0 + 3723.0 / 86400.0;
        assert_eq!(
            st.format_serial(1, &v.to_string()).as_deref(),
            Some("1970-01-01 01:02:03")
        );
    }

    #[test]
    fn serial_1904() {
        let mut st = Styles::default();
        st.date1904 = true;
        st.cell_xfs = vec![14];
        assert_eq!(st.format_serial(0, "0").as_deref(), Some("1904-01-01"));
        assert_eq!(st.format_serial(0, "1").as_deref(), Some("1904-01-02"));
    }

    #[test]
    fn non_date_style_and_bad_value_fall_back() {
        let st = styles_1900();
        // s=0 是 General，非日期样式
        assert_eq!(st.format_serial(0, "45123"), None);
        // 日期样式但值不是数字（如公式结果字符串）
        assert_eq!(st.format_serial(1, "not-a-number"), None);
        // 越界年份回退
        assert_eq!(st.format_serial(1, "10000000"), None);
    }

    #[test]
    fn format_code_detection() {
        assert!(format_code_is_date("yyyy-mm-dd"));
        assert!(format_code_is_date("yyyy-mm-dd hh:mm:ss"));
        assert!(format_code_is_date("h:mm:ss"));
        assert!(format_code_is_date(r#"[$-409]d-mmm-yy"#));
        assert!(!format_code_is_date("0.00"));
        assert!(!format_code_is_date("#,##0"));
        assert!(!format_code_is_date("0.00E+00"));
        assert!(!format_code_is_date("General"));
        // 引号里的字面 days / 方括号里的 Red 不应误判
        assert!(!format_code_is_date(r#"0.00" days""#));
        assert!(!format_code_is_date(r#"0.00_);[Red]\-0.00"#));
    }

    #[test]
    fn custom_numfmt_is_date() {
        let mut st = Styles::default();
        st.num_fmts.insert(164, "yyyy-mm-dd".into());
        st.num_fmts.insert(165, "0.00".into());
        st.cell_xfs = vec![164, 165];
        assert!(st.format_serial(0, "45123").is_some());
        assert_eq!(st.format_serial(1, "45123"), None);
    }
}
