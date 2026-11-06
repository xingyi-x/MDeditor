/* ══════════════ PDF → Markdown ══════════════
 * 用 pdf-extract 拿到「每个字符的位置 + 字号」，再按行重组：
 *   · 字号明显大于正文 → 判定为标题（按倍率分 H1~H6）
 *   · 页面上下边缘区域内、跨页重复的行 → 判定为页眉页脚，丢弃
 *   · 行尾排满且不以句末标点结尾 → 与下一行合并（修复 PDF 断行）
 *   · 表格靠「列起点网格」把绝对定位的文字反推回单元格，支持无表头与跨页续接
 *   · 图片（XObject）用 lopdf 取出并解析 cm/Do 得到纵向位置，插到正确段落之间
 *
 * 坐标约定：所有 Line 的 (x0, x1, y) 都是「显示坐标系」—— y 向下为正，
 * 页面左上角为 (0, 0)，已按 /Rotate 归一化。
 */
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use pdf_extract::{output_doc, Document, MediaBox, OutputDev, OutputError, Transform};

use crate::convert::Ctx;
use super::pdf_image::*;

/* ────────── 数据结构 ────────── */

/// 一次 show_text 产生的文本片段。
/// `end` 是**最后一个字符的右边界**（x + 字宽），每个字符重新计算、不累加，
/// 所以单字误差不会累积；用它和下一段的起始 x 相减才是真实间隙。
/// （若用「最后字符的起始 x」，每个字母后面都会凭空多出半个字宽的间隙。）
#[derive(Clone)]
struct Seg {
    x: f64,
    end: f64,
    text: String,
}

#[derive(Clone)]
struct Line {
    page: u32,
    /// 基线纵坐标（显示坐标系，越小越靠上）
    y: f64,
    page_w: f64,
    page_h: f64,
    /// 最左字符横坐标
    x0: f64,
    /// 最右字符的右边界
    x1: f64,
    size: f64,
    text: String,
    segs: Vec<Seg>,
    /// 每个字符的 (横坐标, 字符)。表格列边界是绝对 x，靠它才能准确切单元格
    chars: Vec<(f64, char)>,
    /// 所在栏（0 = 单栏或左栏，1 = 右栏），双栏重排后才有意义
    col: u8,
    /// 这一行左侧有一个图形层的列表圆点（PDF 里圆点画成矢量图形，文字层拿不到）
    bullet: bool,
    /// 所在提示框编号（0 = 不在框内）；同一编号的行必须成组处理
    quote: u32,
    /// 提示框的首行 —— 通常是「注意」「说明」这类框标题
    quote_head: bool,
    /// 框里装的是代码（示例框），输出成围栏代码块而不是引用段落
    quote_code: bool,
    /// 矢量图的图题（值为图编号，0 = 不是图题）。图内文字收进折叠块，紧跟图题输出
    fig_caption: u32,
    /// 与 PDF 书签标题一致时的书签层级（0 = 不是书签标题）
    outline_level: u8,
}

struct Dev {
    page: u32,
    page_w: f64,
    page_h: f64,
    /// 页码（从 1 开始）→ /Rotate
    rotations: BTreeMap<u32, i64>,
    lines: Vec<Line>,
    cur: Option<Line>,
    last_end: f64,
    last_y: f64,
    first_char: bool,
    /// 只有显式设置 MD_PDF_TRACE 时才逐字符收集调试信息；
    /// release 版正常转换不应为每个字符构造字符串
    trace_on: bool,
    trace: Vec<String>,
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
        || ('\u{3400}'..='\u{4DBF}').contains(&c)
        || ('\u{3040}'..='\u{30FF}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
}

/// 中日韩文字与全角标点
fn is_wide(c: char) -> bool {
    is_cjk(c)
        || ('\u{3000}'..='\u{303F}').contains(&c)
        || ('\u{FF00}'..='\u{FFEF}').contains(&c)
        || ('\u{2018}'..='\u{201F}').contains(&c)
}

/// 可参与「词间距」判断的字符：字母、数字、汉字。
/// 标点（含间隔号 ·、连字符 -、破折号 —）不算，避免在它们前面插入空格。
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_cjk(c)
}

/// PDF 用户坐标 + /Rotate → 显示坐标（y 向下）
fn map_point(px: f64, py: f64, rot: i64, w: f64, h: f64) -> (f64, f64) {
    // /Rotate 是**顺时针**旋转；返回显示坐标系（原点在左上、y 向下）。
    // 90/270 时显示尺寸与 MediaBox 互换：宽 = h、高 = w。
    // 四角自检（以 90 为例）：左下→左上、左上→右上、右上→右下、右下→左下。
    match rot.rem_euclid(360) {
        90 => (py, px),
        180 => (w - px, py),
        // 旧实现写成 (w - py, h - px)，w/h 用反了 —— 实测会算出负坐标，
        // 整页文字被挤到版心外，断行判据全乱
        270 => (h - py, w - px),
        _ => (px, h - py),
    }
}

/// 旋转 90/270 时显示尺寸与 MediaBox 互换
fn display_size(rot: i64, w: f64, h: f64) -> (f64, f64) {
    match rot.rem_euclid(360) {
        90 | 270 => (h, w),
        _ => (w, h),
    }
}

impl Dev {
    fn new(rotations: BTreeMap<u32, i64>) -> Dev {
        Dev {
            page: 0,
            page_w: 595.0,
            page_h: 842.0,
            rotations,
            lines: Vec::new(),
            cur: None,
            last_end: f64::MAX,
            last_y: 0.0,
            first_char: false,
            trace_on: std::env::var("MD_PDF_TRACE").is_ok(),
            trace: Vec::new(),
        }
    }

    fn flush(&mut self) {
        if let Some(l) = self.cur.take() {
            if !l.text.trim().is_empty() {
                self.lines.push(l);
            }
        }
    }

    fn new_line(&mut self, x: f64, y: f64, size: f64) {
        self.flush();
        // 行上的页面尺寸一律存**显示**尺寸：旋转 90/270 的页，版心高宽与 MediaBox 互换。
        // 否则 in_margin（用 page_h 判断上下边缘）与双栏检测（用 page_w）都会算错。
        let rot = self.rotations.get(&self.page).copied().unwrap_or(0);
        let (dw, dh) = display_size(rot, self.page_w, self.page_h);
        self.cur = Some(Line {
            page: self.page,
            y,
            page_w: dw,
            page_h: dh,
            x0: x,
            x1: x,
            size,
            text: String::new(),
            segs: vec![Seg { x, end: x, text: String::new() }],
            chars: Vec::new(),
            col: 0,
            bullet: false,
            quote: 0,
            quote_head: false,
            quote_code: false,
            fig_caption: 0,
            outline_level: 0,
        });
    }

    fn start_seg(&mut self, x: f64) {
        if let Some(l) = self.cur.as_mut() {
            if let Some(last) = l.segs.last() {
                if last.text.trim().is_empty() {
                    return; // 上一段还是空的，别叠一个空段
                }
                if (last.x - x).abs() < 0.01 {
                    return;
                }
            }
            l.segs.push(Seg { x, end: x, text: String::new() });
        }
    }

    fn push(&mut self, s: &str) {
        if let Some(l) = self.cur.as_mut() {
            l.text.push_str(s);
            if let Some(seg) = l.segs.last_mut() {
                seg.text.push_str(s);
            }
        }
    }

    /// 字符之间该不该补一个空格。
    ///
    /// 只有两侧都是「词字符」（字母数字或汉字）才补：
    ///   · 汉字之间不补 —— CJK 字宽上报偏小会算出假间隙，逐字拆开会变成「从 核 关 机」
    ///   · 标点两侧不补 —— 否则「马歇尔·卢森堡」会变成「马歇尔 ·卢森堡」，「2021-05」会变成「2021 -05」
    /// 阈值取 0.10em：不少 PDF 生成器把词间距压到 0.15em，原来的 0.2em 会把 `3rd Edition` 粘成 `3rdEdition`。
    fn need_gap_space(&self, x: f64, tfs: f64, cur: Option<char>) -> bool {
        if self.last_end >= 1e6 || x <= self.last_end + tfs * 0.10 {
            return false;
        }
        let prev = self.cur.as_ref().and_then(|l| l.text.chars().last());
        match (prev, cur) {
            (Some(a), Some(b)) => {
                is_word_char(a) && is_word_char(b) && !(is_wide(a) && is_wide(b))
            }
            _ => false,
        }
    }
}

impl OutputDev for Dev {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.page += 1;
        self.flush();
        self.page_w = (media_box.urx - media_box.llx).abs().max(1.0);
        self.page_h = (media_box.ury - media_box.lly).abs().max(1.0);
        self.last_end = f64::MAX;
        self.last_y = 0.0;
        self.first_char = false;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.flush();
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let tfs = {
            let det = (trm.m11 * trm.m22 - trm.m12 * trm.m21).abs().sqrt();
            font_size * if det.is_finite() && det > 0.0 { det } else { 1.0 }
        };
        let rot = self.rotations.get(&self.page).copied().unwrap_or(0);
        let (x, y) = map_point(trm.m31, trm.m32, rot, self.page_w, self.page_h);
        if self.trace_on && self.trace.len() < 4000 {
            self.trace.push(format!(
                "x={:.2} y={:.2} w={:.4} tfs={:.2} end={:.2} ch={:?}",
                x, y, width, tfs, self.last_end, char
            ));
        }
        if self.cur.is_none() {
            self.new_line(x, y, tfs);
            self.first_char = true;
        }
        if self.first_char {
            let dy = (y - self.last_y).abs();
            // 同一行内不同文本块（表格的下一列、行内换字体）的基线完全相同；
            // 只要基线跳了半个字高以上，就是新的一行
            if dy > tfs * 0.55 {
                self.new_line(x, y, tfs);
            } else {
                self.start_seg(x);
                if self.need_gap_space(x, tfs, char.chars().next()) {
                    self.push(" ");
                }
            }
            self.first_char = false;
        }
        // 同一个 show_text 内部的字符是连续排版的，词间距只会由 TJ 位移或空格字形体现 ——
        // 那里已经体现在坐标/字符里了。用累加字宽去"补"只会把 Ahimsa 拆成 A h im sa
        // （CJK 字体里的拉丁字形宽度上报经常偏小 0.1em 以上）。
        self.push(char);
        self.last_y = y;
        self.last_end = x + width * tfs;
        if let Some(l) = self.cur.as_mut() {
            for c in char.chars() {
                l.chars.push((x, c));
            }
            if x < l.x0 {
                l.x0 = x;
            }
            let right = x + width * tfs;
            if right > l.x1 {
                l.x1 = right;
            }
            if tfs > l.size {
                l.size = tfs;
            }
            if let Some(s) = l.segs.last_mut() {
                s.end = self.last_end;
            }
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.first_char = true;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

/* ────────── 页面元信息：/Rotate 与图片位置 ────────── */

pub(super) fn obj_num(o: &lopdf::Object) -> Option<f64> {
    match o {
        lopdf::Object::Real(r) => Some(*r as f64),
        lopdf::Object::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

pub(super) fn resolve<'a>(doc: &'a lopdf::Document, o: &'a lopdf::Object) -> &'a lopdf::Object {
    match o {
        lopdf::Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        _ => o,
    }
}

/// 页面几何：宽、高、旋转角（MediaBox / Rotate 会沿 Parent 链继承）
fn page_geom(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> (f64, f64, i64) {
    let (mut w, mut h, mut rot) = (0.0, 0.0, 0i64);
    let (mut has_box, mut has_rot) = (false, false);
    let mut cur = Some(page_id);
    let mut depth = 0;
    while let Some(id) = cur {
        if depth > 8 {
            break;
        }
        depth += 1;
        let Ok(d) = doc.get_dictionary(id) else { break };
        if !has_box {
            if let Ok(o) = d.get(b"MediaBox") {
                let o = resolve(doc, o);
                if let Ok(a) = o.as_array() {
                    let v: Vec<f64> = a.iter().filter_map(|x| obj_num(resolve(doc, x))).collect();
                    if v.len() == 4 {
                        w = (v[2] - v[0]).abs();
                        h = (v[3] - v[1]).abs();
                        has_box = true;
                    }
                }
            }
        }
        if !has_rot {
            if let Ok(o) = d.get(b"Rotate") {
                if let Some(v) = obj_num(resolve(doc, o)) {
                    rot = v as i64;
                    has_rot = true;
                }
            }
        }
        if has_box && has_rot {
            break;
        }
        cur = d.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    if w <= 1.0 {
        w = 595.0;
    }
    if h <= 1.0 {
        h = 842.0;
    }
    (w, h, rot)
}

/* ────────── 书签（大纲）与文档信息 ────────── */

fn dict_of<'a>(doc: &'a lopdf::Document, o: &'a lopdf::Object) -> Option<&'a lopdf::Dictionary> {
    resolve(doc, o).as_dict().ok()
}

/// PDFDocEncoding 里 0x80..=0xA0 这一段与 Latin-1 不同，其余字节按 Latin-1 解释
fn pdfdoc_char(c: u8) -> char {
    const HIGH: [char; 33] = [
        '\u{2022}', '\u{2020}', '\u{2021}', '\u{2026}', '\u{2014}', '\u{2013}', '\u{0192}',
        '\u{2044}', '\u{2039}', '\u{203A}', '\u{2212}', '\u{2030}', '\u{201E}', '\u{201C}',
        '\u{201D}', '\u{2018}', '\u{2019}', '\u{201A}', '\u{2122}', '\u{FB01}', '\u{FB02}',
        '\u{0141}', '\u{0152}', '\u{0160}', '\u{0178}', '\u{017D}', '\u{0131}', '\u{0142}',
        '\u{0153}', '\u{0161}', '\u{017E}', '\u{FFFD}', '\u{20AC}',
    ];
    if (0x80..=0xA0).contains(&c) {
        HIGH[(c - 0x80) as usize]
    } else {
        c as char
    }
}

/// PDF 文本字符串（书签标题、文档信息）：UTF-16（带 BOM）/ UTF-8（带 BOM）/ PDFDocEncoding
fn decode_pdf_text(b: &[u8]) -> String {
    if b.len() >= 2 && (b[0] == 0xFE && b[1] == 0xFF || b[0] == 0xFF && b[1] == 0xFE) {
        let be = b[0] == 0xFE;
        let units: Vec<u16> = b[2..]
            .chunks(2)
            .map(|c| {
                let pair = [c[0], c.get(1).copied().unwrap_or(0)];
                if be {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if b.len() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
        return String::from_utf8_lossy(&b[3..]).into_owned();
    }
    b.iter().map(|&c| pdfdoc_char(c)).collect()
}

/// 空白（含换行、制表符）压缩成单个空格
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 命名目标：`/Root /Dests`（PDF 1.1 字典）与 `/Root /Names /Dests`（名字树）合在一起
fn named_dests<'a>(
    doc: &'a lopdf::Document,
    root: &'a lopdf::Dictionary,
) -> HashMap<Vec<u8>, &'a lopdf::Object> {
    let mut map: HashMap<Vec<u8>, &'a lopdf::Object> = HashMap::new();
    if let Some(d) = root.get(b"Dests").ok().and_then(|o| dict_of(doc, o)) {
        for (k, v) in d.iter() {
            map.insert(k.clone(), v);
        }
    }
    let tree = root
        .get(b"Names")
        .ok()
        .and_then(|o| dict_of(doc, o))
        .and_then(|n| n.get(b"Dests").ok())
        .and_then(|o| dict_of(doc, o));
    let mut stack: Vec<&'a lopdf::Dictionary> = tree.into_iter().collect();
    let mut visited = 0usize;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > 100_000 {
            break;
        }
        if let Ok(arr) = node.get(b"Names").and_then(|o| resolve(doc, o).as_array()) {
            for pair in arr.chunks(2) {
                if let [k, v] = pair {
                    if let lopdf::Object::String(key, _) = resolve(doc, k) {
                        map.insert(key.clone(), v);
                    }
                }
            }
        }
        if let Ok(kids) = node.get(b"Kids").and_then(|o| resolve(doc, o).as_array()) {
            for kid in kids {
                if let Some(d) = dict_of(doc, kid) {
                    stack.push(d);
                }
            }
        }
    }
    map
}

/// 书签目标 → 页码（1 起）。支持显式目标数组、命名目标、带 /D 的字典
fn dest_page(
    doc: &lopdf::Document,
    d: &lopdf::Object,
    names: &HashMap<Vec<u8>, &lopdf::Object>,
    page_no: &HashMap<lopdf::ObjectId, u32>,
    depth: u8,
) -> Option<u32> {
    if depth > 4 {
        return None;
    }
    match resolve(doc, d) {
        lopdf::Object::Array(a) => match a.first()? {
            lopdf::Object::Reference(id) => page_no.get(id).copied(),
            lopdf::Object::Integer(n) => u32::try_from(*n).ok().map(|n| n + 1),
            _ => None,
        },
        lopdf::Object::Name(n) | lopdf::Object::String(n, _) => {
            let target: &lopdf::Object = names.get(n.as_slice())?;
            dest_page(doc, target, names, page_no, depth + 1)
        }
        lopdf::Object::Dictionary(dd) => {
            let inner = dd.get(b"D").ok()?;
            dest_page(doc, inner, names, page_no, depth + 1)
        }
        _ => None,
    }
}

/// 读取 PDF 书签：[(层级，1 起；标题；目标页，1 起，解析不出为 0)]，按文档顺序
fn read_outline(doc: &lopdf::Document) -> Vec<(u8, String, u32)> {
    let mut out: Vec<(u8, String, u32)> = Vec::new();
    let page_no: HashMap<lopdf::ObjectId, u32> =
        doc.get_pages().into_iter().map(|(n, id)| (id, n)).collect();
    let Some(root) = doc.trailer.get(b"Root").ok().and_then(|o| dict_of(doc, o)) else {
        return out;
    };
    let Some(outlines) = root.get(b"Outlines").ok().and_then(|o| dict_of(doc, o)) else {
        return out;
    };
    let Ok(first) = outlines.get(b"First").and_then(|o| o.as_reference()) else {
        return out;
    };
    let names = named_dests(doc, root);
    // 深度优先：先压兄弟再压孩子，出栈顺序就是文档顺序
    let mut stack: Vec<(lopdf::ObjectId, u8)> = vec![(first, 1)];
    let mut seen: HashSet<lopdf::ObjectId> = HashSet::new();
    while let Some((id, level)) = stack.pop() {
        if out.len() >= 50_000 || !seen.insert(id) {
            continue;
        }
        let Ok(item) = doc.get_dictionary(id) else { continue };
        if let Ok(next) = item.get(b"Next").and_then(|o| o.as_reference()) {
            stack.push((next, level));
        }
        if level < 32 {
            if let Ok(child) = item.get(b"First").and_then(|o| o.as_reference()) {
                stack.push((child, level + 1));
            }
        }
        let title = match item.get(b"Title").map(|o| resolve(doc, o)) {
            Ok(lopdf::Object::String(b, _)) => collapse_ws(&decode_pdf_text(b)),
            _ => String::new(),
        };
        if title.is_empty() {
            continue;
        }
        let dest = item.get(b"Dest").ok().or_else(|| {
            item.get(b"A")
                .ok()
                .and_then(|a| dict_of(doc, a))
                .and_then(|a| a.get(b"D").ok())
        });
        let page = dest
            .and_then(|d| dest_page(doc, d, &names, &page_no, 0))
            .unwrap_or(0);
        out.push((level, title, page));
    }
    out
}

/// 文档信息字典里的标题（`/Info /Title`）
fn info_title(doc: &lopdf::Document) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok().and_then(|o| dict_of(doc, o))?;
    match resolve(doc, info.get(b"Title").ok()?) {
        lopdf::Object::String(b, _) => {
            let t = collapse_ws(&decode_pdf_text(b));
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        _ => None,
    }
}

#[derive(Default)]
/// 引用 / 提示框的显示坐标矩形（左上角 + 右下角，y 向下）
#[derive(Clone, Copy)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

#[derive(Default)]
pub(super) struct PageMeta {
    rotations: BTreeMap<u32, i64>,
    /// 页 → [(显示坐标 y, XObject 名)]
    pub(super) draws: HashMap<u32, Vec<(f64, String)>>,
    /// 页 → 列表圆点中心 [(x, y)]
    bullets: HashMap<u32, Vec<(f64, f64)>>,
    /// 页 → 引用框（提示框）矩形
    boxes: HashMap<u32, Vec<Rect>>,
    /// 文档是否带结构标签。带标签时只认标签，避免几何猜测误伤
    tagged: bool,
    /// 页 → 所有描边 / 填充路径的显示坐标包围盒。矢量图（框图、示意图）只存在于图形层，
    /// 靠它们才能圈出图的范围，把图内的碎片文字与正文分开
    graphics: HashMap<u32, Vec<Rect>>,
    /// PDF 书签：(层级，1 起；标题；目标页，1 起)。用来确定标题层级
    pub(super) outline: Vec<(u8, String, u32)>,
}

/// 扫描每页 content stream 的 `q / Q / cm / Do / 路径 / 结构标记`。
/// 不做完整的内容流解释，只跟踪图形状态栈、当前变换矩阵、填充色与标签栈。
fn scan_pages(doc: &lopdf::Document) -> PageMeta {
    let mut meta = PageMeta::default();
    // 第一遍：抓出每页的图片位置与所有填充图形。
    // 「是否带结构标签」必须全量判断完才能决定要不要走几何兜底，
    // 否则前面几页没有 /Lbl 时会先被兜底逻辑污染。
    let mut raw: HashMap<u32, (Vec<PathMark>, f64)> = HashMap::new();
    for (pno, pid) in doc.get_pages() {
        let (w, h, rot) = page_geom(doc, pid);
        meta.rotations.insert(pno, rot);
        let Ok(content) = doc.get_page_content(pid) else { continue };
        let (draws, paths, graphics) = scan_ops(&content, w, h, rot);
        if paths.iter().any(|p| p.lbl || p.quote) {
            meta.tagged = true;
        }
        if !draws.is_empty() {
            meta.draws.insert(pno, draws);
        }
        if !graphics.is_empty() {
            meta.graphics.insert(pno, graphics);
        }
        raw.insert(pno, (paths, w));
    }

    // 第二遍：按语义归类
    for (pno, (paths, w)) in raw {
        let mut bullets: Vec<(f64, f64)> = Vec::new();
        let mut boxes: Vec<Rect> = Vec::new();
        for p in &paths {
            let (bw, bh) = (p.x1 - p.x0, p.y1 - p.y0);
            let (r, g, bl) = p.color;
            let dark = (r + g + bl) / 3.0 < 0.88;
            let sat = r.max(g).max(bl) - r.min(g).min(bl);
            // 列表圆点：/Lbl 标记的实心圆；未带标签的文档退化为「小方块/小圆」几何判断。
            //   · 尺寸在「一个字上下」量级 —— 太大会是图标，太小是表格线
            //   · 兜底还要求接近正方形、且不是浅色底纹，避免把细线或底色当圆点
            let small = bw <= 22.0 && bh <= 22.0 && bw >= 1.0 && bh >= 1.0;
            let round = (bw - bh).abs() <= 0.6 * bw.max(bh);
            if small && (p.lbl || (!meta.tagged && round && dark)) {
                bullets.push(((p.x0 + p.x1) * 0.5, (p.y0 + p.y1) * 0.5));
            }
            // 提示框：/BlockQuote 包裹、且大到能装下一行以上文字的底色块
            if p.quote && bw >= w * 0.15 && bh >= 18.0 {
                boxes.push(Rect { x0: p.x0, y0: p.y0, x1: p.x1, y1: p.y1 });
            } else if !meta.tagged
                && bw <= w * 0.03
                && bh >= 30.0
                && sat >= 0.12
                && bh > bw * 6.0
            {
                // 无标签文档：「左侧一条竖色条」是提示框最常见的画法，
                // 右侧边界取到版面右缘，纵向范围由色条本身界定。
                boxes.push(Rect { x0: p.x0, y0: p.y0, x1: w, y1: p.y1 });
            }
        }
        // 同一页可能有多层嵌套底色（底色 + 内边距），只保留最大的那个
        boxes.sort_by(|a, b| {
            let sa = (a.x1 - a.x0) * (a.y1 - a.y0);
            let sb = (b.x1 - b.x0) * (b.y1 - b.y0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut kept: Vec<Rect> = Vec::new();
        for b in boxes {
            let inside = kept.iter().any(|k| {
                b.x0 >= k.x0 - 2.0 && b.x1 <= k.x1 + 2.0 && b.y0 >= k.y0 - 2.0 && b.y1 <= k.y1 + 2.0
            });
            if !inside {
                kept.push(b);
            }
        }
        if !bullets.is_empty() {
            meta.bullets.insert(pno, bullets);
        }
        if !kept.is_empty() {
            meta.boxes.insert(pno, kept);
        }
    }
    meta
}

#[derive(Clone, Copy)]
struct Mat([f64; 6]);

impl Mat {
    fn id() -> Mat {
        Mat([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }
    /// self ∘ m（先应用 m，再应用 self）
    fn mul(&self, m: &Mat) -> Mat {
        let a = self.0;
        let b = m.0;
        Mat([
            a[0] * b[0] + a[2] * b[1],
            a[1] * b[0] + a[3] * b[1],
            a[0] * b[2] + a[2] * b[3],
            a[1] * b[2] + a[3] * b[3],
            a[0] * b[4] + a[2] * b[5] + a[4],
            a[1] * b[4] + a[3] * b[5] + a[5],
        ])
    }
}

/// 一处**填充图形**的显示坐标包围盒。
///
/// 列表的小圆点、提示框的底色与左侧色条都画在图形层，pdf-extract 完全看不到，
/// 只能自己扫内容流拿回来。结构标签（`/Lbl`、`/BlockQuote`）能直接告诉我们
/// 这块图形是什么语义，比纯几何猜测可靠得多。
#[derive(Clone)]
struct PathMark {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    color: (f64, f64, f64),
    /// 被 `/Lbl`（列表标签）包裹
    lbl: bool,
    /// 被 `/BlockQuote`（提示框）包裹
    quote: bool,
}

/// 用户空间矩形经 CTM + 页面旋转后的显示坐标包围盒（y 向下）
fn dev_rect(
    m: &Mat,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    rot: i64,
    pw: f64,
    ph: f64,
) -> (f64, f64, f64, f64) {
    let pts = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    let mut dx0 = f64::MAX;
    let mut dy0 = f64::MAX;
    let mut dx1 = f64::MIN;
    let mut dy1 = f64::MIN;
    for (px, py) in pts {
        let ux = m.0[0] * px + m.0[2] * py + m.0[4];
        let uy = m.0[1] * px + m.0[3] * py + m.0[5];
        let (dx, dy) = map_point(ux, uy, rot, pw, ph);
        dx0 = dx0.min(dx);
        dx1 = dx1.max(dx);
        dy0 = dy0.min(dy);
        dy1 = dy1.max(dy);
    }
    (dx0, dy0, dx1, dy1)
}

fn bbox_add(b: &mut Option<(f64, f64, f64, f64)>, x: f64, y: f64) {
    match b {
        Some(e) => {
            e.0 = e.0.min(x);
            e.1 = e.1.min(y);
            e.2 = e.2.max(x);
            e.3 = e.3.max(y);
        }
        None => *b = Some((x, y, x, y)),
    }
}

/// 极简内容流 tokenizer：识别数字、名字、操作符、路径绘制与结构标记，
/// 字符串与内联图整体跳过。文本层不在这里处理（交给 pdf-extract）。
fn scan_ops(
    content: &[u8],
    page_w: f64,
    page_h: f64,
    rot: i64,
) -> (Vec<(f64, String)>, Vec<PathMark>, Vec<Rect>) {
    let mut out: Vec<(f64, String)> = Vec::new();
    let mut paths: Vec<PathMark> = Vec::new();
    // 描边与填充一视同仁的包围盒（含零宽的直线），只用于圈定矢量图范围
    let mut graphics: Vec<Rect> = Vec::new();
    let mut stack: Vec<Mat> = Vec::new();
    let mut ctm = Mat::id();
    let mut nums: Vec<f64> = Vec::new();
    let mut last_name: Option<String> = None;
    // `<<` 之前的那个名字才是 BDC 的标签名（`/Lbl <</MCID 0>> BDC`），
    // 字典里的 /MCID 会把 last_name 覆盖掉，所以必须提前快照。
    let mut tag_pending: Option<String> = None;    let mut tags: Vec<String> = Vec::new();
    let mut pbox: Option<(f64, f64, f64, f64)> = None;
    let mut fill = (0.0f64, 0.0f64, 0.0f64);
    let mut i = 0usize;
    let n = content.len();

    while i < n {
        let b = content[i];
        match b {
            b'%' => {
                while i < n && content[i] != b'\n' && content[i] != b'\r' {
                    i += 1;
                }
            }
            b'(' => {
                let mut depth = 1;
                i += 1;
                while i < n && depth > 0 {
                    match content[i] {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                nums.push(0.0); // 占位：字符串不参与矩阵运算
            }
            b'<' => {
                if i + 1 < n && content[i + 1] == b'<' {
                    tag_pending = last_name.clone();
                    i += 2;
                } else {
                    while i < n && content[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                }
                nums.push(0.0);
            }
            b'[' | b']' | b'{' | b'}' => {
                nums.push(0.0);
                i += 1;
            }
            b'/' => {
                i += 1;
                let s = i;
                while i < n && !is_delim(content[i]) {
                    i += 1;
                }
                last_name = Some(String::from_utf8_lossy(&content[s..i]).into_owned());
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let s = i;
                i += 1;
                while i < n
                    && matches!(content[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    // 只有紧跟在指数标志后的正负号才属于数字
                    if matches!(content[i], b'+' | b'-')
                        && !matches!(content[i - 1], b'e' | b'E')
                    {
                        break;
                    }
                    i += 1;
                }
                let txt = String::from_utf8_lossy(&content[s..i]).into_owned();
                nums.push(txt.parse::<f64>().unwrap_or(0.0));
            }
            _ if b.is_ascii_alphabetic() => {
                let s = i;
                // 注意：这里必须看 content[i]，不能用外层已经固定的 b ——
                // 否则 `b.is_ascii_alphanumeric()` 恒为真，第一个操作符就会把整条流吞掉
                while i < n
                    && (content[i].is_ascii_alphanumeric()
                        || content[i] == b'*'
                        || content[i] == b'\'')
                {
                    i += 1;
                }
                let op = String::from_utf8_lossy(&content[s..i]).into_owned();
                match op.as_str() {
                    "q" => {
                        stack.push(ctm);
                        nums.clear();
                    }
                    "Q" => {
                        if let Some(m) = stack.pop() {
                            ctm = m;
                        }
                        nums.clear();
                    }
                    "cm" => {
                        if nums.len() >= 6 {
                            let k = nums.len() - 6;
                            let m = Mat([
                                nums[k],
                                nums[k + 1],
                                nums[k + 2],
                                nums[k + 3],
                                nums[k + 4],
                                nums[k + 5],
                            ]);
                            ctm = ctm.mul(&m);
                        }
                        nums.clear();
                    }
                    "Do" => {
                        if let Some(name) = last_name.take() {
                            // 单位正方形经 ctm 变换后的纵向范围 [f, f + d]
                            let top = ctm.0[5] + ctm.0[3];
                            let (_, y) = map_point(0.0, top, rot, page_w, page_h);
                            out.push((y, name));
                        }
                        nums.clear();
                    }
                    /* ── 结构标记：BDC/EMC 维护标签栈 ── */
                    "BDC" => {
                        tags.push(tag_pending.take().unwrap_or_default());
                        nums.clear();
                    }
                    "BMC" => {
                        let t = tag_pending.take().or_else(|| last_name.clone());
                        tags.push(t.unwrap_or_default());
                        nums.clear();
                    }
                    "EMC" => {
                        tags.pop();
                        nums.clear();
                    }
                    /* ── 路径构造：累积用户空间包围盒 ── */
                    "m" | "l" => {
                        if nums.len() >= 2 {
                            let k = nums.len() - 2;
                            bbox_add(&mut pbox, nums[k], nums[k + 1]);
                        }
                        nums.clear();
                    }
                    "c" => {
                        if nums.len() >= 6 {
                            let k = nums.len() - 6;
                            for t in 0..3 {
                                bbox_add(&mut pbox, nums[k + t * 2], nums[k + t * 2 + 1]);
                            }
                        }
                        nums.clear();
                    }
                    "v" | "y" => {
                        if nums.len() >= 4 {
                            let k = nums.len() - 4;
                            for t in 0..2 {
                                bbox_add(&mut pbox, nums[k + t * 2], nums[k + t * 2 + 1]);
                            }
                        }
                        nums.clear();
                    }
                    "re" => {
                        if nums.len() >= 4 {
                            let k = nums.len() - 4;
                            let (x, y, w, h) = (nums[k], nums[k + 1], nums[k + 2], nums[k + 3]);
                            bbox_add(&mut pbox, x, y);
                            bbox_add(&mut pbox, x + w, y + h);
                        }
                        nums.clear();
                    }
                    /* ── 填充：产出一个图形标记 ── */
                    "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                        if let Some((px0, py0, px1, py1)) = pbox.take() {
                            let (dx0, dy0, dx1, dy1) =
                                dev_rect(&ctm, px0, py0, px1, py1, rot, page_w, page_h);
                            if dx1 - dx0 > 0.05 && dy1 - dy0 > 0.05 {
                                paths.push(PathMark {
                                    x0: dx0,
                                    y0: dy0,
                                    x1: dx1,
                                    y1: dy1,
                                    color: fill,
                                    lbl: tags.iter().any(|t| t == "Lbl"),
                                    quote: tags.iter().any(|t| t == "BlockQuote"),
                                });
                            }
                            if (dx1 - dx0).max(dy1 - dy0) > 0.5 {
                                graphics.push(Rect { x0: dx0, y0: dy0, x1: dx1, y1: dy1 });
                            }
                        }
                        nums.clear();
                    }
                    /* ── 描边：只记进 graphics（框图的连线、外框常是描边画的） ── */
                    "S" | "s" => {
                        if let Some((px0, py0, px1, py1)) = pbox.take() {
                            let (dx0, dy0, dx1, dy1) =
                                dev_rect(&ctm, px0, py0, px1, py1, rot, page_w, page_h);
                            if (dx1 - dx0).max(dy1 - dy0) > 0.5 {
                                graphics.push(Rect { x0: dx0, y0: dy0, x1: dx1, y1: dy1 });
                            }
                        }
                        nums.clear();
                    }
                    "n" | "N" => {
                        pbox = None;
                        nums.clear();
                    }
                    "h" => nums.clear(),
                    /* ── 填充色 ── */
                    "g" => {
                        if let Some(v) = nums.last() {
                            fill = (*v, *v, *v);
                        }
                        nums.clear();
                    }
                    "rg" => {
                        if nums.len() >= 3 {
                            let k = nums.len() - 3;
                            fill = (nums[k], nums[k + 1], nums[k + 2]);
                        }
                        nums.clear();
                    }
                    "k" => {
                        if nums.len() >= 4 {
                            let k = nums.len() - 4;
                            let (c, m, y, kk) = (nums[k], nums[k + 1], nums[k + 2], nums[k + 3]);
                            let f = |c: f64, m: f64| (1.0 - c) * (1.0 - m);
                            fill = (f(c, kk), f(m, kk), f(y, kk));
                        }
                        nums.clear();
                    }
                    "ID" => {
                        // 内联图：跳过二进制数据直到 EI
                        i += 1;
                        while i + 1 < n {
                            if content[i] == b'E'
                                && content[i + 1] == b'I'
                                && (i == 0 || is_ws(content[i - 1]))
                            {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                        nums.clear();
                        last_name = None;
                    }
                    _ => nums.clear(),
                }
            }
            _ => i += 1,
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    (out, paths, graphics)
}

pub(super) fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

fn is_delim(b: u8) -> bool {
    is_ws(b) || matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/* ────────── 主入口 ────────── */

/* 全局 panic hook 只安装一次，用 thread-local 标志控制「本次转换是否静音」。
 * 早期实现每次转换都 take_hook/set_hook，属于进程级全局状态：并发转换时
 * 一个线程会把另一个线程的 panic 一起吞掉，两次转换交错还会让 hook 永久静音。
 * 现在只有当前线程在转换期间静音，其它线程的 panic 照常向 stderr 报告。 */
static PANIC_HOOK_ONCE: std::sync::Once = std::sync::Once::new();
thread_local! {
    static QUIET_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn install_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let quiet = QUIET_PANIC.try_with(|c| c.get()).unwrap_or(false);
            if !quiet {
                prev(info);
            }
        }));
    });
}

pub fn convert(data: &[u8], ctx: &Ctx) -> Result<String, String> {
    let mut doc = Document::load_mem(data).map_err(|e| format!("PDF 解析失败：{}", e))?;
    if doc.is_encrypted() {
        let _ = doc.decrypt("");
    }

    // lopdf 只用来拿 /Rotate、图片位置与图片本体。
    // 加密 PDF 也必须解密，否则读到的流仍是密文，图片会静默缺失。
    let mut ldoc = lopdf::Document::load_mem(data).ok();
    if let Some(d) = ldoc.as_mut() {
        if d.is_encrypted() {
            let _ = d.decrypt("");
        }
    }
    let mut meta = ldoc.as_ref().map(scan_pages).unwrap_or_default();
    let total_pages = ldoc.as_ref().map(|d| d.get_pages().len()).unwrap_or(0);
    if let Some(d) = ldoc.as_ref() {
        meta.outline = read_outline(d);
        if let Some(t) = info_title(d) {
            crate::convert::note_meta("pdf_title", t);
        }
        if total_pages > 0 {
            crate::convert::note_meta("pages", total_pages.to_string());
        }
    }

    let mut dev = Dev::new(meta.rotations.clone());
    /* pdf-extract 内部有多处 expect（`missing object reference`、`wrong type`、
     * 以及 `Do` 的名字不在 /Resources 里时直接 expect(key)），遇到畸形 PDF 会 panic。
     * 这些都不是我们能把控的，但**不能让一张坏页把整个转换进程带崩**：
     * 包一层 catch_unwind，panic 后保留已经解析出来的内容，至少还能出半成品。
     * 注意：这条路径要求 release 不开 panic=abort，否则 catch_unwind 无效（进程直接 abort）。 */
    install_panic_hook();
    QUIET_PANIC.with(|c| c.set(true));
    let extracted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        output_doc(&doc, &mut dev)
    }));
    QUIET_PANIC.with(|c| c.set(false));

    match extracted {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("PDF 文本提取失败：{}", e)),
        Err(_) => {
            // 上游 panic：已解析的页仍然保留，但必须告诉调用方结果不完整
            crate::convert::warn(if total_pages > 0 {
                format!(
                    "PDF 解析在第 {} 页附近中断（共 {} 页），之后的内容缺失",
                    dev.page, total_pages
                )
            } else {
                format!("PDF 解析在第 {} 页附近中断，之后的内容缺失", dev.page)
            });
        }
    }
    dev.flush();
    if dev.lines.is_empty() {
        crate::convert::warn("没有提取到任何文字：可能是扫描版（图片型）PDF，需要先做 OCR");
    }

    if let Ok(p) = std::env::var("MD_PDF_TRACE") {
        let only: Option<u32> = std::env::var("MD_PDF_TRACE_PAGE")
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let mut dump = dev.trace.join("\n");
        dump.push_str("\n\n===== SEGS =====\n");
        for l in dev
            .lines
            .iter()
            .filter(|l| only.map_or(true, |p| l.page == p))
            .take(400)
        {
            dump.push_str(&format!(
                "--- line p{} y={:.1} size={:.2} x0={:.1} x1={:.1} text={:?}\n",
                l.page, l.y, l.size, l.x0, l.x1, l.text
            ));
            for s in l.segs.iter() {
                dump.push_str(&format!("    seg x={:.2} end={:.2} text={:?}\n", s.x, s.end, s.text));
            }
            dump.push_str("    -- 逻辑段 --\n");
            let mut prev_x = f64::MIN;
            for (x, t) in segments_of(l) {
                dump.push_str(&format!("    L x={:.2} dx={:.2} {:?}\n", x, x - prev_x, t));
                prev_x = x;
            }
        }
        let _ = fs::write(p, dump);
    }

    let images = if ctx.opts.images {
        match (&ldoc, &meta) {
            (Some(d), m) => extract_images(d, m, &ctx.assets_dir, &ctx.assets_rel),
            _ => HashMap::new(),
        }
    } else {
        HashMap::new()
    };
    Ok(lines_to_md(&mut dev.lines, &images, &meta))
}

/* ────────── 分段 ────────── */

/// 把一行里的文本块合并成「逻辑段」。
///
/// 关键：间隙用**下一段的起始 x 减去上一段最后一个字符的真实 x**，
/// 而不是「起始 x 减去累加字宽得到的右边界」—— 字体上报的字宽偏大时后者会算出**负间隙**，
/// 把表格中相邻两列粘成一列（实测过 gap = -4.26）。
fn segments_of(l: &Line) -> Vec<(f64, String)> {
    let em = l.size.max(1.0);
    let mut out: Vec<(f64, String)> = Vec::new();
    let mut cur_end = f64::MIN;
    // 上一片段末尾带空白字形。片段要 trim 之后再拼，不单独记住就会丢掉这个空格：
    // `"microcontrollers "` + `"based on"` 会粘成 `microcontrollersbased on`
    let mut prev_ws_tail = false;

    for s in &l.segs {
        let raw = s.text.as_str();
        let piece = raw.trim();
        if piece.is_empty() {
            if !raw.is_empty() {
                prev_ws_tail = true;
            }
            continue;
        }
        // 文本片段自带前导空白 = PDF 在这里画了一个空格字形，拼回去时必须保留，
        // 只靠坐标间隙会被字宽误差吃掉（0.15em 的词间距会算成 0.46em 的"紧邻"）
        let ws_head = raw.starts_with(char::is_whitespace);
        let ws_tail = raw.ends_with(char::is_whitespace);
        if let Some(last) = out.last_mut() {
            let gap = s.x - cur_end;
            // 汉字之间被拉开是两端对齐的副产物，不是列边界
            let wide_pair = matches!(
                (last.1.chars().last(), piece.chars().next()),
                (Some(a), Some(b)) if is_wide(a) && is_wide(b)
            );
            let limit = if wide_pair { em * 2.6 } else { em * 1.5 };
            if gap <= limit {
                if !wide_pair
                    && !last.1.ends_with(' ')
                    && (ws_head || prev_ws_tail || gap > em * 0.55)
                {
                    last.1.push(' ');
                }
                last.1.push_str(piece);
                cur_end = s.end;
                prev_ws_tail = ws_tail;
                continue;
            }
        }
        out.push((s.x, piece.to_string()));
        cur_end = s.end;
        prev_ws_tail = ws_tail;
    }
    out
}

/// 一行 → 文本：逻辑段之间用一个空格隔开，段内保留原有空格
fn line_text(l: &Line) -> String {
    let segs = segments_of(l);
    if segs.is_empty() {
        return squeeze_cjk_spaces(l.text.trim());
    }
    let parts: Vec<&str> = segs
        .iter()
        .map(|(_, t)| t.as_str())
        .filter(|t| !t.is_empty())
        .collect();
    squeeze_cjk_spaces(&parts.join(" "))
}

/// 汉字/中文标点之间不该有空格。
/// 分散对齐（两端对齐）的 PDF —— 目录最典型 —— 会把每个汉字单独定位，
/// 不还原就变成「图 4-2 从 核 关 机」。
fn squeeze_cjk_spaces(s: &str) -> String {
    if !s.contains(' ') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == ' ' {
            let mut next_c = None;
            while let Some(&p) = it.peek() {
                if p == ' ' {
                    it.next();
                } else {
                    next_c = Some(p);
                    break;
                }
            }
            if let (Some(a), Some(b)) = (out.chars().last(), next_c) {
                if is_wide(a) && is_wide(b) {
                    continue;
                }
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// 这一行里有没有「列级」的横向跳变（表格的下一列、多栏排版）。
/// 阈值 3 个字宽：中文两端对齐会把字距拉到 1.5~2 字，真正的列间距在 3 字以上。
fn has_column_jump(l: &Line) -> bool {
    let em = l.size.max(1.0);
    let mut prev = f64::MIN;
    let mut seen = 0usize;
    for (x, _) in &l.chars {
        if prev > f64::MIN && x - prev > em * 3.0 && seen >= 2 {
            return true;
        }
        prev = *x;
        seen += 1;
    }
    false
}

/// 把一行拆成表格单元格（逻辑段数在 2~6 之间才算候选）
fn table_cells(l: &Line) -> Option<Vec<(f64, String)>> {
    // 图题自成一段，不能被当成表格行吞进相邻的表格
    if l.fig_caption != 0 {
        return None;
    }
    let segs = segments_of(l);
    if segs.len() < 2 || segs.len() > 8 {
        return None;
    }
    let mut cells = Vec::new();
    for (x, text) in segs {
        let t = text.trim().to_string();
        if t.is_empty() || t.chars().count() > 120 {
            return None;
        }
        cells.push((x, t.replace('|', "\\|")));
    }
    Some(cells)
}

/// 两行的列位置是否对齐（容差 4pt）
fn same_columns(a: &[(f64, String)], b: &[(f64, String)]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|((x1, _), (x2, _))| (x1 - x2).abs() < 4.0)
}

/// 把列起点收进网格（4pt 内视作同一列）
fn push_col(grid: &mut Vec<f64>, x: f64) {
    if !grid.iter().any(|g| (g - x).abs() < 4.0) {
        grid.push(x);
    }
}

/// 按列网格切出单元格。
/// 表格的列边界是绝对 x 坐标，与「字符间隙」无关 —— 右对齐的日期会一直顶到下一列起点，
/// 中间没有任何间隙，只有按列起点切才能分对。
fn cut_by_grid(l: &Line, grid: &[f64]) -> Vec<String> {
    let mut cells = vec![String::new(); grid.len()];
    for (x, c) in &l.chars {
        let mut k = 0usize;
        for (i, g) in grid.iter().enumerate() {
            if *x >= *g - 1.5 {
                k = i;
            }
        }
        cells[k].push(*c);
    }
    cells
        .into_iter()
        .map(|s| {
            let mut o = String::new();
            let mut sp = false;
            for ch in s.chars() {
                if ch.is_whitespace() {
                    if !o.is_empty() {
                        sp = true;
                    }
                } else {
                    if sp {
                        o.push(' ');
                        sp = false;
                    }
                    o.push(ch);
                }
            }
            o.replace('|', "\\|")
        })
        .collect()
}

/// 这一行的「文本块起点」是否都落在列网格上。
/// 没有这层校验，两列的键值表会一路把后面的正文、目录行全吞进表格
/// （网格只有 2 列时，任何一行都能被切成 2 格，而且行首都恰好对齐第一列）。
fn fits_grid(l: &Line, grid: &[f64]) -> bool {
    let segs = segments_of(l);
    if segs.len() < 2 {
        return false;
    }
    let mut ok = 0usize;
    for (x, _) in &segs {
        if grid.iter().any(|g| (g - x).abs() <= 5.0) {
            ok += 1;
        }
    }
    ok * 10 >= segs.len() * 8
}

fn render_table(rows: &[Vec<String>]) -> String {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push('|');
        for c in 0..cols {
            out.push(' ');
            out.push_str(row.get(c).map(|s| s.as_str()).unwrap_or(""));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            for _ in 0..cols {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out
}

/// 把一段文字拼进单元格。以项目符号开头的续行另起一行（`<br>`），保留单元格里的列表结构；
/// 圆点后补一个空格 —— PDF 里圆点和正文之间靠定位拉开，文字层没有空格（`•Reading`）。
fn join_cell(slot: &mut String, t: &str) {
    let t = t.trim();
    if t.is_empty() {
        return;
    }
    let mut it = t.chars();
    let first = it.next().unwrap_or(' ');
    let bullet = is_bullet_char(first);
    if !slot.is_empty() {
        slot.push_str(if bullet { "<br>" } else { " " });
    }
    if bullet {
        slot.push(first);
        slot.push(' ');
        slot.push_str(it.as_str().trim_start());
    } else {
        slot.push_str(t);
    }
}

/// 按列网格切好的一行 → 单元格（统一经过 join_cell 整理圆点）
fn row_of(cells: &[String], cols: usize) -> Vec<String> {
    let mut row = vec![String::new(); cols.max(cells.len()).max(1)];
    for (k, c) in cells.iter().enumerate() {
        join_cell(&mut row[k], c);
    }
    row
}

/// 回扫时暂存的续行是**自下而上**收集的，拼回单元格时要恢复成自上而下的阅读顺序
fn append_pending(row: &mut [String], pend: &mut Vec<(usize, String)>) {
    for (k, t) in pend.drain(..).rev() {
        if let Some(slot) = row.get_mut(k) {
            join_cell(slot, &t);
        }
    }
}

/// 列优先排版的一行：首列的多行标签（同样自下而上收集）+ 暂存的续行
fn label_row(label: &mut Vec<String>, pend: &mut Vec<(usize, String)>, cols: usize) -> Vec<String> {
    let mut row = vec![String::new(); cols.max(1)];
    for t in label.drain(..).rev() {
        join_cell(&mut row[0], &t);
    }
    append_pending(&mut row, pend);
    row
}

/// 正向扫描时识别「列优先」的表格行：从 idx 起连续几行只有首列有字（多行标签），
/// 紧接着的下一行只在非首列有字、且纵向回到了标签首行附近 ——
/// PDF 先画完左列的 `Unimplemented, / Reserved`，再从同一行顶部画右列的说明。
/// 返回标签行数；不是这种形态返回 0。
fn column_first_labels(all: &[Line], idx: usize, grid: &[f64], head_x: f64, size: f64) -> usize {
    let Some(first) = all.get(idx) else { return 0 };
    let mut n = 0usize;
    let mut last_y = f64::MIN;
    while idx + n < all.len() && n < 6 {
        let l = &all[idx + n];
        if l.page != first.page || (l.size - size).abs() > 0.8 || l.fig_caption != 0 {
            return 0;
        }
        let cells = cut_by_grid(l, grid);
        let filled: Vec<usize> = (0..cells.len()).filter(|k| !cells[*k].is_empty()).collect();
        if filled.len() == 1 && filled[0] == 0 {
            let fx = l.chars.first().map(|c| c.0).unwrap_or(-1.0);
            if (fx - head_x).abs() > 5.0 || l.y < last_y {
                return 0;
            }
            last_y = l.y;
            n += 1;
            continue;
        }
        // 第一条非标签行：只能落在非首列，而且必须回到标签首行的高度
        let back_to_top = l.y <= first.y + size * 0.6;
        let ok = n > 0 && !filled.is_empty() && filled[0] != 0 && back_to_top;
        return if ok { n } else { 0 };
    }
    0
}

/* ────────── 断句判断 ────────── */

#[derive(Clone, Copy, PartialEq, Debug)]
enum EndKind {
    /// 句子没结束（含以右引号/右括号收尾的情况）
    Open,
    /// 分句层级：只有行排满时才算被排版切断
    Soft,
    /// 明确的句末，不合并
    Hard,
}

/// 判断行尾标点的强弱。
///
/// 旧实现把 `”` `』` `」` `】` `）` `)` 一律当硬结尾，中文书里这些符号绝大多数出现在**句中**
/// （「而“非暴力 Ahimsa”⏎则让你真正活出你自己。」），实测导致 13.5% 的段落被拦腰截断。
/// 现在先剥掉尾部闭合符，再看里面真正的标点。
fn end_kind(text: &str) -> EndKind {
    let t = text.trim_end();
    if t.is_empty() {
        return EndKind::Open;
    }
    let core = t.trim_end_matches(|c: char| {
        matches!(c, '”' | '’' | '』' | '」' | '】' | '）' | ')' | ']' | '》' | '"' | '\'')
    });
    if core.is_empty() {
        return EndKind::Open;
    }
    match core.chars().last() {
        Some('。') | Some('！') | Some('？') | Some('!') | Some('?') => EndKind::Hard,
        Some('.') => {
            if core.ends_with("..") {
                EndKind::Soft
            } else {
                EndKind::Hard
            }
        }
        Some('；') | Some(';') | Some('：') | Some(':') => EndKind::Soft,
        Some('、') | Some(',') | Some('，') => EndKind::Soft,
        _ => EndKind::Open,
    }
}

/* ────────── 列表 / 目录 ────────── */

fn is_bullet_char(c: char) -> bool {
    matches!(
        c,
        '•' | '·'
            | '▪'
            | '▫'
            | '◦'
            | '‣'
            | '∙'
            | '・'
            | '●'
            | '○'
            | '■'
            | '□'
            | '▶'
            | '▸'
            | '◆'
            | '◇'
            | '★'
            | '☆'
            | '✓'
            | '✔'
    )
}

fn bullet_prefix(t: &str) -> Option<String> {
    let mut it = t.chars();
    let c = it.next()?;
    let rest: String = it.collect();

    // 项目符号
    if is_bullet_char(c) {
        return Some(format!("- {}", rest.trim_start()));
    }
    // 圈号 ①-⑳ / ⑴-⒇ / ⒈-⒛ / ⒜-⒵
    if ('\u{2460}'..='\u{24B5}').contains(&c) {
        return Some(format!("- {}", rest.trim_start()));
    }
    // （1）（一）/(1)/(a)
    if matches!(c, '（' | '(') {
        let inner: String = rest.chars().take_while(|x| *x != '）' && *x != ')').collect();
        // （一）等 CJK 编号占 3 字节，但 chars() 只算 1 个字符 —— 必须按字符数跳过，
    // 否则 `（一）第一步` 会被吃掉两个字，输出成 `- 一步`。
    let after: String = rest.chars().skip(inner.chars().count() + 1).collect();
        let looks_num = !inner.is_empty()
            && inner.chars().count() <= 4
            && inner
                .chars()
                .all(|x| x.is_ascii_digit() || ('\u{4E00}'..='\u{9FFF}').contains(&x) || x.is_ascii_alphabetic());
        if looks_num && !after.trim().is_empty() {
            return Some(format!("- {}", after.trim_start()));
        }
        return None;
    }
    // 1. / 1) / 1、  （排除 1.0.0 这类版本号）
    if c.is_ascii_digit() {
        let s: String = t.chars().take_while(|x| x.is_ascii_digit()).collect();
        let after: String = t.chars().skip(s.len()).collect();
        let head = after.chars().next();
        if matches!(head, Some('.') | Some(')') | Some('、')) {
            let tail: String = after.chars().skip(1).collect();
            // 紧跟数字的是版本号/编号（1.0.0、2.5），不是列表项
            if !tail.chars().next().is_some_and(|x| x.is_ascii_digit()) && !tail.trim().is_empty() {
                return Some(format!("{}. {}", s, tail.trim_start()));
            }
        }
        return None;
    }
    // a) / a.
    if c.is_ascii_alphabetic() {
        if let Some(head) = rest.chars().next() {
            if matches!(head, ')' | '.') {
                let tail: String = rest.chars().skip(1).collect();
                if !tail.trim().is_empty() && t.chars().count() > 2 {
                    return Some(format!("- {}", tail.trim_start()));
                }
            }
        }
    }
    None
}

fn looks_like_page_number(t: &str) -> bool {
    let s = t.trim();
    if s.is_empty() || s.chars().count() > 12 {
        return false;
    }
    if s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == ' ') {
        return true;
    }
    let s2: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (s2.starts_with('第') && s2.ends_with('页') && s2.chars().count() <= 8)
        || (s2.starts_with('-')
            && s2.ends_with('-')
            && s2.chars().all(|c| c.is_ascii_digit() || c == '-'))
        || (s2.contains('/') && s2.chars().all(|c| c.is_ascii_digit() || c == '/'))
}

/// 目录行：`标题 ………… 12`、`标题 ........ 1-52`、`标题 ........ TOC-1` → (标题, 页标)
fn toc_entry(t: &str) -> Option<(String, String)> {
    let s = t.trim();
    let chars: Vec<char> = s.chars().collect();
    let mut i = chars.len();
    while i > 0 && (chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '-') {
        i -= 1;
    }
    if i == chars.len() || i == 0 {
        return None;
    }
    let page: String = chars[i..].iter().collect();
    if !is_page_label(&page) {
        return None;
    }
    let head: String = chars[..i].iter().collect();
    let head_t = head.trim_end();
    let dots = head_t
        .chars()
        .rev()
        .take_while(|c| is_dot_char(*c) || *c == ' ')
        .filter(|c| is_dot_char(*c))
        .count();
    if dots < 3 {
        return None;
    }
    let title = head_t
        .trim_end_matches(|c: char| is_dot_char(c) || c == ' ')
        .trim()
        .to_string();
    if title.is_empty() || title.chars().count() > 200 {
        return None;
    }
    Some((title, page))
}

/// 一行里塞了多个项目符号（「非暴力沟通能够：●…；●…；●…」）时拆成多段
fn split_inline_bullets(t: &str) -> Vec<String> {
    let marks: Vec<char> = t.chars().filter(|c| is_bullet_char(*c)).collect();
    if marks.len() < 2 {
        return vec![t.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut last_was_mark = false;
    for c in t.chars() {
        if is_bullet_char(c) {
            let head = cur.trim();
            if !head.is_empty() {
                out.push(head.to_string());
            }
            cur.clear();
            last_was_mark = true;
        } else {
            let _ = last_was_mark;
            cur.push(c);
        }
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/* ────────── 双栏重排 ────────── */

fn y_span(v: &[Line]) -> (f64, f64) {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for l in v {
        if l.y < lo {
            lo = l.y;
        }
        if l.y > hi {
            hi = l.y;
        }
    }
    (lo, hi)
}

/// 把一行按横坐标切成左右两半（双栏页面上左右两栏共享同一条基线）
fn split_row(l: &Line, mid: f64) -> (Line, Line) {
    let mut a = l.clone();
    let mut b = l.clone();
    a.segs = l.segs.iter().filter(|s| s.x < mid).cloned().collect();
    b.segs = l.segs.iter().filter(|s| s.x >= mid).cloned().collect();
    a.chars = l.chars.iter().filter(|(x, _)| *x < mid).cloned().collect();
    b.chars = l.chars.iter().filter(|(x, _)| *x >= mid).cloned().collect();
    if a.segs.is_empty() {
        a.segs.push(Seg { x: l.x0, end: l.x0, text: String::new() });
    }
    if b.segs.is_empty() {
        b.segs.push(Seg { x: mid, end: mid, text: String::new() });
    }
    a.text = line_text(&a);
    b.text = line_text(&b);
    a.x0 = a.chars.first().map(|c| c.0).unwrap_or(l.x0);
    a.x1 = a.chars.last().map(|c| c.0).unwrap_or(a.x0);
    b.x0 = b.chars.first().map(|c| c.0).unwrap_or(mid);
    b.x1 = b.chars.last().map(|c| c.0).unwrap_or(b.x0);
    (a, b)
}

/// 逐页检测双栏。
///
/// 判据不是「行首 x0 分两簇」—— 左右两栏常常共享同一条基线，早在 Dev 阶段
/// 就被并成一行了。这里改用**字符级横向空隙**找分栏线，再用「两侧都铺满版面
/// 纵向 30% 以上」把两列键值表排除掉（键值表只有几行，纵向跨度很小）。
fn split_two_columns(seg: &[Line]) -> Option<(Vec<Line>, Vec<Line>)> {
    let body: Vec<&Line> = seg.iter().filter(|l| !in_margin(l)).collect();
    if body.len() < 8 {
        return None;
    }
    let pw = body[0].page_w.max(1.0);
    let ph = body[0].page_h.max(1.0);

    let mut xs: Vec<f64> = Vec::new();
    for l in &body {
        for (x, _) in &l.chars {
            xs.push(*x);
        }
    }
    if xs.len() < 80 {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (mut best, mut at) = (0.0f64, 0usize);
    for i in 1..xs.len() {
        let g = xs[i] - xs[i - 1];
        if g > best {
            best = g;
            at = i;
        }
    }
    if best < pw * 0.06 {
        return None;
    }
    let mid = (xs[at - 1] + xs[at]) / 2.0;
    if mid < pw * 0.25 || mid > pw * 0.75 {
        return None;
    }

    let mut lrows: Vec<Line> = Vec::new();
    let mut rrows: Vec<Line> = Vec::new();
    for l in seg {
        if l.x1 <= mid + 2.0 {
            lrows.push(l.clone());
        } else if l.x0 >= mid - 2.0 {
            rrows.push(l.clone());
        } else {
            let (a, b) = split_row(l, mid);
            if !a.text.trim().is_empty() {
                lrows.push(a);
            }
            if !b.text.trim().is_empty() {
                rrows.push(b);
            }
        }
    }
    if lrows.len() < 4 || rrows.len() < 4 {
        return None;
    }

    let (ly0, ly1) = y_span(&lrows);
    let (ry0, ry1) = y_span(&rrows);
    if (ly1 - ly0) < ph * 0.30 || (ry1 - ry0) < ph * 0.30 {
        return None;
    }
    let overlap = ly1.min(ry1) - ly0.max(ry0);
    let shorter = (ly1 - ly0).min(ry1 - ry0).max(1.0);
    if overlap / shorter < 0.5 {
        return None;
    }
    lrows.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    rrows.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    Some((lrows, rrows))
}

/// 逐页重排：判定为双栏的页面，先整栏左、再整栏右，避免左右栏逐行交错
fn reorder_columns(lines: &mut Vec<Line>) {
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    let mut i = 0usize;
    while i < lines.len() {
        let page = lines[i].page;
        let mut j = i;
        while j < lines.len() && lines[j].page == page {
            j += 1;
        }
        let seg = &lines[i..j];
        match split_two_columns(seg) {
            Some((mut l, mut r)) => {
                // 栏编号必须与单栏页区分开：0 = 单栏，1 = 双栏左，2 = 双栏右。
                // 若左右栏都记作 0/1，左栏就会和所有单栏页混在一组统计版心宽度，
                // 版心被拉到整页宽，栏内每行都判成「没排满」，段段断开。
                for x in l.iter_mut() {
                    x.col = 1;
                }
                for x in r.iter_mut() {
                    x.col = 2;
                }
                out.extend(l);
                out.extend(r);
            }
            None => out.extend(seg.iter().cloned()),
        }
        i = j;
    }
    *lines = out;
}

/* ────────── 行 → Markdown ────────── */

#[derive(Clone)]
struct Block {
    page: u32,
    y: f64,
    size: f64,
    text: String,
    heading: bool,
    right_ratio: f64,
    /// 已经渲染好的 Markdown 片段（表格），直接输出、不参与合并
    pre: bool,
    left: f64,
    /// 这一块文字实际覆盖到的最后一页（跨页续接时大于 page）
    page_end: u32,
    /// 行内存在列级横向跳变（表格行 / 多栏），不参与断行合并
    multi: bool,
    /// 这一块**最后一个物理行**的基线纵坐标，用来判断下一行是不是接着的续行
    last_y: f64,
    /// 图形层列表圆点
    bullet: bool,
    /// 提示框编号（0 = 不在框内）
    quote: u32,
    /// 提示框标题行
    quote_head: bool,
    /// 框内是代码
    quote_code: bool,
    /// 书签层级（0 = 不是书签标题）
    level: u8,
    /// 矢量图的图题（图编号，0 = 不是图题）
    fig: u32,
}

fn raw_level_of(size: f64, body: f64) -> u8 {
    let r = size / body.max(0.1);
    if r >= 1.8 {
        1
    } else if r >= 1.5 {
        2
    } else if r >= 1.28 {
        3
    } else {
        4
    }
}

/// 页面上下边缘区域（页眉 / 页脚）
fn in_margin(l: &Line) -> bool {
    l.y < l.page_h * 0.09 || l.y > l.page_h * 0.91
}

/// 页眉页脚的比对键：数字串统一成 `#`，空白压缩成一个空格
fn edge_key(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut in_digits = false;
    let mut space = false;
    for c in t.trim().chars() {
        if c.is_whitespace() {
            space = true;
            in_digits = false;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

/// 某一页上的边缘行是否属于页眉页脚：
///   · 出现在 ≥2 个不同页、且覆盖半数以上的页（全书统一的页眉页脚）；
///   · 或者含字母、且前后 4 页内至少还有 2 页在**同一高度**（±3pt）出现 ——
///     章节页眉、带页码的页脚都套用固定版式，位置纹丝不动；
///     脚注也可能落在页面底部区域、甚至连续几页内容相同，但它的高度随正文长短变化，靠这一条区分。
///     纯数字的行不走这一条，交给 looks_like_page_number。
fn is_running_edge(key: &str, seen: &BTreeMap<u32, f64>, page: u32, y: f64, pages: u32) -> bool {
    if seen.len() >= 2 && (seen.len() as f64) >= pages as f64 * 0.5 {
        return true;
    }
    if !key.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    let lo = page.saturating_sub(4);
    let hi = page.saturating_add(4);
    seen.range(lo..=hi)
        .filter(|(p, py)| **p != page && (**py - y).abs() <= 3.0)
        .count()
        >= 2
}

/// 内容流顺序异常时整页文字是倒着输出的（`/Rotate 180` 的页最典型）。
/// 这类页的行 y 严格单调递减，若不反转，断行合并的纵向间距判据会全部失败，
/// 整页被切成一行一段。
fn fix_reversed_pages(all: &mut [Line]) {
    let mut i = 0usize;
    while i < all.len() {
        let page = all[i].page;
        let mut j = i;
        while j < all.len() && all[j].page == page {
            j += 1;
        }
        if j - i >= 2 {
            let mut desc = true;
            for k in i + 1..j {
                if all[k].y >= all[k - 1].y {
                    desc = false;
                    break;
                }
            }
            if desc {
                all[i..j].reverse();
            }
        }
        i = j;
    }
}

/// 用图形层信息给每行打标：
/// 左侧有矢量圆点 → 列表项；落进 `/BlockQuote` 底色框 → 提示框。
fn apply_struct(all: &mut [Line], meta: &PageMeta) {
    /* 列表圆点：圆点垂直中心大致在文字视觉中心（基线往上约 0.3 个字高），
     * 水平上必须在行首左侧、且不至于离得太远（超过 3 个字宽就不是同一行了）。 */
    for l in all.iter_mut() {
        if let Some(bs) = meta.bullets.get(&l.page) {
            let cy = l.y - l.size * 0.30;
            l.bullet = bs.iter().any(|(bx, by)| {
                (by - cy).abs() <= l.size * 0.55 && *bx < l.x0 && l.x0 - *bx <= l.size * 3.2
            });
        }
    }

    let mut boxes: Vec<(u32, Rect, u32)> = Vec::new();
    let mut pages: Vec<u32> = meta.boxes.keys().copied().collect();
    pages.sort_unstable();
    let mut id = 0u32;
    for p in pages {
        let mut v = meta.boxes[&p].clone();
        v.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        for r in v {
            id += 1;
            boxes.push((p, r, id));
        }
    }
    if boxes.is_empty() {
        return;
    }

    for l in all.iter_mut() {
        for (p, r, bid) in &boxes {
            if *p != l.page {
                continue;
            }
            // 行基线要在框的纵向范围内（留半个字高的余量），横向也必须在框内
            if l.y >= r.y0 - l.size * 0.7
                && l.y <= r.y1 + l.size * 0.7
                && l.x0 >= r.x0 - 2.0
                && l.x1 <= r.x1 + 8.0
            {
                l.quote = *bid;
                break;
            }
        }
    }

    /* 框的首行通常是「注意」「说明」这类框标题，单独加粗成小标题。
     * 只有一行内容的框不加粗 —— 那整句就是正文。 */
    for bid in 1..=id {
        let mut head: Option<(usize, f64)> = None;
        let mut n = 0usize;
        for (i, l) in all.iter().enumerate() {
            if l.quote != bid {
                continue;
            }
            n += 1;
            if head.map(|(_, y)| l.y < y).unwrap_or(true) {
                head = Some((i, l.y));
            }
        }
        if n >= 2 {
            if let Some((i, _)) = head {
                all[i].quote_head = true;
            }
        }
    }

    /* 判定哪些框装的是代码。
     * 这类框（「示例」框）里的换行是**有意为之**的语句边界，合并成段落会毁掉代码，
     * 所以要先识别出来，后面既不合并、也换成围栏代码块输出。
     * 判据：去掉标题行后，正文字符以 ASCII 为主，且出现代码符号。 */
    let mut code_box: Vec<bool> = vec![false; id as usize + 1];
    for bid in 1..=id {
        let mut body = String::new();
        for l in all.iter() {
            if l.quote == bid && !l.quote_head {
                body.push_str(&l.text);
            }
        }
        let n = body.chars().filter(|c| !c.is_whitespace()).count();
        if n < 40 {
            continue;
        }
        let cjk = body.chars().filter(|c| is_cjk(*c)).count();
        let sym = body.contains('{')
            || body.contains(';')
            || body.contains("/*")
            || body.contains("->")
            || body.contains(");");
        code_box[bid as usize] = sym && (cjk as f64) < (n as f64) * 0.35;
    }
    for l in all.iter_mut() {
        if l.quote != 0 {
            l.quote_code = code_box[l.quote as usize];
        }
    }

    /* 代码框首行常带一个「语言标记」小字（实测 `c TASK(...)`）。
     * 这类标记只有一两个字符、和代码同一个基线，留在首行会变成 `c TASK(...)`。
     * 只在「单个 c/C + 空格 + 大写标识符或 #」这一种形态下剥离，避免误伤真代码。 */
    let mut head_done: Vec<u32> = Vec::new();
    for l in all.iter_mut() {
        if l.quote == 0 || !l.quote_code || l.quote_head || head_done.contains(&l.quote) {
            continue;
        }
        head_done.push(l.quote);
        let t = l.text.clone();
        let cs: Vec<char> = t.chars().collect();
        if cs.len() > 3 && matches!(cs[0], 'c' | 'C') && cs[1] == ' ' {
            let next = cs[2];
            if next.is_ascii_uppercase() || next == '#' {
                let rest: String = cs[2..].iter().collect();
                set_line_text(l, &rest);
            }
        }
    }
    if std::env::var("MD_PDF_TRACE_Q").is_ok() {
        for l in all.iter() {
            if l.quote != 0 && l.quote_code {
                eprintln!(
                    "QBOX{} head={} p{} x0={:.1} x1={:.1} {:?}",
                    l.quote,
                    l.quote_head as u8,
                    l.page,
                    l.x0,
                    l.x1,
                    l.text
                );
            }
        }
    }
}

/* ────────── 目录：点线引导符、页标与折行 ────────── */

fn is_dot_char(c: char) -> bool {
    matches!(c, '.' | '·' | '…' | '．')
}

/// 目录页标：`12`、`1-52`、`TOC-1`、`Preface-1`、`RevisionHistory-3`。
/// 必须含数字且以数字结尾 —— 标题里的 `2nd`、`16bit` 不算页码。
fn is_page_label(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > 24 || t.starts_with('-') {
        return false;
    }
    let mut digit = false;
    for c in t.chars() {
        if c.is_ascii_digit() {
            digit = true;
        } else if !(c.is_ascii_alphabetic() || c == '-') {
            return false;
        }
    }
    digit && t.ends_with(|c: char| c.is_ascii_digit())
}

/// 没有点线也能单独认定的页标：只认带连字符的「章-页」形态（`25-3`、`TOC-1`）。
/// 纯数字太容易和标题末尾的数字（`… 2 Radar SPUs`）混淆。
fn is_strong_label(t: &str) -> bool {
    t.contains('-') && is_page_label(t)
}

/// 行首是不是章节编号（`1 `、`2.1 `、`19.2.8.22 `）
fn starts_with_section_number(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_ascii_digit() => {}
        _ => return false,
    }
    for c in it {
        if c.is_ascii_digit() || c == '.' {
            continue;
        }
        return c.is_whitespace();
    }
    false
}

/// 是否含目录点线：连续的点号至少 4 个，允许夹空格（`. . . .` 和 `....` 都算）
/// 目录点线的计数：`…`（U+2026）**只能算 1 个**。
/// 中文正文里的省略号就是两个 `…` 连写（`……`），按 3 个点算会凑满 4 点阈值，
/// 于是整段正文被当成目录行重排，省略号在重排时消失（实测《非暴力沟通》丢了几十处）。
/// 真正的目录引导符用的是句点 `. . . .`，或者四个以上的 `…`。
fn has_leader(t: &str) -> bool {
    let mut dots = 0usize;
    let mut ell = 0usize;
    for c in t.chars() {
        if is_dot_char(c) {
            if c == '…' {
                ell += 1;
            } else {
                dots += 1;
            }
            if dots >= 4 || ell >= 4 {
                return true;
            }
        } else if c != ' ' {
            dots = 0;
            ell = 0;
        }
    }
    false
}

/// 目录行里的记号
enum TocTok {
    Text(String),
    /// 点线引导符
    Leader,
    /// 页标
    Page(String),
}

fn flush_toc_text(out: &mut Vec<TocTok>, txt: &mut String) {
    let t = txt.trim();
    if !t.is_empty() {
        out.push(TocTok::Text(t.to_string()));
    }
    txt.clear();
}

/// 把一行目录切成记号流：正文 / 点线 / 页标。
///
/// 目录行的实际阅读顺序可能是 `[上一条的尾巴] …… [页码] [下一条的开头]`，
/// 一行里出现两组点线是常态，所以按记号流处理，不能只找最长的那一段。
/// 点线判定（允许夹空格）：
///   · 句点（`.` `·` `．`）达到 4 个，或 `…` 连写 4 个以上；
///   · 或至少 2 个句点、后面紧跟页标；
///   · 或前面是空白、句点一直延伸到行尾 —— 长标题把页标挤到了下一行，本行只剩半截点线。
/// 后两条只认句点：中文正文的省略号 `……` 是两个 `…`，按点线处理会连同省略号一起被吃掉。
/// 点线后的页标取最长的 `[A-Za-z0-9-]` 串：`…….13图 2-2…` 里只取 `13`，后面是下一条的开头。
fn toc_tokens(t: &str) -> Vec<TocTok> {
    let cs: Vec<char> = t.chars().collect();
    let mut out: Vec<TocTok> = Vec::new();
    let mut txt = String::new();
    let mut i = 0usize;
    while i < cs.len() {
        if !is_dot_char(cs[i]) {
            txt.push(cs[i]);
            i += 1;
            continue;
        }
        let s = i;
        let mut dots = 0usize;
        let mut ell = 0usize;
        let mut j = i;
        while j < cs.len() && (is_dot_char(cs[j]) || cs[j] == ' ') {
            if is_dot_char(cs[j]) {
                if cs[j] == '…' {
                    ell += 1;
                } else {
                    dots += 1;
                }
            }
            j += 1;
        }
        let mut k = j;
        while k < cs.len() && (cs[k].is_ascii_alphanumeric() || cs[k] == '-') {
            k += 1;
        }
        let label: String = cs[j..k].iter().collect();
        let has_label = is_page_label(&label);
        let to_end = j >= cs.len();
        let spaced = s > 0 && cs[s - 1].is_whitespace();
        // 句点那条线（`dots`）才有弱判据；`…` 只有连写 4 个以上才认，
        // 否则中文正文的省略号 `……` 会被当成引导符，重排后连同省略号一起消失
        let is_leader = dots >= 4
            || ell >= 4
            || (dots >= 2 && has_label)
            || (to_end && spaced && dots >= 1);
        if is_leader {
            flush_toc_text(&mut out, &mut txt);
            out.push(TocTok::Leader);
            if has_label {
                out.push(TocTok::Page(label));
                i = k;
            } else {
                i = j;
            }
        } else {
            // 不是点线：版本号、编号里的点，句末句点，原样当正文
            for c in &cs[s..j] {
                txt.push(*c);
            }
            i = j;
        }
    }
    flush_toc_text(&mut out, &mut txt);
    out
}

/// 一行目录的记号流，另外补上两种没有点线的形态：
///   · 整行只有页标（长标题把页标挤到了下一行，`19-35` 单独一行）；
///   · 编号条目的标题排满整行，页标直接跟在后面（`25.2.1.2 … enabled  25-3`）。
fn toc_line_tokens(t: &str) -> Vec<TocTok> {
    let s = t.trim();
    if is_page_label(s) {
        return vec![TocTok::Page(s.to_string())];
    }
    let toks = toc_tokens(s);
    if toks.len() == 1 && starts_with_section_number(s) {
        let split = match toks.first() {
            Some(TocTok::Text(x)) => x
                .rsplit_once(char::is_whitespace)
                .filter(|(head, last)| is_strong_label(last) && !head.trim().is_empty())
                .map(|(head, last)| (head.trim().to_string(), last.to_string())),
            _ => None,
        };
        if let Some((head, last)) = split {
            return vec![TocTok::Text(head), TocTok::Page(last)];
        }
    }
    toks
}

/// 目录区内部可以接续的行：带点线、只有页标、或者「标题 + 页标」的编号条目
fn toc_line_ok(t: &str) -> bool {
    let toks = toc_line_tokens(t);
    toks.iter().any(|x| matches!(x, TocTok::Leader))
        || matches!(
            toks.as_slice(),
            [TocTok::Page(_)] | [TocTok::Text(_), TocTok::Page(_)]
        )
}

fn set_line_text(l: &mut Line, t: &str) {
    l.text = t.to_string();
    l.segs = vec![Seg {
        x: l.x0,
        end: l.x1.max(l.x0 + 1.0),
        text: t.to_string(),
    }];
    l.chars.clear();
}

/// 把目录区重排成「标题 ........ 页标」，每条一行，交给 `toc_entry` 输出成 `- 标题 · 页标`。
///
/// 不重排的话：条目被点线拆成两半、还和相邻条目粘在一起
/// （`图 2-1AUTOSAR架构 4.3.0.......13图 2-2BSW卫星副本与代理`）；
/// 表格识别还会把「编号 | 标题」切成两列；长标题把页标挤到下一行时，页标变成孤零零的一行。
/// 目录区从带点线（≥4 个点）的行开始，向后吸收：带点线的行、只有页标的行、
/// 以页标收尾的无点线编号条目，以及夹在中间、编号开头的折行长标题。
/// 任何文字都不丢：缺页标的条目、孤立的页标都原样保留成单独一行。
fn rebuild_toc(all: &mut Vec<Line>) {
    let mut i = 0usize;
    while i < all.len() {
        if !has_leader(&all[i].text) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < all.len() {
            let t = all[j].text.as_str();
            if has_leader(t) || toc_line_ok(t) {
                j += 1;
                continue;
            }
            let wrapped = starts_with_section_number(t.trim())
                && t.chars().count() <= 160
                && j + 1 < all.len()
                && has_leader(&all[j + 1].text);
            if wrapped {
                j += 1;
                continue;
            }
            break;
        }

        // 条目：(标题, 页标, 标题起始行)。起始行决定输出位置所在的页与纵坐标
        let mut entries: Vec<(String, Option<String>, usize)> = Vec::new();
        let mut title = String::new();
        let mut title_line = i;
        let mut awaiting = false;
        for k in i..j {
            for tok in toc_line_tokens(&all[k].text) {
                match tok {
                    TocTok::Text(s) => {
                        // 点线之后没等到页标又出现了正文：上一条缺页标，单独成条
                        if awaiting && !title.is_empty() {
                            entries.push((std::mem::take(&mut title), None, title_line));
                        }
                        awaiting = false;
                        if title.is_empty() {
                            title_line = k;
                        } else {
                            let a = title.chars().last().unwrap_or(' ');
                            let b = s.chars().next().unwrap_or(' ');
                            if !is_wide(a) && !is_wide(b) {
                                title.push(' ');
                            }
                        }
                        title.push_str(&s);
                    }
                    TocTok::Leader => awaiting = true,
                    TocTok::Page(p) => {
                        let attach = title.is_empty()
                            && entries.last().map(|e| e.1.is_none()).unwrap_or(false);
                        if !title.is_empty() {
                            entries.push((std::mem::take(&mut title), Some(p), title_line));
                        } else if attach {
                            if let Some(last) = entries.last_mut() {
                                last.1 = Some(p);
                            }
                        } else {
                            // 孤立的页标：不知道属于哪一条，原样保留
                            entries.push((p, None, k));
                        }
                        awaiting = false;
                    }
                }
            }
        }
        if !title.is_empty() {
            entries.push((title, None, title_line));
        }

        let mut fresh: Vec<Line> = Vec::with_capacity(entries.len());
        for (t, page, src) in entries {
            let mut l = all[src].clone();
            let t = squeeze_cjk_spaces(&t);
            match page {
                Some(p) => set_line_text(&mut l, &format!("{} ........ {}", t, p)),
                None => set_line_text(&mut l, &t),
            }
            fresh.push(l);
        }
        let n = fresh.len();
        all.splice(i..j, fresh);
        // n == 0：这一片只有孤立的点线，已被删除；原地继续检查挪到 i 的新行
        i += n;
    }
}

/// 「1 简介」「2.1 术语和基本概念」这种编号目录行。
/// 判定必须严格，否则正文里以数字开头的句子会被误伤：
/// · 编号后紧跟空白（`1 简介` 成立，`1. 第一部分` 不成立 —— 那种由 bullet_prefix 负责）
/// · 整行短、且不以句末标点收尾
fn is_numbered_toc(t: &str) -> bool {
    let s = t.trim();
    // 已经重排好的「标题 ........ 页标」由 toc_entry 负责，不能再被加一层列表前缀
    if toc_entry(s).is_some() {
        return false;
    }
    let n = s.chars().count();
    if n < 3 || n > 60 {
        return false;
    }
    if s.chars()
        .last()
        .map(|c| matches!(c, '。' | '，' | '！' | '？' | '；' | '：' | '、' | ',' | '.'))
        .unwrap_or(true)
    {
        return false;
    }
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let mut digits = 0;
    while i < cs.len() && cs[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if digits == 0 || digits > 2 {
        return false;
    }
    // 最多两级：「2」「2.1」
    let mut levels = 0;
    while i < cs.len() && cs[i] == '.' {
        let mut k = i + 1;
        let mut d = 0;
        while k < cs.len() && cs[k].is_ascii_digit() {
            k += 1;
            d += 1;
        }
        if d == 0 || d > 2 || levels >= 2 {
            break;
        }
        levels += 1;
        i = k;
    }
    i < cs.len() && cs[i].is_whitespace()
}

/// 连续多行的编号目录 → 输出成列表项
fn mark_numbered_toc(all: &mut [Line]) {
    let mut i = 0usize;
    while i < all.len() {
        if !is_numbered_toc(&all[i].text) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < all.len() && is_numbered_toc(&all[j].text) {
            j += 1;
        }
        // 少于 4 行的连续编号更可能是正文里的并列短句，不动
        if j - i >= 4 {
            for l in all[i..j].iter_mut() {
                let t = l.text.trim().to_string();
                set_line_text(l, &format!("- {}", t));
            }
        }
        i = j;
    }
}

/* ────────── 书签 → 标题层级 ────────── */

/// 标题比对键：只留字母数字（含汉字）并统一小写 —— 抹平空格、™、标点与断词差异
fn norm_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 书签索引：目标页 → [(层级, 比对键, 是否已认领)]
struct OutlineIndex {
    by_page: HashMap<u32, Vec<(u8, String, bool)>>,
    total: usize,
    matched: usize,
}

impl OutlineIndex {
    fn new(outline: &[(u8, String, u32)]) -> OutlineIndex {
        let mut by_page: HashMap<u32, Vec<(u8, String, bool)>> = HashMap::new();
        let mut total = 0usize;
        for (level, title, page) in outline {
            let key = norm_key(title);
            if *page == 0 || key.chars().count() < 2 {
                continue;
            }
            total += 1;
            by_page.entry(*page).or_default().push((*level, key, false));
        }
        OutlineIndex { by_page, total, matched: 0 }
    }

    /// 认领一条书签：同一页、比对键一致、尚未被认领过。返回书签层级
    fn claim(&mut self, page: u32, text: &str) -> Option<u8> {
        let key = norm_key(text);
        if key.chars().count() < 2 {
            return None;
        }
        let list = self.by_page.get_mut(&page)?;
        let entry = list.iter_mut().find(|e| !e.2 && e.1 == key)?;
        entry.2 = true;
        let level = entry.0;
        self.matched += 1;
        Some(level)
    }

    /// 书签可信：条目够多，且至少三成能在目标页找到对应的标题行。
    /// 书签与正文对不上的文档（书签是另起的名字、目标页整体错位）退回按字号判断标题。
    fn trusted(&self) -> bool {
        self.total >= 3 && self.matched * 10 >= self.total * 3
    }
}

/// 字号够大、但明显不是标题的碎字：图里的单个字母、数字、编码片段（`K`、`3`、`96`、`SA –`）。
/// 书签可信时只给书签之外的标题用更严格的门槛 —— 真正的章节标题已经由书签认领。
fn heading_text_ok(t: &str, strict: bool) -> bool {
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    // 汉字标题可以只有一个字（「序」「跋」）；拉丁字母则要求成词
    let need = if t.chars().any(is_cjk) {
        1
    } else if strict {
        3
    } else {
        2
    };
    letters >= need
}

/* ────────── 矢量图：以图题为锚点圈出图的范围 ────────── */

/// 一幅矢量图：图内文字从正文里摘出来，紧跟图题输出成折叠块
struct Figure {
    page: u32,
    texts: Vec<String>,
}

/// 图题号之后的部分；不是图题返回 None。
/// 认 `Figure 3` / `FIGURE 3` / `Fig. 3-1` / `Abbildung 2` / `图 3`；编号后面必须是空白、标点或行尾，
/// `Figure3D` 这种词不算。
fn caption_rest(t: &str) -> Option<&str> {
    let t = t.trim();
    if t.chars().count() > 160 {
        return None;
    }
    let rest = ["Figure", "FIGURE", "Fig.", "Abbildung", "图"]
        .iter()
        .find_map(|p| t.strip_prefix(*p))?
        .trim_start();
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let n = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '.' | '–')))
        .unwrap_or(rest.len());
    let after = &rest[n..];
    if !after.is_empty()
        && !after.starts_with(|c: char| c.is_whitespace() || matches!(c, ':' | '：' | ','))
    {
        return None;
    }
    Some(after.trim())
}

fn is_caption(l: &Line) -> bool {
    let head = l.text.trim_start();
    if !(head.starts_with("Fig")
        || head.starts_with("FIG")
        || head.starts_with("Abb")
        || head.starts_with('图'))
    {
        return false;
    }
    caption_rest(&line_text(l)).is_some()
}

/// 图题在图上方的版式才向下找图。正文里的「Figure 1 shows …」也以图号开头，
/// 但它是句子（以句号收尾）—— 只有不像句子的短行才允许向下找，免得把下方的图挂到这句话上。
fn caption_strong(l: &Line) -> bool {
    let t = line_text(l);
    let t = t.trim_end();
    t.chars().count() <= 100 && !t.ends_with(|c: char| matches!(c, '.' | '。' | ':' | '：'))
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    Rect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// 从图题出发，把紧挨着的图形层标记（描边 / 填充）逐个并进来，直到出现空档。
/// up = true 向上找（图题在图下方，FrameMaker 默认版式），否则向下找。
/// 中间夹着一段正文（≥ 25 个字的行）就停：那是另一块内容，不是同一幅图。
fn grow_figure(marks: &[Rect], lines: &[&Line], cap: &Line, up: bool) -> Option<Rect> {
    const FIRST_GAP: f64 = 20.0;
    const GAP: f64 = 18.0;
    let page_h = cap.page_h.max(1.0);
    let page_w = cap.page_w.max(1.0);
    let cap_top = cap.y - cap.size;
    let cap_bottom = cap.y + cap.size * 0.3;
    let mut cand: Vec<Rect> = marks
        .iter()
        .copied()
        .filter(|m| {
            // 小圆点（列表符号）、页眉页脚区的线条与标志不参与
            let big = (m.x1 - m.x0).max(m.y1 - m.y0) >= 6.0;
            let edge = m.y1 <= page_h * 0.09 || m.y0 >= page_h * 0.91;
            let side = if up { m.y1 <= cap_top + 2.0 } else { m.y0 >= cap_bottom - 2.0 };
            big && !edge && side
        })
        .collect();
    if up {
        cand.sort_by(|a, b| b.y1.partial_cmp(&a.y1).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        cand.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
    }
    let mut edge = if up { cap_top } else { cap_bottom };
    let mut region: Option<Rect> = None;
    let mut used: Vec<Rect> = Vec::new();
    for m in cand {
        let gap = if region.is_none() { FIRST_GAP } else { GAP };
        let near = if up { m.y1 >= edge - gap } else { m.y0 <= edge + gap };
        if !near {
            break;
        }
        // 当前区域与这个标记之间的空档里有没有正文
        let (lo, hi) = if up { (m.y1, edge) } else { (edge, m.y0) };
        if hi > lo + 1.0
            && lines
                .iter()
                .any(|l| l.y > lo + 1.0 && l.y < hi - 1.0 && l.text.chars().count() >= 25)
        {
            break;
        }
        region = Some(match region {
            None => m,
            Some(r) => rect_union(r, m),
        });
        used.push(m);
        edge = if up { edge.min(m.y0) } else { edge.max(m.y1) };
    }
    let r = region?;
    // 贯穿整片区域的细横线：图只有外框的上下两条，表格每行一条（实测表 2 有 12 条）。
    // 达到 3 条就是紧挨着图题的表格，不能当成图把整张表收进折叠块
    let width = r.x1 - r.x0;
    let rules = used
        .iter()
        .filter(|m| m.y1 - m.y0 <= 1.5 && m.x1 - m.x0 >= width * 0.9)
        .count();
    if used.len() >= 2 && rules < 3 && r.y1 - r.y0 >= 30.0 && width >= page_w * 0.2 {
        Some(r)
    } else {
        None
    }
}

/// 找出矢量图并给图题打标。返回 (每行归属的图编号，0 = 正文；图列表，编号 = 下标 + 1)。
/// 框图、示意图只存在于图形层，pdf-extract 只拿得到图里零散的文字（模块名、连线标注、
/// 旋转的竖排小字），混进正文就是一堆 `Prim / a / ry / AD / C`，还会被大字号误判成标题。
fn detect_figures(all: &mut [Line], meta: &PageMeta) -> (Vec<u32>, Vec<Figure>) {
    let mut owner: Vec<u32> = vec![0; all.len()];
    let mut figs: Vec<Figure> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        let page = all[i].page;
        let mut j = i;
        while j < all.len() && all[j].page == page {
            j += 1;
        }
        if let Some(marks) = meta.graphics.get(&page) {
            for c in i..j {
                if owner[c] != 0 || !is_caption(&all[c]) {
                    continue;
                }
                let region = {
                    let lines: Vec<&Line> = all[i..j].iter().collect();
                    let cap = &all[c];
                    match grow_figure(marks, &lines, cap, true) {
                        Some(r) => Some(r),
                        None if caption_strong(cap) => grow_figure(marks, &lines, cap, false),
                        None => None,
                    }
                };
                let Some(r) = region else { continue };
                let id = figs.len() as u32 + 1;
                let mut texts: Vec<String> = Vec::new();
                for k in i..j {
                    if k == c || owner[k] != 0 {
                        continue;
                    }
                    let l = &all[k];
                    let inside = l.y >= r.y0 - 2.0
                        && l.y - l.size * 0.8 <= r.y1 + 2.0
                        && l.x0 >= r.x0 - 2.0
                        && l.x1 <= r.x1 + 2.0;
                    if inside && l.text.chars().count() <= 120 {
                        owner[k] = id;
                        texts.push(line_text(l));
                    }
                }
                all[c].fig_caption = id;
                figs.push(Figure { page, texts });
            }
        }
        i = j;
    }
    (owner, figs)
}

/// 图内文字的折叠块。正文按原样逐行放进代码块：这些碎片没有可靠的阅读顺序，
/// 只保证不丢、可搜索；看图请回到原 PDF 的对应页。
fn render_figure(f: &Figure) -> String {
    if f.texts.is_empty() {
        return String::new();
    }
    let body = f.texts.join("\n");
    let mut run = 0usize;
    let mut longest = 0usize;
    for c in body.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!(
        "<details>\n<summary>图内文字（仅供检索，版式与图形请看原 PDF 第 {} 页）</summary>\n\n{}text\n{}\n{}\n\n</details>\n\n",
        f.page, fence, body, fence
    )
}

/// 输出一幅图的折叠块（每幅只输出一次）
fn emit_figure(out: &mut String, id: u32, figures: &[Figure], done: &mut HashSet<u32>) {
    if id == 0 || !done.insert(id) {
        return;
    }
    if let Some(f) = figures.get(id as usize - 1) {
        out.push_str(&render_figure(f));
    }
}

/// 图题没能输出（被并进别的块等）时的兜底：离开这一页前把这页的图补上，文字绝不丢
fn flush_page_figures(out: &mut String, page: u32, figures: &[Figure], done: &mut HashSet<u32>) {
    for (k, f) in figures.iter().enumerate() {
        if f.page == page {
            emit_figure(out, k as u32 + 1, figures, done);
        }
    }
}

fn lines_to_md(
    all: &mut Vec<Line>,
    images: &HashMap<u32, Vec<(f64, String)>>,
    meta: &PageMeta,
) -> String {
    all.retain(|l| !l.text.trim().is_empty());
    if all.is_empty() {
        return String::new();
    }
    let pages = all.iter().map(|l| l.page).max().unwrap_or(1);

    /* 0) 先把「倒着输出」的页翻正（/Rotate 180 页），否则后面的行距判据全部失效 */
    fix_reversed_pages(all);

    /* 1) 页眉页脚：只看页面上下边缘区域，且必须在多页重复出现。
     *    旧实现全篇比对「出现 ≥3 次」，正文里重复的短行（每章都有的「本章小结」）会被误删。
     *    比对前先把数字串归一化成 `#`：页脚里的页码每页都不同（`User’s Manual 1-3 V1.5.0`），
     *    不归一化就永远凑不齐「多页重复」。章节页眉只出现在本章的页上，达不到「半数以上的页」，
     *    所以另加一条：相邻几页里反复出现的边缘行同样是页眉页脚。 */
    let mut edge_pages: HashMap<String, BTreeMap<u32, f64>> = HashMap::new();
    for l in all.iter() {
        if !in_margin(l) {
            continue;
        }
        let t = l.text.trim();
        if t.is_empty() || t.chars().count() > 80 {
            continue;
        }
        edge_pages
            .entry(edge_key(t))
            .or_default()
            .entry(l.page)
            .or_insert(l.y);
    }
    all.retain(|l| {
        if !in_margin(l) {
            return true;
        }
        let t = l.text.trim();
        if t.chars().count() <= 80 {
            let key = edge_key(t);
            if let Some(seen) = edge_pages.get(&key) {
                if is_running_edge(&key, seen, l.page, l.y, pages) {
                    return false;
                }
            }
        }
        !looks_like_page_number(t)
    });
    if all.is_empty() {
        return String::new();
    }

    /* 1.2) 矢量图：图内的碎片文字摘出正文，之后紧跟图题输出成折叠块。
     *      必须在双栏重排、表格识别、标题判定之前做 —— 图里的文字会干扰这三步。 */
    let (owner, figures) = detect_figures(all, meta);
    if owner.iter().any(|o| *o != 0) {
        let mut k = 0usize;
        all.retain(|_| {
            let keep = owner[k] == 0;
            k += 1;
            keep
        });
    }
    let with_text = figures.iter().filter(|f| !f.texts.is_empty()).count();
    if with_text > 0 {
        crate::convert::warn(format!(
            "{} 处图形区域的文字已收进图题下方的折叠块（仅供检索），版式与图形请对照原 PDF",
            with_text
        ));
    }
    if all.is_empty() {
        let mut out = String::new();
        let mut done: HashSet<u32> = HashSet::new();
        for k in 0..figures.len() {
            emit_figure(&mut out, k as u32 + 1, &figures, &mut done);
        }
        return out;
    }

    /* 1.5) 双栏重排：必须在页眉页脚清掉之后做，否则居中的页码会破坏左右聚类 */
    reorder_columns(all);

    /* 1.6) 图形层结构：列表圆点、提示框。必须在双栏重排之后 ——
     *      split_row 会克隆行，重排后再打标才不会丢。 */
    apply_struct(all, meta);

    /* 1.7) 目录 / 图表目录的点线引导符行：先按阅读顺序重排，再交给 toc_entry 结构化 */
    rebuild_toc(all);

    /* 1.8) 不带点线的编号目录行（`1 简介` / `2.1 术语`）→ 列表项 */
    mark_numbered_toc(all);

    /* 1.9) 书签：与目标页上某一行的文字一致，这一行就是标题，层级取书签层级。
     *      字号只能区分三四档，而且同字号的加粗小标题（`1.1 About this Document`）会漏掉；
     *      书签是作者给出的章节结构，比字号可靠得多。页眉页脚区的行不参与，免得章节名页眉抢先认领。 */
    let mut outline = OutlineIndex::new(&meta.outline);
    if outline.total > 0 {
        for l in all.iter_mut() {
            if in_margin(l) || l.quote != 0 || l.fig_caption != 0 {
                continue;
            }
            let t = line_text(l);
            if let Some(level) = outline.claim(l.page, &t) {
                l.outline_level = level;
            }
        }
    }

    /* 2) 正文基准字号：按**字符数**加权投票。
     *    旧实现按行数投票，表格密集的文档里 9pt 表格行会把 body 拉到 9pt，
     *    于是 10.5~11pt 的正文全被当成标题。 */
    let mut hist: HashMap<i64, usize> = HashMap::new();
    let mut fallback: HashMap<i64, usize> = HashMap::new();
    for l in all.iter() {
        let key = (l.size * 2.0).round() as i64;
        *fallback.entry(key).or_insert(0) += 1;
        let n = l.text.chars().count();
        if n < 8 || has_column_jump(l) {
            continue;
        }
        *hist.entry(key).or_insert(0) += n;
    }
    let chosen = if hist.is_empty() { &fallback } else { &hist };
    let body_size = chosen
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(k, _)| (*k as f64) / 2.0)
        .unwrap_or(10.0)
        .max(1.0);

    /* 3) 正文右边界（判断一行是否「排满」）。
     *
     *    这里踩过两个坑：
     *    · 按「本页最宽行」算 → 版权页/章首页整页都是短行，每行都被当成排满，
     *      短句之间被强行接成一大段；
     *    · 按「全文 90 分位」算 → 只要文档里出现**一页比正文更宽的内容**
     *      （旋转页、跨栏标题、超宽表格备注行），分位值就被抬高，
     *      全篇正文行的 ratio 集体掉到 0.80 阈值以下，段段碎裂。
     *
     *    改用**字符数加权的众数桶**：正文满行的右端最集中在某个 10pt 区间，
     *    用它作为版心右边界。离群行（旋转页、超宽表格）权重极低，自然被忽略。 */
    let mut by_col: HashMap<u8, Vec<(f64, usize)>> = HashMap::new();
    for l in all.iter() {
        by_col.entry(l.col).or_default().push((l.x1, l.text.chars().count()));
    }
    let mut col_right: HashMap<u8, f64> = HashMap::new();
    for (c, v) in by_col {
        // 分桶 → (桶内最大 x1, 字符数总和)
        let mut buckets: HashMap<i64, (f64, usize)> = HashMap::new();
        for (x, n) in v {
            let k = (x / 10.0).round() as i64;
            let e = buckets.entry(k).or_insert((x, 0));
            if x > e.0 {
                e.0 = x;
            }
            e.1 += n;
        }
        let best = buckets
            .values()
            .max_by_key(|(_, n)| *n)
            .map(|(x, _)| *x)
            .unwrap_or(1.0);
        col_right.insert(c, best.max(1.0));
    }

    /* 4) 表格：靠列起点网格把绝对定位的文字反推回单元格。支持无表头与跨页续接。 */
    let mut tables: HashMap<usize, (usize, String)> = HashMap::new();
    {
        let mut i = 0;
        // 上一张表的结束位置。向上回扫表头/续行时不得越过它，
        // 否则第二张表会把上一张表的尾部重新吞进来，出现重复行或整表被合并。
        let mut last_table_end = 0usize;
        while i < all.len() {
            let Some(first) = table_cells(&all[i]) else {
                i += 1;
                continue;
            };
            let mut rows = vec![first];
            let mut j = i + 1;
            while j < all.len() && rows.len() < 400 {
                let lj = &all[j];
                if lj.page > all[i].page + 1 {
                    break;
                }
                match table_cells(lj) {
                    Some(c) if same_columns(&rows[0], &c) => {
                        rows.push(c);
                        j += 1;
                    }
                    _ => {
                        // 备注写成两行时，续行只落在最后一列，字段少但仍是这张表的一部分
                        let segs = segments_of(lj);
                        let last_col = rows[0].last().map(|(x, _)| *x).unwrap_or(f64::MAX);
                        let is_tail = segs.len() == 1
                            && segs[0].0 >= last_col - 6.0
                            && segs[0].1.chars().count() <= 120
                            && (lj.size - all[i].size).abs() < 0.6;
                        if is_tail {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            if rows.len() < 2 {
                i += 1;
                continue;
            }

            // ① 表头：紧邻上方、列数比数据行多、左边界对齐的那一行
            let mut start = i;
            let mut header: Option<Vec<(f64, String)>> = None;
            if i > 0 && all[i - 1].page == all[i].page {
                if let Some(h) = table_cells(&all[i - 1]) {
                    if h.len() > rows[0].len() && (h[0].0 - rows[0][0].0).abs() < 8.0 {
                        header = Some(h);
                        start = i - 1;
                    }
                }
            }

            // ② 列网格 = 表头 + 所有数据行的列起点并集，再用它重切每个单元格
            let mut grid: Vec<f64> = Vec::new();
            if let Some(h) = &header {
                for (x, _) in h {
                    push_col(&mut grid, *x);
                }
            }
            for r in &rows {
                for (x, _) in r {
                    push_col(&mut grid, *x);
                }
            }
            grid.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if grid.len() < 2 {
                i += 1;
                continue;
            }
            // 列间距超过版心宽度的 40% 时，这更可能是双栏正文而不是表格 ——
            // 双栏检测万一漏判，也别把正文切成表格。
            let pw = all[i].page_w.max(1.0);
            let widest = grid
                .windows(2)
                .map(|w| w[1] - w[0])
                .fold(0.0f64, |a, b| if b > a { b } else { a });
            if widest > pw * 0.40 {
                i += 1;
                continue;
            }

            // ③ 用列网格向前扫描：行首对准第一列 → 新行；只有中间各列有字 → 上一行的续行
            let head_x = grid[0];
            let header_texts: Option<Vec<String>> =
                header.as_ref().map(|h| h.iter().map(|(_, t)| t.clone()).collect());
            let mut out_rows: Vec<Vec<String>> = Vec::new();

            // ②' 没有表头时，表可能从上一段延续过来：用同一张网格往前补。
            //     自下而上扫描：
            //       · 首列有字、且各段都落在网格上 → 新的一行，认领它下方暂存的续行；
            //       · 只有非首列有字 → 续行，先暂存，等上方的行首来认领；
            //       · 只有首列有字、且与暂存续行处在同一纵向范围 → 列优先排版的首列多行标签
            //         （PDF 先画完左列的 `Unimplemented, / Reserved`，再从同一行顶部画右列说明）。
            //     必须校验 fits_grid：否则上方的正文、表题、页眉都会被网格硬切成「单元格」。
            //     回扫结束时仍没被认领的续行不属于这张表：begin 退回最后一次确认的位置，
            //     把它们留给正文输出 —— 不能被表格范围吞掉而凭空消失。
            let mut begin = start;
            if header.is_none() {
                let mut pend: Vec<(usize, String)> = Vec::new();
                let mut pend_top = f64::MAX;
                let mut pend_bottom = f64::MIN;
                let mut label: Vec<String> = Vec::new();
                let mut back: Vec<Vec<String>> = Vec::new();
                let mut committed = begin;
                while begin > last_table_end {
                    let l = &all[begin - 1];
                    if l.page != all[i].page
                        || (l.size - all[i].size).abs() > 0.8
                        || l.fig_caption != 0
                    {
                        break;
                    }
                    let cells = cut_by_grid(l, &grid);
                    let filled: Vec<usize> =
                        (0..cells.len()).filter(|k| !cells[*k].is_empty()).collect();
                    if filled.is_empty() {
                        break;
                    }
                    if filled[0] == 0 {
                        let fx = l.chars.first().map(|c| c.0).unwrap_or(-1.0);
                        if (fx - grid[0]).abs() > 5.0 {
                            break;
                        }
                        if filled.len() >= 2 {
                            if !fits_grid(l, &grid) {
                                break;
                            }
                            // 下方还有一行列优先的待定行：它的标签与续行已经齐了，先收尾
                            if !label.is_empty() {
                                back.push(label_row(&mut label, &mut pend, grid.len()));
                            }
                            let mut row = row_of(&cells, grid.len());
                            append_pending(&mut row, &mut pend);
                            pend_top = f64::MAX;
                            pend_bottom = f64::MIN;
                            back.push(row);
                            begin -= 1;
                            committed = begin;
                            continue;
                        }
                        let tol = l.size * 0.6;
                        if !pend.is_empty() && l.y >= pend_top - tol && l.y <= pend_bottom + tol {
                            label.push(cells[0].clone());
                            begin -= 1;
                            continue;
                        }
                        break;
                    }
                    // 只有非首列有字：续行。若上方又出现续行，下方的「标签 + 续行」已构成完整一行
                    if !label.is_empty() {
                        back.push(label_row(&mut label, &mut pend, grid.len()));
                        pend_top = f64::MAX;
                        pend_bottom = f64::MIN;
                        committed = begin;
                    }
                    for k in filled {
                        pend.push((k, cells[k].clone()));
                    }
                    pend_top = pend_top.min(l.y);
                    pend_bottom = pend_bottom.max(l.y);
                    begin -= 1;
                }
                if !label.is_empty() {
                    back.push(label_row(&mut label, &mut pend, grid.len()));
                    committed = begin;
                }
                begin = committed;
                back.reverse();
                for (n, r) in back.into_iter().enumerate() {
                    out_rows.insert(n, r);
                }
            }

            let mut end = start;
            let mut cur_page = all[start].page;
            while end < all.len() && out_rows.len() < 400 {
                let l = &all[end];
                // 允许跨一页续接（长表格）；图题不属于表格
                if l.page > cur_page + 1 || (l.size - all[i].size).abs() > 0.8 || l.fig_caption != 0 {
                    break;
                }
                if end == start && header.is_some() {
                    if let Some(h) = &header {
                        out_rows.push(h.iter().map(|(_, t)| t.clone()).collect());
                    }
                    end += 1;
                    continue;
                }
                let cells = cut_by_grid(l, &grid);
                // 跨页后重复出现的表头：跳过，不要当成数据行
                if l.page != cur_page {
                    if let Some(h) = &header_texts {
                        let same = cells.len() == h.len()
                            && cells
                                .iter()
                                .zip(h.iter())
                                .all(|(a, b)| a.trim() == b.trim());
                        if same {
                            cur_page = l.page;
                            end += 1;
                            continue;
                        }
                    }
                    cur_page = l.page;
                }
                let filled: Vec<usize> =
                    (0..cells.len()).filter(|k| !cells[*k].is_empty()).collect();
                if filled.is_empty() {
                    break;
                }
                if filled[0] == 0 {
                    let fx = l.chars.first().map(|c| c.0).unwrap_or(-1.0);
                    if (fx - head_x).abs() > 5.0 {
                        break;
                    }
                    if filled.len() < 2 || !fits_grid(l, &grid) {
                        // 列优先排版的一行：首列多行标签在前，右列说明随后从同一行顶部开始
                        let n = if filled.len() == 1 {
                            column_first_labels(all, end, &grid, head_x, all[i].size)
                        } else {
                            0
                        };
                        if n == 0 {
                            break;
                        }
                        let mut row = vec![String::new(); grid.len()];
                        for li in end..end + n {
                            let c = cut_by_grid(&all[li], &grid);
                            join_cell(&mut row[0], &c[0]);
                        }
                        out_rows.push(row);
                        end += n;
                        continue;
                    }
                    out_rows.push(row_of(&cells, grid.len()));
                } else {
                    let Some(row) = out_rows.last_mut() else { break };
                    for k in filled {
                        if let Some(slot) = row.get_mut(k) {
                            join_cell(slot, &cells[k]);
                        }
                    }
                }
                end += 1;
            }
            if out_rows.len() < 2 {
                i += 1;
                continue;
            }
            tables.insert(begin, (end - 1, render_table(&out_rows)));
            last_table_end = end;
            i = end;
        }
    }

    /* 4.5) 自适应行距倍数（判断纵向间距是否仍是「同一段内的续行」）。
     *
     *    段内行距与段间距往往只差 15~25%，写死倍数会在不同排版上互相踩雷：
     *    实测一本中文书段内 2.4 倍、段间 2.9 倍 —— 取 2.2 会把每一行都断开（段落数暴涨 2.3 倍），
     *    取 3.2 又会把换段合并。
     *    改为用文档自身「同页相邻行间距 / 字号」的中位数估计段内行距，再留 18% 余量。 */
    let gap_factor = {
        let override_gap = std::env::var("MD_PDF_GAP")
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());
        let mut v: Vec<f64> = Vec::new();
        for w in all.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if a.page != b.page || a.col != b.col {
                continue;
            }
            if (a.size - b.size).abs() > a.size * 0.15 {
                continue;
            }
            let dy = b.y - a.y;
            if dy <= 0.0 {
                continue;
            }
            let r = dy / a.size.max(1.0);
            // 只取「像行距」的样本：跨栏/跨节的巨大间距不进统计
            if (0.5..=6.0).contains(&r) {
                v.push(r);
            }
        }
        match override_gap {
            Some(g) => g,
            None if v.len() < 20 => 2.8,
            None => {
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                (v[v.len() / 2] * 1.18).clamp(1.3, 3.6)
            }
        }
    };

    /* 5) 合并断行 */
    let mut blocks: Vec<Block> = Vec::new();
    let mut idx = 0;
    while idx < all.len() {
        if let Some((end, md)) = tables.get(&idx) {
            blocks.push(Block {
                page: all[idx].page,
                y: all[idx].y,
                size: 0.0,
                text: md.clone(),
                heading: false,
                right_ratio: 1.0,
                pre: true,
                left: 0.0,
                page_end: all[idx].page,
                multi: false,
                last_y: all[idx].y,
                bullet: false,
                quote: 0,
                quote_head: false,
                quote_code: false,
                level: 0,
                fig: 0,
            });
            idx = end + 1;
            continue;
        }
        let l = &all[idx];
        idx += 1;
        let t = line_text(l);
        if t.is_empty() {
            continue;
        }
        let body_right = col_right.get(&l.col).copied().unwrap_or(1.0).max(1.0);
        let ratio = (l.x1 / body_right).clamp(0.0, 1.0);
        let left = l.x0;
        let multi = has_column_jump(l);
        if let Some(prev) = blocks.last_mut() {
            let ek = end_kind(&prev.text);
            // 硬换行修复：只有「上一行排满」且没有明确句末时，才认为是被排版切断
            let allow_by_end = match ek {
                EndKind::Hard => false,
                EndKind::Soft => prev.right_ratio >= 0.92,
                EndKind::Open => prev.right_ratio >= 0.80,
            };
            let same_page = prev.page == l.page;
            let next_page = l.page == prev.page + 1
                && prev.right_ratio >= 0.90
                && (left - prev.left).abs() < 3.0;
            // 纵向间距必须是「正常行距」才算续行。
            // 只看「上一行排满」不够：段间空行、图上下的独立短句、以及旋转页里
            // 两行相距 100pt 的情况（实测被误接成一段）都会踩雷。
            // gap_factor 由第 4.5 步从文档自身估计（段内行距中位数 × 1.18）。
            let line_gap = if same_page { l.y - prev.last_y } else { 0.0 };
            let gap_ok = !same_page || (line_gap >= -l.size * 0.2 && line_gap <= l.size * gap_factor);
            /* 提示框内部：框的宽度才是「排满」的基准，用整页版心去判断永远判不满。
             * 同一框内的相邻行只要求纵向间距正常，就认定是同一条。
             * 进出框（编号变化）一律不合并。 */
            let same_box = l.quote != 0 && prev.quote == l.quote;
            /* 列表项续行必须保持在列表缩进上。
             * 缩进回退到正文左边距，说明列表已经结束、后面是新段落 ——
             * 这条只在列表块上生效：正文段落的「首行缩进」同样是向右偏移，
             * 对普通段落套用会误伤。 */
            let indent_ok = !prev.bullet || left >= prev.left - 2.0;
            let can_merge = !prev.heading
                && !prev.pre
                && !prev.multi
                && !multi
                && !l.bullet
                && !prev.quote_head
                && !prev.quote_code
                && prev.quote == l.quote
                && indent_ok
                && (same_page || next_page)
                && gap_ok
                && (allow_by_end || same_box)
                && !prev.text.contains("....")
                && toc_entry(&prev.text).is_none()
                && toc_entry(&t).is_none()
                && bullet_prefix(&t).is_none()
                // 书签标题、图题各自独立成块
                && prev.level == 0
                && l.outline_level == 0
                && prev.fig == 0
                && l.fig_caption == 0
                && l.size <= prev.size * 1.08
                && l.size * 1.12 >= prev.size;
            if can_merge {
                let last_c = prev.text.chars().last();
                let first_c = t.chars().next();
                if prev.text.ends_with('-') && first_c.map(|c| c.is_lowercase()).unwrap_or(false) {
                    prev.text.pop();
                    prev.text.push_str(&t);
                } else if let (Some(a), Some(b)) = (last_c, first_c) {
                    let need_space =
                        !is_wide(a) && !is_wide(b) && !a.is_whitespace() && !b.is_whitespace();
                    if need_space {
                        prev.text.push(' ');
                    }
                    prev.text.push_str(&t);
                } else {
                    prev.text.push_str(&t);
                }
                prev.right_ratio = ratio;
                prev.size = prev.size.max(l.size);
                prev.last_y = l.y;
                if l.page > prev.page_end {
                    prev.page_end = l.page;
                }
                continue;
            }
        }
        blocks.push(Block {
            page: l.page,
            y: l.y,
            size: l.size,
            text: t,
            heading: false,
            right_ratio: ratio,
            pre: false,
            left,
            page_end: l.page,
            multi,
            last_y: l.y,
            bullet: l.bullet,
            quote: l.quote,
            quote_head: l.quote_head,
            quote_code: l.quote_code,
            level: l.outline_level,
            fig: l.fig_caption,
        });
    }

    // 书签标题在 PDF 里折成了两行时，合并之后按整块再比一次
    if outline.total > 0 {
        for b in blocks.iter_mut() {
            if b.pre || b.level != 0 || b.quote != 0 || b.fig != 0 {
                continue;
            }
            if let Some(level) = outline.claim(b.page, &b.text) {
                b.level = level;
            }
        }
    }
    // 书签与正文对不上（书签另起了名字、目标页整体错位）时整体放弃，退回按字号判断
    let use_outline = outline.trusted();
    if !use_outline {
        for b in blocks.iter_mut() {
            b.level = 0;
        }
    }

    // 标题判定放在合并之后：合并会把短行接成长段，先用合并后的长度做过滤更准
    for b in blocks.iter_mut() {
        if b.pre {
            continue;
        }
        if b.level != 0 {
            b.heading = true;
            continue;
        }
        let n = b.text.chars().count();
        b.heading = b.size >= body_size * 1.12
            && n <= 100
            && !b.multi
            && !b.bullet
            && b.quote == 0
            && b.fig == 0
            && toc_entry(&b.text).is_none()
            && bullet_prefix(&b.text).is_none()
            && heading_text_ok(&b.text, use_outline);
    }

    // 标题层级：先按字号倍率定 1~4 档，再把实际用到的档位压成连续层级，
    // 否则字号分布不巧时会出现「# 之后直接 ###」的跳级。书签标题的层级直接取书签，不参与。
    let mut head_levels: Vec<u8> = blocks
        .iter()
        .filter(|b| b.heading && b.level == 0)
        .map(|b| raw_level_of(b.size, body_size))
        .collect();
    head_levels.sort_unstable();
    head_levels.dedup();

    /* 6) 输出：图片按纵向位置插到对应段落之间 */
    let mut out = String::new();
    let mut cur_page = blocks.first().map(|b| b.page).unwrap_or(1);
    let mut last_end = cur_page;
    let mut done: HashSet<u32> = HashSet::new();
    let mut img_cursor: HashMap<u32, usize> = HashMap::new();
    let mut in_list = false;
    let mut cur_quote = 0u32;
    let mut fence_open = false;
    let mut figs_done: HashSet<u32> = HashSet::new();
    // 最近一个书签标题的层级：书签之外的标题挂在它下面一级
    let mut last_outline_level: u8 = 0;

    for b in blocks {
        // 离开代码框：收围栏。必须**最先**做 —— 后面还有整页图片的刷新，
        // 晚一步收围栏，图片就会被写进代码块里。
        if fence_open && b.quote != cur_quote {
            out.push_str("```\n\n");
            fence_open = false;
        }
        while cur_page < b.page {
            flush_page_images(&mut out, cur_page, images, &mut img_cursor, &mut done);
            if !fence_open {
                flush_page_figures(&mut out, cur_page, &figures, &mut figs_done);
            }
            cur_page += 1;
        }
        if b.page_end > last_end {
            last_end = b.page_end;
        }
        if !fence_open {
            // 先吐出纵向位置在这一块之上的图片
            emit_images_before(&mut out, b.page, b.y, images, &mut img_cursor);
        }
        // 离开提示框
        if b.quote == 0 && cur_quote != 0 {
            out.push('\n');
            cur_quote = 0;
        }

        if b.pre {
            if in_list {
                out.push('\n');
                in_list = false;
            }
            out.push_str(&b.text);
            out.push('\n');
            continue;
        }
        let text = b.text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        // 代码框（「示例」类）：保留原换行，输出成围栏代码块
        if b.quote != 0 && b.quote_code {
            if in_list {
                out.push('\n');
                in_list = false;
            }
            cur_quote = b.quote;
            if b.quote_head {
                out.push_str(&format!("**{}**\n\n```\n", text));
                fence_open = true;
            } else {
                if !fence_open {
                    out.push_str("```\n");
                    fence_open = true;
                }
                out.push_str(&text);
                out.push('\n');
            }
            continue;
        }

        // 提示框 / 注意框：输出成引用块，框标题单独加粗
        if b.quote != 0 {
            if in_list {
                out.push('\n');
                in_list = false;
            }
            if cur_quote != b.quote {
                cur_quote = b.quote;
            }
            if b.quote_head {
                out.push_str(&format!("> **{}**\n>\n", text));
            } else {
                out.push_str(&format!("> {}\n", text));
            }
            continue;
        }

        // 矢量图的图题：照常输出图题，紧跟着放图内文字的折叠块
        if b.fig != 0 {
            if in_list {
                out.push('\n');
                in_list = false;
            }
            out.push_str(&format!("{}\n\n", text));
            emit_figure(&mut out, b.fig, &figures, &mut figs_done);
            continue;
        }

        // 目录行
        if !b.heading {
            if let Some((title, page)) = toc_entry(&text) {
                if in_list {
                    out.push('\n');
                }
                out.push_str(&format!("- {} · {}\n", title, page));
                in_list = true;
                continue;
            }
        }

        if b.heading {
            if in_list {
                out.push('\n');
                in_list = false;
            }
            let level = if b.level != 0 {
                last_outline_level = b.level;
                (b.level as usize).min(6)
            } else if use_outline {
                (last_outline_level as usize + 1).min(6)
            } else {
                head_levels
                    .iter()
                    .position(|r| *r == raw_level_of(b.size, body_size))
                    .map(|i| i + 1)
                    .unwrap_or(1)
                    .min(6)
            };
            out.push_str(&format!("{} {}\n\n", "#".repeat(level), text));
            continue;
        }

        // 一行里塞了多个项目符号：拆成「前导句 + 若干列表项」
        let parts = split_inline_bullets(&text);
        if parts.len() > 1 {
            if in_list {
                out.push('\n');
            }
            let starts_with_mark = text.chars().next().map(is_bullet_char).unwrap_or(false);
            let mut any_bullet = false;
            for (k, p) in parts.iter().enumerate() {
                if k == 0 && !starts_with_mark {
                    out.push_str(&format!("{}\n\n", p));
                } else {
                    out.push_str(&format!("- {}\n", p));
                    any_bullet = true;
                }
            }
            in_list = any_bullet;
            continue;
        }

        let bullet = bullet_prefix(&text);
        if in_list && bullet.is_none() {
            out.push('\n');
            in_list = false;
        }
        if let Some(item) = bullet {
            out.push_str(&format!("{}\n", item));
            in_list = true;
            continue;
        }
        // 图形层的圆点（PDF 里画成矢量圆，文字层完全拿不到）→ 补成列表项
        if b.bullet {
            out.push_str(&format!("- {}\n", text));
            in_list = true;
            continue;
        }
        out.push_str(&format!("{}\n\n", text));
    }

    // 文档在代码框里结束时也要收围栏
    if fence_open {
        out.push_str("```\n\n");
    }
    while cur_page <= last_end {
        flush_page_images(&mut out, cur_page, images, &mut img_cursor, &mut done);
        flush_page_figures(&mut out, cur_page, &figures, &mut figs_done);
        cur_page += 1;
    }
    // 整页只有图、没有文字的页面：追加到末尾，别把内容整页丢掉
    let mut rest: Vec<u32> = images.keys().copied().filter(|p| !done.contains(p)).collect();
    rest.sort_unstable();
    for p in rest {
        emit_images_before(&mut out, p, f64::MAX, images, &mut img_cursor);
        done.insert(p);
    }
    // 图题没能输出、所在页也没轮到刷新的图：最后兜底，图内文字绝不丢
    for k in 0..figures.len() {
        emit_figure(&mut out, k as u32 + 1, &figures, &mut figs_done);
    }
    out
}

/// 输出该页中纵向位置在 `before_y` 之前的图片
fn emit_images_before(
    out: &mut String,
    page: u32,
    before_y: f64,
    images: &HashMap<u32, Vec<(f64, String)>>,
    cursor: &mut HashMap<u32, usize>,
) {
    let Some(list) = images.get(&page) else { return };
    let i = cursor.entry(page).or_insert(0);
    while *i < list.len() {
        if list[*i].0 > before_y {
            break;
        }
        out.push_str(&format!("![]({})\n\n", list[*i].1));
        *i += 1;
    }
}

/// 收尾时把该页剩下的图全部吐出
fn flush_page_images(
    out: &mut String,
    page: u32,
    images: &HashMap<u32, Vec<(f64, String)>>,
    cursor: &mut HashMap<u32, usize>,
    done: &mut HashSet<u32>,
) {
    if let Some(list) = images.get(&page) {
        let i = cursor.entry(page).or_insert(0);
        while *i < list.len() {
            out.push_str(&format!("![]({})\n\n", list[*i].1));
            *i += 1;
        }
    }
    done.insert(page);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    /// 构造一行：每个片段 (x, 文本) 的字符按 0.5em 等宽排布
    fn line(page: u32, y: f64, size: f64, segs: &[(f64, &str)]) -> Line {
        let mut l = Line {
            page,
            y,
            page_w: 595.0,
            page_h: 842.0,
            x0: f64::MAX,
            x1: f64::MIN,
            size,
            text: String::new(),
            segs: Vec::new(),
            chars: Vec::new(),
            col: 0,
            bullet: false,
            quote: 0,
            quote_head: false,
            quote_code: false,
            fig_caption: 0,
            outline_level: 0,
        };
        for (x, t) in segs {
            let mut cx = *x;
            for ch in t.chars() {
                l.chars.push((cx, ch));
                cx += size * 0.5;
            }
            l.segs.push(Seg { x: *x, end: cx, text: t.to_string() });
            if !l.text.is_empty() {
                l.text.push(' ');
            }
            l.text.push_str(t);
            l.x0 = l.x0.min(*x);
            l.x1 = l.x1.max(cx);
        }
        l
    }

    fn convert_lines(mut lines: Vec<Line>) -> String {
        let images: HashMap<u32, Vec<(f64, String)>> = HashMap::new();
        let meta = PageMeta::default();
        lines_to_md(&mut lines, &images, &meta)
    }

    #[test]
    fn keeps_space_carried_by_previous_fragment() {
        let mut l = line(1, 100.0, 11.0, &[(42.0, "microcontrollers ")]);
        let end = l.segs[0].end;
        l.segs.push(Seg { x: end, end: end + 40.0, text: "based on".to_string() });
        assert_eq!(line_text(&l), "microcontrollers based on");
    }

    #[test]
    fn column_first_table_row_keeps_every_line() {
        // 第 40 页表 2：左列两行标签先画完，右列说明再从同一行顶部开始
        let out = convert_lines(vec![
            line(40, 183.9, 11.0, &[(42.6, "Table 2"), (90.0, "Bit Function Terminology")]),
            line(40, 201.9, 11.0, &[(42.6, "Function of Bits"), (174.1, "Description")]),
            line(40, 219.4, 11.0, &[(42.6, "Unimplemented,")]),
            line(40, 233.4, 11.0, &[(42.6, "Reserved")]),
            line(
                40,
                219.4,
                11.0,
                &[(174.1, "Register bit fields named 0 indicate unimplemented functions with the following")],
            ),
            line(40, 233.4, 11.0, &[(174.1, "behavior:")]),
            line(40, 250.4, 11.0, &[(174.1, "•Reading these bit fields returns 0.")]),
            line(40, 301.3, 11.0, &[(174.1, "These bit fields are reserved.")]),
            line(40, 332.8, 11.0, &[(42.6, "rw"), (174.1, "The bit or bit field can be read and written.")]),
            line(
                40,
                350.3,
                11.0,
                &[(42.6, "rwh"), (174.1, "As rw, but bit or bit field can be also set or reset by hardware.")],
            ),
            line(40, 367.9, 11.0, &[(42.6, "r"), (174.1, "The bit or bit field can only be read (read-only).")]),
        ]);
        assert!(out.contains("Table 2 Bit Function Terminology\n\n| Function of Bits | Description |"), "{}", out);
        assert!(
            out.contains(
                "| Unimplemented, Reserved | Register bit fields named 0 indicate unimplemented functions \
                 with the following behavior:<br>• Reading these bit fields returns 0. \
                 These bit fields are reserved. |"
            ),
            "{}",
            out
        );
        assert!(out.contains("| rw | The bit or bit field can be read and written. |"), "{}", out);
    }

    #[test]
    fn paragraph_above_table_is_not_cut_into_cells() {
        let out = convert_lines(vec![
            line(42, 260.0, 11.0, &[(42.5, "From application point of view the register reset value is the initial content.")]),
            line(42, 280.0, 11.0, &[(42.5, "Symbol"), (131.0, "Description")]),
            line(42, 298.0, 11.0, &[(42.5, "nBE"), (131.0, "Indicates that no Bus Error is generated.")]),
            line(42, 316.0, 11.0, &[(42.5, "nE"), (131.0, "Indicates that no Error is generated.")]),
        ]);
        assert!(
            out.starts_with(
                "From application point of view the register reset value is the initial content.\n\n\
                 | Symbol | Description |"
            ),
            "{}",
            out
        );
        assert!(out.contains("| nBE | Indicates that no Bus Error is generated. |"), "{}", out);
    }

    #[test]
    fn toc_handles_spaced_leaders_wrapped_labels_and_missing_leaders() {
        let mut all = vec![
            line(25, 700.0, 11.0, &[(42.5, "19.2.8.8.2 Data Read Sequence in Integration Mode  . . . . . . . .  19-17")]),
            line(25, 714.0, 11.0, &[(42.5, "19.2.8.8.3 Data Mapping for 4 Antennae  . ")]),
            line(25, 728.0, 11.0, &[(104.9, "19-18")]),
            line(25, 742.0, 11.0, &[(42.5, "25.2.1.2 Delta-sigma Modulator enabled  25-3")]),
            line(25, 756.0, 11.0, &[(42.5, "19.2.8.9 3rd stage FFT . . . . . . . 19-20")]),
        ];
        rebuild_toc(&mut all);
        let got: Vec<Option<(String, String)>> = all.iter().map(|l| toc_entry(&l.text)).collect();
        let want = |t: &str, p: &str| Some((t.to_string(), p.to_string()));
        assert_eq!(
            got,
            vec![
                want("19.2.8.8.2 Data Read Sequence in Integration Mode", "19-17"),
                want("19.2.8.8.3 Data Mapping for 4 Antennae", "19-18"),
                want("25.2.1.2 Delta-sigma Modulator enabled", "25-3"),
                want("19.2.8.9 3rd stage FFT", "19-20"),
            ]
        );
    }

    #[test]
    fn toc_keeps_entries_that_share_one_line() {
        let mut all = vec![line(
            3,
            100.0,
            11.0,
            &[(42.0, "图 2-1AUTOSAR架构 4.3.0.......13图 2-2BSW卫星副本与代理.......14")],
        )];
        rebuild_toc(&mut all);
        let got: Vec<(String, String)> = all.iter().filter_map(|l| toc_entry(&l.text)).collect();
        assert_eq!(
            got,
            vec![
                ("图 2-1AUTOSAR架构 4.3.0".to_string(), "13".to_string()),
                ("图 2-2BSW卫星副本与代理".to_string(), "14".to_string()),
            ]
        );
    }

    #[test]
    fn page_labels_and_leaders() {
        assert!(is_page_label("1-52") && is_page_label("TOC-1") && is_page_label("Preface-1"));
        assert!(!is_page_label("2nd") && !is_page_label("16bit") && !is_page_label("V1.5.0"));
        assert!(has_leader("About this document. . . . . Preface-1"));
        assert!(!has_leader("Version 1.1.1 e.g. text"));
    }

    #[test]
    fn running_edges_need_same_position_on_neighbour_pages() {
        assert_eq!(edge_key("User’s Manual 1-3 V1.5.0 "), "User’s Manual #-# V#.#.#");
        let mut footer: BTreeMap<u32, f64> = BTreeMap::new();
        for p in 38..=43 {
            footer.insert(p, 825.4);
        }
        assert!(is_running_edge("Introduction", &footer, 40, 825.4, 1942));
        // 同样文字的脚注，高度随正文浮动：不是页脚
        let mut note: BTreeMap<u32, f64> = BTreeMap::new();
        note.insert(39, 771.0);
        note.insert(40, 783.0);
        note.insert(41, 790.0);
        assert!(!is_running_edge("#) Not available", &note, 40, 783.0, 1942));
    }

    #[test]
    fn decodes_pdf_text_strings() {
        assert_eq!(decode_pdf_text(&[0xFE, 0xFF, 0x00, 0x41, 0x21, 0x22]), "A™");
        assert_eq!(decode_pdf_text(b"AURIX\x92 TC3xx"), "AURIX™ TC3xx");
    }

    #[test]
    fn recognises_figure_captions_only() {
        assert_eq!(caption_rest("Figure 85 System PLL Block Diagram"), Some("System PLL Block Diagram"));
        assert_eq!(caption_rest("Figure 1"), Some(""));
        assert_eq!(caption_rest("Figure3D model"), None);
        assert_eq!(caption_rest("Table 6 Reset Values"), None);
    }

    #[test]
    fn headings_need_real_words() {
        assert!(!heading_text_ok("K", false));
        assert!(!heading_text_ok("96", false));
        assert!(!heading_text_ok("SA –", true));
        assert!(heading_text_ok("序", true));
        assert!(heading_text_ok("Overview", true));
    }

    /// 中文正文的省略号是两个 `…`。按「每个 `…` 算 3 点」会凑满 4 点阈值，
    /// 整行被当成目录行重排，省略号在重排中消失（实测书稿丢了几十处）。
    #[test]
    fn cjk_ellipsis_is_not_a_toc_leader() {
        assert!(!has_leader("当美好的事情发生时能够庆祝……我便能掌握我的力量。"));
        assert!(!has_leader("消费主义、碎片化的信息、竞争比较……"));
        assert!(!has_leader("他向我提出了离婚……"));
        // 真正的引导符仍然认
        assert!(has_leader("About this document . . . . Preface-1"));
        assert!(has_leader("1 Introduction ........ 1-1"));

        let mut all = vec![line(
            1,
            100.0,
            11.0,
            &[(42.0, "他向我提出了离婚……长话短说，今天我们得以一起来到这里。")],
        )];
        rebuild_toc(&mut all);
        assert_eq!(all.len(), 1);
        assert!(
            all[0].text.contains("离婚……长话短说"),
            "省略号被吃掉了：{}",
            all[0].text
        );
    }
}
