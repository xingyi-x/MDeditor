/* ══════════════ PDF 图片提取：滤镜解码 / 色彩空间 / 落盘 ══════════════
 * 从 pdf.rs 整体拆出，纯重构，行为不变。
 * 依赖 pdf.rs 暴露的 obj_num / resolve / is_ws / PageMeta。
 */
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use super::pdf::{is_ws, obj_num, resolve, PageMeta};

/* ────────── 图片提取 ──────────
 * lopdf 的 decompressed_content() 对 Subtype=Image 的流直接返回 Err，
 * 所以图片必须自己 inflate：FlateDecode → zlib 解压 →（可选 PNG 预测器还原）→ 重新编码 PNG。
 * DCTDecode（JPEG）的流本身就是成品，直接落盘。
 */

const MIN_W: u32 = 60;
const MIN_H: u32 = 40;

fn filter_list(dict: &lopdf::Dictionary) -> Vec<String> {
    match dict.get(b"Filter") {
        Ok(lopdf::Object::Name(n)) => vec![String::from_utf8_lossy(n).into_owned()],
        Ok(lopdf::Object::Array(a)) => a
            .iter()
            .filter_map(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// 图片的像素格式
#[derive(Clone)]
enum Spec {
    Gray,
    Rgb,
    Cmyk,
    /// 调色板：基准色分量数 + 上层色表
    Indexed { base: usize, lookup: Vec<u8> },
}

impl Spec {
    fn components(&self) -> usize {
        match self {
            Spec::Gray => 1,
            Spec::Rgb => 3,
            Spec::Cmyk => 4,
            Spec::Indexed { .. } => 1,
        }
    }
}

fn lookup_bytes(doc: &lopdf::Document, o: &lopdf::Object) -> Vec<u8> {
    let o = resolve(doc, o);
    match o {
        lopdf::Object::Stream(s) => match s.decompressed_content() {
            Ok(v) => v,
            Err(_) => inflate(&s.content).unwrap_or_else(|| s.content.clone()),
        },
        lopdf::Object::String(s, _) => s.clone(),
        _ => Vec::new(),
    }
}

fn color_spec(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<Spec> {
    let cs = dict.get(b"ColorSpace").ok()?;
    let cs = resolve(doc, cs);
    match cs {
        lopdf::Object::Name(n) => match n.as_slice() {
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(Spec::Rgb),
            b"DeviceGray" | b"G" | b"CalGray" => Some(Spec::Gray),
            b"DeviceCMYK" | b"CMYK" => Some(Spec::Cmyk),
            // 命名色彩空间不在字典里写时无从判断，按 RGB 猜（多数 PDF 如此）
            _ => Some(Spec::Rgb),
        },
        lopdf::Object::Array(a) => {
            let head = a
                .first()
                .and_then(|o| o.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .unwrap_or_default();
            match head.as_str() {
                "ICCBased" => {
                    let id = a.get(1).and_then(|o| o.as_reference().ok())?;
                    let s = doc.get_object(id).ok()?.as_stream().ok()?;
                    match s.dict.get(b"N").ok().and_then(obj_num) {
                        Some(1.0) => Some(Spec::Gray),
                        Some(4.0) => Some(Spec::Cmyk),
                        _ => Some(Spec::Rgb),
                    }
                }
                "Indexed" | "I" => {
                    let base = match a.get(1).map(|o| resolve(doc, o)) {
                        Some(lopdf::Object::Name(n)) => match n.as_slice() {
                            b"DeviceGray" | b"G" => 1,
                            b"DeviceCMYK" | b"CMYK" => 4,
                            _ => 3,
                        },
                        _ => 3,
                    };
                    let hival = a.get(2).and_then(obj_num).unwrap_or(255.0) as usize;
                    let lookup = a.get(3).map(|o| lookup_bytes(doc, o)).unwrap_or_default();
                    Some(Spec::Indexed { base, lookup: lookup[..lookup.len().min((hival + 1) * base)].to_vec() })
                }
                "CalRGB" | "DeviceRGB" => Some(Spec::Rgb),
                "CalGray" | "DeviceGray" => Some(Spec::Gray),
                "Separation" | "DeviceN" => Some(Spec::Gray),
                _ => Some(Spec::Rgb),
            }
        }
        _ => Some(Spec::Rgb),
    }
}

/// /DecodeParms 允许是字典、字典数组，也允许是**间接引用**。
/// 旧实现遇到 Reference 直接返回 None，PNG predictor 因此整体失效。
fn decode_parms(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<lopdf::Dictionary> {
    let o = dict.get(b"DecodeParms").or_else(|_| dict.get(b"DP")).ok()?;
    let o = resolve(doc, o);
    let pick = |x: &lopdf::Object| match resolve(doc, x) {
        lopdf::Object::Dictionary(d) => Some(d.clone()),
        _ => None,
    };
    match o {
        lopdf::Object::Dictionary(d) => Some(d.clone()),
        lopdf::Object::Array(a) => a.iter().find_map(pick),
        _ => None,
    }
}

fn predictor_of(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> i64 {
    decode_parms(doc, dict)
        .and_then(|d| d.get(b"Predictor").ok().and_then(obj_num))
        .unwrap_or(1.0) as i64
}

/// LZW 的 EarlyChange（默认 1）：编码表长度达到 2^width - 1 时提前加宽码长
fn early_change(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> usize {
    match decode_parms(doc, dict)
        .and_then(|d| d.get(b"EarlyChange").ok().and_then(obj_num))
    {
        Some(v) if v <= 0.0 => 0,
        _ => 1,
    }
}

/// 恶意或畸形 PDF 的解压上限：不设上限时一个几十 KB 的流就能吃光内存，
/// 而且 `catch_unwind` 拦不住 OOM abort。
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;
/// 单张图最多 5000 万像素（约 150MB RGB 缓冲）
const MAX_IMAGE_PIXELS: u64 = 50_000_000;

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    // 先按 zlib 解（带 header），失败再按 raw deflate 试。
    // take() 限制解压后的字节数，避免 zip bomb。
    for raw in [false, true] {
        let mut out = Vec::new();
        let r = if raw {
            flate2::read::DeflateDecoder::new(data)
                .take(MAX_DECODED_BYTES)
                .read_to_end(&mut out)
        } else {
            flate2::read::ZlibDecoder::new(data)
                .take(MAX_DECODED_BYTES)
                .read_to_end(&mut out)
        };
        if r.is_ok() || !out.is_empty() {
            if !out.is_empty() {
                return Some(out);
            }
        }
    }
    None
}

/// ASCII85Decode（Adobe 变体）：5 个可见字符 → 4 字节，`z` 是 4 个零，`~>` 结束
fn ascii85_decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut group: Vec<u8> = Vec::with_capacity(5);
    let mut ended = false;
    for &b in data {
        if ended {
            break;
        }
        if b == b'~' {
            ended = true;
            continue;
        }
        if is_ws(b) {
            continue;
        }
        if b == b'z' && group.is_empty() {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            return None;
        }
        group.push(b - b'!');
        if group.len() == 5 {
            let mut v: u32 = 0;
            for &d in &group {
                v = v.wrapping_mul(85).wrapping_add(d as u32);
            }
            out.extend_from_slice(&v.to_be_bytes());
            group.clear();
        }
    }
    if !group.is_empty() {
        let n = group.len();
        while group.len() < 5 {
            group.push(84); // 用 'u' 补齐
        }
        let mut v: u32 = 0;
        for &d in &group {
            v = v.wrapping_mul(85).wrapping_add(d as u32);
        }
        let bytes = v.to_be_bytes();
        out.extend_from_slice(&bytes[..n - 1]);
    }
    Some(out)
}

/// LZWDecode（PDF 变体，MSB-first 变长码，ClearCode=256 / EOD=257）
fn lzw_decode(data: &[u8], early: usize) -> Option<Vec<u8>> {
    const CLEAR: u16 = 256;
    const EOD: u16 = 257;
    fn reset(table: &mut Vec<Vec<u8>>) {
        table.clear();
        for i in 0..256u16 {
            table.push(vec![i as u8]);
        }
        table.push(Vec::new()); // CLEAR
        table.push(Vec::new()); // EOD
    }
    let mut table: Vec<Vec<u8>> = Vec::new();
    reset(&mut table);
    let mut width = 9usize;
    let mut bitbuf: u32 = 0;
    let mut bits = 0usize;
    let mut prev: Option<Vec<u8>> = None;
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() || bits >= width {
        while bits < width && pos < data.len() {
            bitbuf = (bitbuf << 8) | data[pos] as u32;
            bits += 8;
            pos += 1;
        }
        if bits < width {
            break;
        }
        let code = ((bitbuf >> (bits - width)) & ((1u32 << width) - 1)) as u16;
        bits -= width;
        bitbuf = if bits == 0 { 0 } else { bitbuf & ((1u32 << bits) - 1) };
        if code == CLEAR {
            reset(&mut table);
            width = 9;
            prev = None;
            continue;
        }
        if code == EOD {
            break;
        }
        let entry: Vec<u8> = match table.get(code as usize) {
            Some(e) if !e.is_empty() => e.clone(),
            Some(_) => return None,
            None => {
                let p = prev.as_ref()?;
                let mut e = p.clone();
                e.push(p[0]);
                e
            }
        };
        out.extend_from_slice(&entry);
        if let Some(p) = &prev {
            let mut e = p.clone();
            e.push(entry[0]);
            table.push(e);
        }
        prev = Some(entry);
        if table.len() + early >= (1usize << width) && width < 12 {
            width += 1;
        }
        if out.len() as u64 > MAX_DECODED_BYTES {
            return None;
        }
    }
    Some(out)
}

/// RunLengthDecode：长度字节 <128 表示复制 n+1 个字面字节，>128 表示重复 257-n 次
fn run_length_decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let n = data[i];
        i += 1;
        if n == 128 {
            break;
        }
        if n < 128 {
            let k = n as usize + 1;
            if i + k > data.len() {
                return None;
            }
            out.extend_from_slice(&data[i..i + k]);
            i += k;
        } else {
            let k = 257 - n as usize;
            let Some(&b) = data.get(i) else { return None };
            i += 1;
            out.extend(std::iter::repeat(b).take(k));
        }
        if out.len() as u64 > MAX_DECODED_BYTES {
            return None;
        }
    }
    Some(out)
}

/// 还原 PNG 预测器（Predictor >= 10）的行滤波。
/// `row_bytes` 是每行字节数（低位深时必须按位算，不能拿 w*n）。
fn undo_predictor(data: &[u8], row_bytes: usize, h: usize, colors: usize, bpc: usize, pred: i64) -> Option<Vec<u8>> {
    if pred < 10 {
        return Some(data[..data.len().min(row_bytes * h)].to_vec());
    }
    if row_bytes == 0 || (row_bytes + 1) * h > data.len() {
        return None;
    }
    let bpp = ((colors * bpc) + 7) / 8;
    let bpp = bpp.max(1);
    let mut out = vec![0u8; row_bytes * h];
    let mut prev = vec![0u8; row_bytes];
    for y in 0..h {
        let base = y * (row_bytes + 1);
        let ft = data[base];
        let src = &data[base + 1..base + 1 + row_bytes];
        let mut cur = vec![0u8; row_bytes];
        for i in 0..row_bytes {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let x = src[i];
            cur[i] = match ft {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => {
                    let p = a as i32 + b as i32 - c as i32;
                    let pa = (p - a as i32).abs();
                    let pb = (p - b as i32).abs();
                    let pc = (p - c as i32).abs();
                    let pr = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    x.wrapping_add(pr)
                }
                _ => x,
            };
        }
        out[y * row_bytes..y * row_bytes + row_bytes].copy_from_slice(&cur);
        prev = cur;
    }
    Some(out)
}

/// 把任意位深的采样值展开成每样本一字节。
/// `scale=false` 时保留 0..2^bpc-1 的**原始索引值**（Indexed 调色板必须用它查表），
/// `scale=true` 时按最大亮度线性放大到 0..255（灰度/RGB 分量用）。
fn expand_bits(raw: &[u8], count: usize, bpc: usize, scale: bool) -> Option<Vec<u8>> {
    match bpc {
        8 => {
            if raw.len() < count {
                return None;
            }
            Some(raw[..count].to_vec())
        }
        16 => {
            if raw.len() < count * 2 {
                return None;
            }
            Some(raw.chunks_exact(2).map(|c| c[0]).take(count).collect())
        }
        1 | 2 | 4 => {
            let per = 8 / bpc;
            let mask = ((1u16 << bpc) - 1) as u8;
            let mut out = Vec::with_capacity(count);
            for &b in raw {
                for k in 0..per {
                    let shift = 8 - bpc * (k + 1);
                    let v = (b >> shift) & mask;
                    out.push(if !scale {
                        v
                    } else {
                        match bpc {
                            1 => if v == 1 { 255 } else { 0 },
                            2 => v * 85,
                            4 => v * 17,
                            _ => v,
                        }
                    });
                    if out.len() == count {
                        return Some(out);
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn is_inverted(dict: &lopdf::Dictionary) -> bool {
    if let Ok(a) = dict.get(b"Decode").map(|o| o.as_array()) {
        if let Ok(a) = a {
            if a.len() >= 2 {
                let first = a.first().and_then(obj_num).unwrap_or(0.0);
                let second = a.get(1).and_then(obj_num).unwrap_or(1.0);
                return first > second;
            }
        }
    }
    false
}

fn encode_png(w: u32, h: u32, color: png::ColorType, pixels: &[u8]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || pixels.is_empty() {
        return None;
    }
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_color(color);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(pixels).ok()?;
    }
    Some(buf)
}

/// 把 Spec 描述的像素转成 RGB/RGBA 字节
fn spec_to_rgb(spec: &Spec, raw: &[u8], count: usize, invert: bool) -> Option<(png::ColorType, Vec<u8>)> {
    match spec {
        Spec::Gray => {
            let mut g = raw.to_vec();
            if invert {
                for v in g.iter_mut() {
                    *v = 255 - *v;
                }
            }
            Some((png::ColorType::Grayscale, g))
        }
        Spec::Rgb => {
            let mut rgb = raw.to_vec();
            if invert {
                for v in rgb.iter_mut() {
                    *v = 255 - *v;
                }
            }
            Some((png::ColorType::Rgb, rgb))
        }
        Spec::Cmyk => {
            let mut rgb = Vec::with_capacity(count * 3);
            for px in raw.chunks_exact(4) {
                let k = px[3] as f32 / 255.0;
                let f = |v: u8| {
                    let v = if invert { 255 - v } else { v };
                    ((1.0 - v as f32 / 255.0) * (1.0 - k) * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                };
                rgb.push(f(px[0]));
                rgb.push(f(px[1]));
                rgb.push(f(px[2]));
            }
            Some((png::ColorType::Rgb, rgb))
        }
        Spec::Indexed { base, lookup } => {
            let mut rgb = Vec::with_capacity(count * 3);
            for &idx in raw.iter().take(count) {
                let i = idx as usize * *base;
                if *base == 1 {
                    let v = lookup.get(i).copied().unwrap_or(255);
                    rgb.push(v);
                    rgb.push(v);
                    rgb.push(v);
                } else {
                    rgb.push(lookup.get(i).copied().unwrap_or(0));
                    rgb.push(lookup.get(i + 1).copied().unwrap_or(0));
                    rgb.push(lookup.get(i + 2).copied().unwrap_or(0));
                }
            }
            Some((png::ColorType::Rgb, rgb))
        }
    }
}

/// SMask（软蒙版）→ 每像素一字节的 alpha
/// 按 /Filter 链解码图像流。返回 (数据, 是否为 JPEG 成品)。
/// DCTDecode 之后的字节本身就是 JPEG，不能再喂给 PNG 编码器。
fn decode_filters(
    doc: &lopdf::Document,
    dict: &lopdf::Dictionary,
    payload: &[u8],
) -> Option<(Vec<u8>, bool)> {
    let mut data = payload.to_vec();
    for f in filter_list(dict) {
        match f.as_str() {
            "FlateDecode" | "Fl" => data = inflate(&data)?,
            "ASCII85Decode" | "A85" => data = ascii85_decode(&data)?,
            "LZWDecode" | "LZW" => data = lzw_decode(&data, early_change(doc, dict))?,
            "RunLengthDecode" | "RL" => data = run_length_decode(&data)?,
            "DCTDecode" | "DCT" => return Some((data, true)),
            // CCITT T.4/T.6（CCITTFaxDecode）与 JPEG2000（JPXDecode）解码器需要专门实现，
            // 当前版本有意跳过：明确返回 None，不输出损坏/空白图像，也不引入未经验证的解码器。
            "CCITTFaxDecode" | "CCF" | "JPXDecode" => return None,
            _ => return None,
        }
        if data.len() as u64 > MAX_DECODED_BYTES {
            return None;
        }
    }
    Some((data, false))
}

fn smask_alpha(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<Vec<u8>> {
    let id = dict.get(b"SMask").ok()?.as_reference().ok()?;
    let s = doc.get_object(id).ok()?.as_stream().ok()?;
    let w = s.dict.get(b"Width").ok().and_then(obj_num)? as usize;
    let h = s.dict.get(b"Height").ok().and_then(obj_num)? as usize;
    if (w as u64) * (h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }
    let (payload, jpeg) = decode_filters(doc, &s.dict, &s.content)?;
    if jpeg {
        return None;
    }
    let pred = predictor_of(doc, &s.dict);
    let raw = undo_predictor(&payload, w, h, 1, 8, pred)?;
    Some(raw)
}

/// 合并 alpha 到 RGB，得到 RGBA
fn with_alpha(color: png::ColorType, pixels: Vec<u8>, alpha: &[u8]) -> (png::ColorType, Vec<u8>) {
    if color != png::ColorType::Rgb {
        return (color, pixels);
    }
    let n = pixels.len() / 3;
    let mut out = Vec::with_capacity(n * 4);
    for i in 0..n {
        out.push(pixels[i * 3]);
        out.push(pixels[i * 3 + 1]);
        out.push(pixels[i * 3 + 2]);
        out.push(alpha.get(i).copied().unwrap_or(255));
    }
    (png::ColorType::Rgba, out)
}

/// 一条图像流 → 可直接落盘的字节（JPEG 原样，其余重编为 PNG）
fn image_bytes(doc: &lopdf::Document, stream: &lopdf::Stream) -> Option<(String, Vec<u8>)> {
    let dict = &stream.dict;
    let w = dict.get(b"Width").ok().and_then(obj_num).unwrap_or(0.0).max(0.0) as u32;
    let h = dict.get(b"Height").ok().and_then(obj_num).unwrap_or(0.0).max(0.0) as u32;
    // 尺寸下限过滤项目符号级小图标（有意跳过，不算问题）
    if w < MIN_W || h < MIN_H {
        return None;
    }
    // 上限防止畸形 PDF 用超大尺寸耗尽内存
    if (w as u64) * (h as u64) > MAX_IMAGE_PIXELS {
        crate::convert::warn("部分图片尺寸超出处理上限，已跳过");
        return None;
    }
    if filter_list(dict)
        .iter()
        .any(|f| matches!(f.as_str(), "CCITTFaxDecode" | "CCF" | "JPXDecode"))
    {
        crate::convert::warn("部分图片使用 JPX / CCITT 压缩，暂不支持，已跳过");
        return None;
    }
    let r = decode_image(doc, stream, w, h);
    if r.is_none() {
        crate::convert::warn("部分图片解码失败或色彩空间暂不支持，已跳过");
    }
    r
}

fn decode_image(doc: &lopdf::Document, stream: &lopdf::Stream, w: u32, h: u32) -> Option<(String, Vec<u8>)> {
    let dict = &stream.dict;
    let bpc = dict.get(b"BitsPerComponent").ok().and_then(obj_num).unwrap_or(8.0).max(1.0) as usize;

    let (payload, jpeg) = decode_filters(doc, dict, &stream.content)?;
    if jpeg {
        // 4 通道（CMYK / YCCK）JPEG 必须解码后重编码成 PNG，否则浏览器按 RGB 解释
        // 会得到反相 / 错色。普通灰度 / RGB JPEG 以及 CMYK 解码失败时都原样落盘 .jpg，
        // 绝不输出损坏图像。
        if let Some(png_bytes) = decode_cmyk_jpeg_to_png(&payload) {
            return Some((".png".to_string(), png_bytes));
        }
        return Some((".jpg".to_string(), payload));
    }

    let spec = color_spec(doc, dict)?;
    let colors = spec.components();
    let row_bytes = (w as usize * colors * bpc + 7) / 8;
    let pred = predictor_of(doc, dict);
    let raw = undo_predictor(&payload, row_bytes, h as usize, colors, bpc, pred)?;
    let count = w as usize * h as usize * colors;
    // Indexed 色彩空间存的是调色板**索引**，低位深时不能放大成 0..255，
    // 否则 1/2/4bit 的调色板图会整片错色。
    let indexed = matches!(spec, Spec::Indexed { .. });
    let samples = expand_bits(&raw, count, bpc, !indexed)?;

    // Indexed 的索引只有一个分量，展开后按调色板映射
    let invert = is_inverted(dict);
    let (color, pixels) = spec_to_rgb(&spec, &samples, w as usize * h as usize, invert)?;
    let (color, pixels) = match smask_alpha(doc, dict) {
        Some(a) => with_alpha(color, pixels, &a),
        None => (color, pixels),
    };
    Some((".png".to_string(), encode_png(w, h, color, &pixels)?))
}

/// DCTDecode 的 4 通道 JPEG（CMYK / YCCK）解码并重编码成 PNG；
/// 普通灰度 / RGB JPEG 返回 None，由调用方原样落盘 .jpg。
///
/// 依据 jpeg-decoder 0.3.2 源码（不凭记忆猜公式）：
///   · `info().pixel_format` 对 4 通道一律是 `PixelFormat::CMYK32`（该版本没有 YCCK32 变体）；
///   · `decoder.rs` 的 `color_convert_line_cmyk` 对 Adobe 反相通道做 `255 - c`，
///     `color_convert_line_ycck` 输出 `[r,g,b,255-k]` —— 即 decode() 返回的是「已翻正」的
///     C/M/Y/K（YCCK 时前三个通道为 RGB 值），不是 Adobe 原始反相值；
///   · crate 官方 `examples/decode.rs` 的 `cmyk_to_rgb` 把这四个值当 CMYK 用同一公式转 RGB：
///       `CMY = C*(1-K)+K`，`RGB = (1-CMY)*255`，等价于 `R = (255-C)*(255-K)/255`。
///     这里照抄该公式（含 YCCK，与官方示例一致）。
/// 解码前用 MAX_IMAGE_PIXELS / MAX_DECODED_BYTES 卡住尺寸与解码缓冲区防炸弹；
/// 任一步失败返回 None，调用方回退为原始 .jpg，不丢弃也不输出损坏图片。
fn decode_cmyk_jpeg_to_png(data: &[u8]) -> Option<Vec<u8>> {
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(data));
    if dec.read_info().is_err() {
        return None;
    }
    let info = dec.info()?;
    // 只处理 4 通道；灰度 / RGB 原样输出
    if info.pixel_format != jpeg_decoder::PixelFormat::CMYK32 {
        return None;
    }
    let w = info.width as u32;
    let h = info.height as u32;
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }
    // crate 用 components*width*height 作为解码缓冲区大小；4 通道即 4*w*h。
    let max_buf = (MAX_DECODED_BYTES as usize)
        .min((MAX_IMAGE_PIXELS as usize).saturating_mul(4));
    dec.set_max_decoding_buffer_size(max_buf);
    let pixels = dec.decode().ok()?;
    let count = (w as usize) * (h as usize);
    if pixels.len() < count * 4 {
        return None;
    }
    let mut rgb = Vec::with_capacity(count * 3);
    // YCCK 与 CMYK 的第四个通道都是「翻正后的 K」，但前三个通道语义不同：
    //   CMYK → crate 输出 (255-C, 255-M, 255-Y)，仍需按 CMYK 公式转 RGB；
    //   YCCK → crate 的 color_convert_line_ycck 输出 (R, G, B)，直接就是 RGB，
    //          再套 CMYK 公式会把颜色算错（青/品红互换）。
    // jpeg-decoder 0.3.2 的 info() 不暴露 ColorTransform，只能自己读 APP14 的 transform 字节
    // （0 = CMYK，2 = YCCK），这也是 libjpeg 判定同一件事的官方依据。
    let ycck = jpeg_is_ycck(data);
    for px in pixels.chunks_exact(4).take(count) {
        if ycck {
            rgb.extend_from_slice(&px[0..3]);
            continue;
        }
        let c = px[0] as f32 / 255.0;
        let m = px[1] as f32 / 255.0;
        let y = px[2] as f32 / 255.0;
        let k = px[3] as f32 / 255.0;
        // CMYK -> CMY -> RGB（官方 examples/decode.rs 公式，数学上等于 (1-c)(1-k)）
        let cv = c * (1.0 - k) + k;
        let mv = m * (1.0 - k) + k;
        let yv = y * (1.0 - k) + k;
        rgb.push(((1.0 - cv) * 255.0).round().clamp(0.0, 255.0) as u8);
        rgb.push(((1.0 - mv) * 255.0).round().clamp(0.0, 255.0) as u8);
        rgb.push(((1.0 - yv) * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    encode_png(w, h, png::ColorType::Rgb, &rgb)
}

/// 判断 4 通道 JPEG 是否使用 YCCK 颜色变换。
/// 依据 APP14 段（0xFFEE）里的 `Adobe` 标记：transform 0 = CMYK，2 = YCCK。
/// 没有 APP14 或长度不足时按 CMYK 处理（更常见的默认）。
fn jpeg_is_ycck(data: &[u8]) -> bool {
    let mut i = 2usize; // 跳过 SOI
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = data[i + 1];
        // 无长度的独立标记
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        // SOS 之后是熵编码数据，不再有段结构
        if m == 0xD9 || m == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if len < 2 || i + 2 + len > data.len() {
            break;
        }
        // APP14: len(2) + "Adobe"(5) + version(2) + flags0(2) + flags1(2) + transform(1) = 14
        if m == 0xEE && len >= 14 && &data[i + 4..i + 9] == b"Adobe" {
            return data[i + 15] == 2;
        }
        i += 2 + len;
    }
    false
}

/// 图片内容的短摘要（FNV-1a 低 32 位），用于给资源文件名去重
fn short_hash(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", h & 0xffff_ffff)
}

pub(super) fn extract_images(
    doc: &lopdf::Document,
    meta: &PageMeta,
    assets_dir: &Path,
    assets_rel: &str,
) -> HashMap<u32, Vec<(f64, String)>> {
    let mut out: HashMap<u32, Vec<(f64, String)>> = HashMap::new();
    let pages = doc.get_pages();

    // XObject 名 → ObjectId（每页一份资源字典）
    let mut name_maps: HashMap<u32, HashMap<String, lopdf::ObjectId>> = HashMap::new();
    let mut all_ids: Vec<lopdf::ObjectId> = Vec::new();
    for (pno, pid) in pages.iter() {
        let Ok(d) = doc.get_dictionary(*pid) else { continue };
        let res = d
            .get(b"Resources")
            .ok()
            .map(|o| resolve(doc, o))
            .and_then(|o| o.as_dict().ok().cloned());
        let res = match res {
            Some(r) => r,
            None => continue,
        };
        let Some(xo) = res.get(b"XObject").ok().and_then(|o| o.as_dict().ok()) else { continue };
        let mut m = HashMap::new();
        for (name, val) in xo.iter() {
            if let Ok(id) = val.as_reference() {
                m.insert(String::from_utf8_lossy(name).into_owned(), id);
                all_ids.push(id);
            }
        }
        name_maps.insert(*pno, m);
    }
    if all_ids.is_empty() {
        return out;
    }
    // 不要一上来就建 _assets：文档里没有可导出的图片（全是小图标 / 不支持的滤镜）时，
    // 会留下一个空目录。等第一张图真正要落盘时再创建。

    // 统计每个图片对象出现在**多少页**。
    // 用来区分两种「重复」：
    //   · 出现 1~3 页 → 正常复用插图（同一张示意图在文中出现两次），每页都要插；
    //   · 出现 >3 页 → 水印 / 背景 / 页眉 logo，只插一次。
    let mut page_count: HashMap<lopdf::ObjectId, usize> = HashMap::new();
    for (pno, _) in pages.iter() {
        let (Some(names), Some(draws)) = (name_maps.get(pno), meta.draws.get(pno)) else {
            continue;
        };
        let mut here: HashSet<lopdf::ObjectId> = HashSet::new();
        for (_, name) in draws {
            if let Some(id) = names.get(name) {
                here.insert(*id);
            }
        }
        for id in here {
            *page_count.entry(id).or_insert(0) += 1;
        }
    }

    // 每个对象只导出一次，多页复用时共享同一个相对路径
    let mut cache: HashMap<lopdf::ObjectId, String> = HashMap::new();
    let mut idx = 0usize;
    let mut assets_ready = false;

    for (pno, pid) in pages.iter() {
        let draws = match meta.draws.get(pno) {
            Some(d) => d.clone(),
            None => {
                // content stream 里找不到定位信息：退回按资源字典顺序、页首插入
                let list: Vec<(f64, String)> = name_maps
                    .get(pno)
                    .map(|m| m.keys().cloned().collect::<Vec<String>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|n| (0.0, n))
                    .collect();
                list
            }
        };
        let _ = pid;
        let Some(names) = name_maps.get(pno) else { continue };
        // 同一页里同一对象被多次 Do（拼贴、平铺）只插一次
        let mut used_here: HashSet<lopdf::ObjectId> = HashSet::new();

        for (y, name) in draws {
            let Some(id) = names.get(&name).copied() else { continue };
            if !used_here.insert(id) {
                continue;
            }
            if let Some(rel) = cache.get(&id) {
                // 已经导出过：装饰图（出现在 >3 页）只保留第一次，其余复用插图照常插入
                if page_count.get(&id).copied().unwrap_or(1) > 3 {
                    continue;
                }
                out.entry(*pno).or_default().push((y, rel.clone()));
                continue;
            }
            let rel = {
                let Some(stream) = doc.get_object(id).ok().and_then(|o| o.as_stream().ok()) else { continue };
                let is_img = stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .map(|n| n == b"Image")
                    .unwrap_or(false);
                if !is_img {
                    continue;
                }
                let Some((ext, bytes)) = image_bytes(doc, stream) else { continue };
                idx += 1;
                // 文件名带内容摘要：两份不同的 PDF 输出到同一个 assets 目录时，
                // 不会再因为都叫 p001_1.png 而互相覆盖
                let base = format!("p{:03}_{}_{}{}", pno, idx, short_hash(&bytes), ext);
                if !assets_ready {
                    if let Err(e) = fs::create_dir_all(assets_dir) {
                        crate::convert::warn(format!("图片目录创建失败，图片未导出：{}", e));
                        continue;
                    }
                    assets_ready = true;
                }
                if let Err(e) = fs::write(assets_dir.join(&base), &bytes) {
                    crate::convert::warn(format!("部分图片写入失败：{}", e));
                    continue;
                }
                let r = format!("{}/{}", assets_rel, base);
                cache.insert(id, r.clone());
                r
            };
            out.entry(*pno).or_default().push((y, rel));
        }
    }

    for list in out.values_mut() {
        list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        list.dedup_by(|a, b| a.1 == b.1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼一个只有 APP14（可选）与 EOI 的伪 JPEG，用于验证标记解析
    fn fake_jpeg(transform: Option<u8>, with_app0: bool) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        if with_app0 {
            // APP0 / JFIF：长度 16（含长度字段），数据 14 字节
            v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
            v.extend_from_slice(b"JFIF\0");
            v.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        }
        if let Some(t) = transform {
            // APP14 / Adobe：长度 14（含长度字段），数据 12 字节
            v.extend_from_slice(&[0xFF, 0xEE, 0x00, 0x0E]);
            v.extend_from_slice(b"Adobe");
            v.extend_from_slice(&[0x00, 0x64, 0x00, 0x00, 0x00, 0x00]);
            v.push(t);
        }
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn detects_ycck_transform() {
        assert!(jpeg_is_ycck(&fake_jpeg(Some(2), false)));
    }

    #[test]
    fn plain_cmyk_is_not_ycck() {
        assert!(!jpeg_is_ycck(&fake_jpeg(Some(0), false)));
    }

    #[test]
    fn missing_app14_defaults_to_cmyk() {
        assert!(!jpeg_is_ycck(&fake_jpeg(None, true)));
        assert!(!jpeg_is_ycck(&[0xFF, 0xD8, 0xFF, 0xD9]));
        assert!(!jpeg_is_ycck(&[]));
    }

    #[test]
    fn skips_app0_before_app14() {
        assert!(jpeg_is_ycck(&fake_jpeg(Some(2), true)));
    }

    #[test]
    fn stops_at_sos_without_scanning_entropy_data() {
        // APP14 在 SOS 之后不应被当作有效标记（熵编码数据里可能出现 0xFFEE）
        let mut v = vec![0xFF, 0xD8];
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        v.extend_from_slice(&[0xFF, 0xEE, 0x00, 0x0E]);
        v.extend_from_slice(b"Adobe");
        v.extend_from_slice(&[0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x02]);
        assert!(!jpeg_is_ycck(&v));
    }
}
