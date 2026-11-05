//! numbering.xml 解析与 marker 生成（design.md §5.2②）。
//!
//! - `numId → abstractNumId` 映射 + `(numId,level)` 计数器（builder 持有）
//! - `lvlText` 中 `%1..%9` 占位符用对应层计数器现场替换
//! - numFmt 子集：decimal/lowerLetter/upperLetter/lowerRoman/upperRoman/chineseCounting/bullet
//!   未覆盖的 numFmt 降级为十进制计数

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::xml::{attr, local_name};

const MAX_LEVEL: u8 = 8; // ilvl 0..8 共 9 层

#[derive(Clone)]
pub struct Level {
    pub fmt: String,
    pub text: String,
    pub start: i64,
}

pub struct Numbering {
    /// numId → abstractNumId
    num_to_abs: HashMap<u32, u32>,
    /// abstractNumId → 每层定义（下标 = ilvl）
    abs: HashMap<u32, Vec<Option<Level>>>,
    /// (numId, ilvl) → startOverride（lvlOverride/startOverride）
    overrides: HashMap<(u32, u8), i64>,
}

impl Default for Numbering {
    fn default() -> Self {
        Numbering {
            num_to_abs: HashMap::new(),
            abs: HashMap::new(),
            overrides: HashMap::new(),
        }
    }
}

pub fn parse(xml: &str) -> Result<Numbering> {
    let mut st = Numbering::default();
    let mut cur_abs: Option<u32> = None;
    let mut cur_lvl: Option<(u32, u8, Level)> = None; // (absId, ilvl, 半成品)
    let mut cur_num: Option<u32> = None;
    let mut cur_ovr: Option<(u32, u8)> = None;

    let mut r = Reader::from_str(xml);
    loop {
        let ev = r
            .read_event()
            .map_err(|_| Error::msg("numbering: parse error"))?;
        match ev {
            Event::Start(e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"abstractNum" => {
                        cur_abs = attr(&e, "abstractNumId", None).and_then(|v| v.parse().ok());
                        if let Some(id) = cur_abs {
                            st.abs
                                .entry(id)
                                .or_insert_with(|| vec![None; MAX_LEVEL as usize + 1]);
                        }
                    }
                    b"lvl" => {
                        let ilvl: u8 = attr(&e, "ilvl", None)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        if let Some(abs) = cur_abs {
                            cur_lvl = Some((
                                abs,
                                ilvl.min(MAX_LEVEL),
                                Level {
                                    fmt: String::new(),
                                    text: String::new(),
                                    start: 1,
                                },
                            ));
                        }
                    }
                    b"num" => {
                        cur_num = attr(&e, "numId", None).and_then(|v| v.parse().ok());
                    }
                    b"lvlOverride" => {
                        let ilvl: u8 = attr(&e, "ilvl", None)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        if let Some(n) = cur_num {
                            cur_ovr = Some((n, ilvl.min(MAX_LEVEL)));
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"abstractNumId" => {
                        if let (Some(n), true) = (cur_num, cur_ovr.is_none()) {
                            if let Some(a) = attr(&e, "val", None).and_then(|v| v.parse().ok()) {
                                st.num_to_abs.insert(n, a);
                            }
                        }
                    }
                    b"numFmt" => {
                        if let Some((_, _, lvl)) = &mut cur_lvl {
                            lvl.fmt = attr(&e, "val", None).unwrap_or_default();
                        }
                    }
                    b"lvlText" => {
                        if let Some((_, _, lvl)) = &mut cur_lvl {
                            lvl.text = attr(&e, "val", None).unwrap_or_default();
                        }
                    }
                    b"start" => {
                        if let Some(v) = attr(&e, "val", None).and_then(|v| v.parse().ok()) {
                            match &mut cur_lvl {
                                Some((_, _, lvl)) => lvl.start = v,
                                None => {
                                    if let Some((n, l)) = cur_ovr {
                                        st.overrides.insert((n, l), v);
                                    }
                                }
                            }
                        }
                    }
                    b"startOverride" => {
                        if let Some((n, l)) = cur_ovr {
                            if let Some(v) = attr(&e, "val", None).and_then(|v| v.parse().ok()) {
                                st.overrides.insert((n, l), v);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                match name.as_slice() {
                    b"abstractNum" => cur_abs = None,
                    b"lvl" => {
                        if let Some((abs, ilvl, lvl)) = cur_lvl.take() {
                            if let Some(levels) = st.abs.get_mut(&abs) {
                                levels[ilvl as usize] = Some(lvl);
                            }
                        }
                    }
                    b"num" => cur_num = None,
                    b"lvlOverride" => cur_ovr = None,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(st)
}

impl Numbering {
    /// 为一个带 numPr 的段落生成 marker：推进计数器并渲染 lvlText。
    /// 返回 None 表示该 numId 无定义（numbering 信息仍保留，marker 省略）。
    pub fn marker(
        &self,
        num_id: u32,
        level: u8,
        counters: &mut HashMap<(u32, u8), i64>,
    ) -> Option<String> {
        let abs_id = *self.num_to_abs.get(&num_id)?;
        let levels = self.abs.get(&abs_id)?;
        let level = level.min(MAX_LEVEL);
        let lvl = levels[level as usize].as_ref()?;
        // startOverride 优先于 abstractNum 内的 start
        let start = self
            .overrides
            .get(&(num_id, level))
            .copied()
            .unwrap_or(lvl.start);
        // 推进本层计数器；更深层计数器全部重置（Word 行为）
        let cur = counters
            .get(&(num_id, level))
            .map(|v| v + 1)
            .unwrap_or(start);
        counters.insert((num_id, level), cur);
        counters.retain(|&(n, l), _| !(n == num_id && l > level));

        if lvl.fmt == "bullet" {
            // 项目符号：lvlText 即符号本身，不消费计数器
            return Some(lvl.text.clone());
        }
        // 替换 %1..%9：引用同列表其它层的当前计数器值（未推进过则用该层 start）
        let mut out = String::with_capacity(lvl.text.len() + 8);
        let bytes = lvl.text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let ch = lvl.text[i..].chars().next().unwrap();
            if ch == '%' {
                let next = lvl.text[i + 1..].chars().next();
                if let Some(c2 @ '1'..='9') = next {
                    let ref_lvl = c2 as u8 - b'1'; // %1 → ilvl 0
                    let v = counters
                        .get(&(num_id, ref_lvl))
                        .copied()
                        .unwrap_or_else(|| {
                            // 未用过的层取其 start
                            levels[ref_lvl as usize]
                                .as_ref()
                                .map(|l| self.start_of(num_id, ref_lvl, l))
                                .unwrap_or(1)
                        });
                    let fmt = levels[ref_lvl as usize]
                        .as_ref()
                        .map(|l| l.fmt.as_str())
                        .unwrap_or("");
                    out.push_str(&format_value(v, fmt));
                    i += 2; // '%' 与 ASCII 数字各 1 字节
                    continue;
                }
            }
            out.push(ch);
            i += ch.len_utf8();
        }
        Some(out)
    }

    fn start_of(&self, num_id: u32, level: u8, lvl: &Level) -> i64 {
        self.overrides
            .get(&(num_id, level))
            .copied()
            .unwrap_or(lvl.start)
    }
}

/// 按 numFmt 渲染计数值
fn format_value(v: i64, fmt: &str) -> String {
    match fmt {
        "lowerLetter" => letters(v, false),
        "upperLetter" => letters(v, true),
        "lowerRoman" => roman(v, false),
        "upperRoman" => roman(v, true),
        "chineseCounting" | "chineseLegalSimplified" => chinese(v),
        // bullet 已在上方短路；未知 numFmt 降级为十进制
        _ => {
            // 手写 itoa：避免 i64 Display 的 fmt 机制进产物
            let mut s = String::new();
            crate::json::put_i64(v, &mut s);
            s
        }
    }
}

/// 字母编号：1→a … 26→z，27→aa（电子表格风格）
fn letters(mut v: i64, upper: bool) -> String {
    if v < 1 {
        let mut s = String::new();
        crate::json::put_i64(v, &mut s);
        return s;
    }
    let mut s = Vec::new();
    while v > 0 {
        let rem = ((v - 1) % 26) as u8;
        s.push(b'a' + rem);
        v = (v - 1) / 26;
    }
    s.reverse();
    let s: Vec<u8> = s
        .into_iter()
        .map(|c| if upper { c - 32 } else { c })
        .collect();
    // 纯 ASCII 字节，from_utf8 必然成功；fallback 走 unwrap_or_default
    String::from_utf8(s).unwrap_or_default()
}

/// 罗马数字（1..=3999）
fn roman(v: i64, upper: bool) -> String {
    const PAIRS: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    if !(1..=3999).contains(&v) {
        let mut s = String::new();
        crate::json::put_i64(v, &mut s);
        return s;
    }
    let mut n = v;
    let mut s = String::new();
    for (val, sym) in PAIRS {
        while n >= val {
            s.push_str(sym);
            n -= val;
        }
    }
    if upper {
        // 罗马数字仅含 ASCII；用 ascii 大写化避免 unicode 大小写映射表
        s.make_ascii_uppercase();
    }
    s
}

/// 中文数字（1..=9999），超界降级十进制
fn chinese(v: i64) -> String {
    const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    if !(1..=9999).contains(&v) {
        let mut s = String::new();
        crate::json::put_i64(v, &mut s);
        return s;
    }
    let units: [(i64, char); 4] = [(1, '\0'), (10, '十'), (100, '百'), (1000, '千')];
    let mut out = String::new();
    let mut rest = v;
    let mut started = false;
    for &(unit_val, unit_sym) in units.iter().rev() {
        let d = rest / unit_val;
        rest %= unit_val;
        if d > 0 {
            // 数字位恒发音（一百/一千），唯一例外：10..19 的前导"一"省略（一十 -> 十）
            if !(unit_val == 10 && !started && d == 1) {
                out.push(DIGITS[d as usize]);
            }
            if unit_val > 1 {
                out.push(unit_sym);
            }
            started = true;
        } else if started && rest > 0 {
            if out.chars().last() != Some('零') {
                out.push('零');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUMBERING: &str = r#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
    <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="7">
    <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="chineseCounting"/><w:lvlText w:val="第%1章"/></w:lvl>
    <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1-%2"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="9">
    <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val=""/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="3"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="5"><w:abstractNumId w:val="7"/></w:num>
  <w:num w:numId="10"><w:abstractNumId w:val="9"/></w:num>
  <w:num w:numId="11">
    <w:abstractNumId w:val="0"/>
    <w:lvlOverride w:ilvl="0"><w:startOverride w:val="100"/></w:lvlOverride>
  </w:num>
</w:numbering>"#;

    fn mk() -> Numbering {
        parse(NUMBERING).unwrap()
    }

    #[test]
    fn decimal_and_reset() {
        let n = mk();
        let mut c = HashMap::new();
        assert_eq!(n.marker(3, 0, &mut c).unwrap(), "1.");
        assert_eq!(n.marker(3, 0, &mut c).unwrap(), "2.");
        // 下钻第 1 层
        assert_eq!(n.marker(3, 1, &mut c).unwrap(), "a)");
        assert_eq!(n.marker(3, 1, &mut c).unwrap(), "b)");
        // 回到第 0 层：深层计数器重置
        assert_eq!(n.marker(3, 0, &mut c).unwrap(), "3.");
        assert_eq!(n.marker(3, 1, &mut c).unwrap(), "a)");
    }

    #[test]
    fn cross_level_placeholders() {
        let n = mk();
        let mut c = HashMap::new();
        // 第%1章 → chineseCounting；%1-%2 引用两层计数器
        assert_eq!(n.marker(5, 0, &mut c).unwrap(), "第一章");
        assert_eq!(n.marker(5, 1, &mut c).unwrap(), "一-I");
        assert_eq!(n.marker(5, 1, &mut c).unwrap(), "一-II");
        assert_eq!(n.marker(5, 0, &mut c).unwrap(), "第二章");
        assert_eq!(n.marker(5, 1, &mut c).unwrap(), "二-I");
    }

    #[test]
    fn bullet_and_override() {
        let n = mk();
        let mut c = HashMap::new();
        assert_eq!(n.marker(10, 0, &mut c).unwrap(), "");
        // startOverride=100
        assert_eq!(n.marker(11, 0, &mut c).unwrap(), "100.");
        assert_eq!(n.marker(11, 0, &mut c).unwrap(), "101.");
        // 未定义 numId
        assert!(n.marker(999, 0, &mut c).is_none());
    }

    #[test]
    fn fmt_converters() {
        assert_eq!(letters(1, false), "a");
        assert_eq!(letters(26, true), "Z");
        assert_eq!(letters(27, false), "aa");
        assert_eq!(letters(703, false), "aaa");
        assert_eq!(roman(1994, true), "MCMXCIV");
        assert_eq!(roman(9, false), "ix");
        assert_eq!(roman(0, true), "0"); // 越界降级
        assert_eq!(chinese(10), "十");
        assert_eq!(chinese(23), "二十三");
        assert_eq!(chinese(110), "一百一十");
        assert_eq!(chinese(101), "一百零一");
        assert_eq!(chinese(1234), "一千二百三十四");
        assert_eq!(chinese(10000), "10000"); // 越界降级
    }
}
