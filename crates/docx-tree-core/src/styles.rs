//! styles.xml 解析与有效格式合并（design.md §5.2①）。
//!
//! 优先级（自低到高，逐层覆盖）：
//!   docDefaults → pStyle 的 basedOn 继承链（链根→链尾）→ 文档内直接格式
//! Run：docDefaults rPr → 段落样式 rPr → rStyle 链 → 直接 rPr
//! basedOn 环检测：链长上限 32。

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::xml::{attr, local_name};

/// 链长上限（防 basedOn 环导致死循环）
const MAX_CHAIN: usize = 32;

/// 字符/Run 格式。全部为三态：None = 未设置（继承低优先级层）。
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RPr {
    pub b: Option<bool>,
    pub i: Option<bool>,
    pub u: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<String>,
    /// 半磅（w:sz 原始值）
    pub sz: Option<u32>,
    pub font: Option<String>,
}

/// 段落格式（白名单子集）
#[derive(Clone, Default, PartialEq, Debug)]
pub struct PPr {
    /// "left"|"center"|"right"|"justify"
    pub jc: Option<String>,
    pub left: Option<i64>,
    pub right: Option<i64>,
    pub first: Option<i64>,
    pub hanging: Option<i64>,
}

impl RPr {
    /// 用更高优先级层覆盖自身已设置字段
    pub fn overlay(&mut self, hi: &RPr) {
        macro_rules! ov {
            ($f:ident) => {
                if hi.$f.is_some() {
                    self.$f = hi.$f.clone();
                }
            };
        }
        ov!(b);
        ov!(i);
        ov!(u);
        ov!(strike);
        ov!(color);
        ov!(sz);
        ov!(font);
    }
}

impl PPr {
    pub fn overlay(&mut self, hi: &PPr) {
        macro_rules! ov {
            ($f:ident, clone) => {
                if hi.$f.is_some() {
                    self.$f = hi.$f.clone();
                }
            };
            ($f:ident, copy) => {
                if hi.$f.is_some() {
                    self.$f = hi.$f;
                }
            };
        }
        ov!(jc, clone);
        ov!(left, copy);
        ov!(right, copy);
        ov!(first, copy);
        ov!(hanging, copy);
    }
}

pub struct StyleDef {
    pub name: Option<String>,
    pub based_on: Option<String>,
    pub is_paragraph: bool,
    pub ppr: PPr,
    pub rpr: RPr,
}

pub struct Styles {
    pub doc_p: PPr,
    pub doc_r: RPr,
    pub map: HashMap<String, StyleDef>,
    /// 段落样式索引（文档顺序），输出到 JSON 供 TS 渲染层使用
    pub index: Vec<(String, Option<String>)>,
}

impl Default for Styles {
    fn default() -> Self {
        Styles {
            doc_p: PPr::default(),
            doc_r: RPr::default(),
            map: HashMap::new(),
            index: Vec::new(),
        }
    }
}

/// 容器路径：只有容器元素压栈，属性叶子元素不压栈
#[derive(PartialEq, Clone, Copy)]
enum Ctx {
    Other,
    DocDefaults,
    RDefault, // rPrDefault
    PDefault, // pPrDefault
    RPrDoc,   // docDefaults 内的 rPr
    PPrDoc,   // docDefaults 内的 pPr
    Style,
    StylePPr,
    StyleRPr,
}

pub fn parse(xml: &str) -> Result<Styles> {
    let mut st = Styles::default();
    let mut path: Vec<Ctx> = vec![Ctx::Other];
    let mut cur: Option<(String, StyleDef)> = None;

    let mut r = Reader::from_str(xml);
    loop {
        match r
            .read_event()
            .map_err(|_| Error::msg("styles: parse error"))?
        {
            Event::Start(e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                st.step(&mut path, &mut cur, &e, &name, true);
            }
            Event::Empty(e) => {
                // Empty：属性即全部内容，按叶子元素处理（容器在 styles.xml 中不会自闭合）
                let name = local_name(e.name().as_ref()).to_vec();
                st.step(&mut path, &mut cur, &e, &name, false);
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                st.step_end(&mut path, &mut cur, &name);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(st)
}

impl Styles {
    fn step(
        &mut self,
        path: &mut Vec<Ctx>,
        cur: &mut Option<(String, StyleDef)>,
        e: &BytesStart,
        name: &[u8],
        is_container_possible: bool,
    ) {
        let parent = *path.last().unwrap();
        // 决定本元素是否为容器，以及压入什么
        let ctx = match (parent, name) {
            (_, b"docDefaults") => Ctx::DocDefaults,
            (Ctx::DocDefaults, b"rPrDefault") => Ctx::RDefault,
            (Ctx::DocDefaults, b"pPrDefault") => Ctx::PDefault,
            (Ctx::RDefault, b"rPr") => Ctx::RPrDoc,
            (Ctx::PDefault, b"pPr") => Ctx::PPrDoc,
            (Ctx::Style, b"pPr") => Ctx::StylePPr,
            (Ctx::Style, b"rPr") => Ctx::StyleRPr,
            (_, b"style") => {
                let id = attr(e, "styleId", None);
                let ty = attr(e, "type", None);
                if let Some(id) = id {
                    cur.replace((
                        id,
                        StyleDef {
                            name: None,
                            based_on: None,
                            // type 缺省视为 paragraph
                            is_paragraph: ty.as_deref().map(|t| t != "character").unwrap_or(true),
                            ppr: PPr::default(),
                            rpr: RPr::default(),
                        },
                    ));
                }
                Ctx::Style
            }
            _ => Ctx::Other, // 叶子元素
        };

        if ctx != Ctx::Other {
            // 容器自身一般无需要收集的属性，直接压栈
            path.push(ctx);
            return;
        }
        let _ = is_container_possible;
        // 叶子元素：按父上下文收集属性
        self.collect_leaf(parent, cur, e, name);
    }

    fn collect_leaf(
        &mut self,
        parent: Ctx,
        cur: &mut Option<(String, StyleDef)>,
        e: &BytesStart,
        name: &[u8],
    ) {
        match parent {
            Ctx::RPrDoc | Ctx::StyleRPr => {
                let got = read_rpr_leaf(e, name);
                if got != RPr::default() {
                    match parent {
                        Ctx::RPrDoc => self.doc_r.overlay(&got),
                        _ => {
                            if let Some((_, s)) = cur {
                                s.rpr.overlay(&got);
                            }
                        }
                    }
                }
            }
            Ctx::PPrDoc | Ctx::StylePPr => {
                let got = read_ppr_leaf(e, name);
                if got != PPr::default() {
                    match parent {
                        Ctx::PPrDoc => self.doc_p.overlay(&got),
                        _ => {
                            if let Some((_, s)) = cur {
                                s.ppr.overlay(&got);
                            }
                        }
                    }
                }
            }
            Ctx::Style => {
                if let Some((_, s)) = cur {
                    match name {
                        b"name" => s.name = attr(e, "val", None),
                        b"basedOn" => s.based_on = attr(e, "val", None),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn step_end(&mut self, path: &mut Vec<Ctx>, cur: &mut Option<(String, StyleDef)>, name: &[u8]) {
        let top = *path.last().unwrap();
        let pops = match (top, name) {
            (Ctx::DocDefaults, b"docDefaults") => true,
            (Ctx::RDefault, b"rPrDefault") => true,
            (Ctx::PDefault, b"pPrDefault") => true,
            (Ctx::RPrDoc, b"rPr") => true,
            (Ctx::PPrDoc, b"pPr") => true,
            (Ctx::Style, b"style") => {
                if let Some((id, s)) = cur.take() {
                    if s.is_paragraph {
                        self.index.push((id.clone(), s.name.clone()));
                    }
                    self.map.insert(id, s);
                }
                true
            }
            (Ctx::StylePPr, b"pPr") | (Ctx::StyleRPr, b"rPr") => true,
            _ => false,
        };
        if pops {
            path.pop();
        }
    }

    /// basedOn 继承链（含自身），返回根→尾顺序。环/超长截断。
    pub fn chain<'s>(&'s self, id: Option<&str>) -> Vec<&'s StyleDef> {
        let mut ids: Vec<&str> = Vec::new();
        let mut cur = id;
        while let Some(i) = cur {
            if ids.len() >= MAX_CHAIN || ids.contains(&i) {
                break; // 环检测：按链断开处截断
            }
            match self.map.get(i) {
                Some(s) => {
                    ids.push(i);
                    cur = s.based_on.as_deref();
                }
                None => break, // 样式缺失：静默降级为仅直接格式
            }
        }
        ids.iter().rev().filter_map(|i| self.map.get(*i)).collect()
    }

    /// 段落有效格式
    pub fn effective_ppr(&self, style_id: Option<&str>, direct: &PPr) -> PPr {
        let mut acc = self.doc_p.clone();
        for s in self.chain(style_id) {
            acc.overlay(&s.ppr);
        }
        acc.overlay(direct);
        acc
    }

    /// Run 有效格式
    pub fn effective_rpr(
        &self,
        para_style: Option<&str>,
        rstyle: Option<&str>,
        direct: &RPr,
    ) -> RPr {
        let mut acc = self.doc_r.clone();
        for s in self.chain(para_style) {
            acc.overlay(&s.rpr);
        }
        for s in self.chain(rstyle) {
            acc.overlay(&s.rpr);
        }
        acc.overlay(direct);
        acc
    }
}

/// 读取 rPr 白名单叶子属性
pub fn read_rpr_leaf(e: &BytesStart, name: &[u8]) -> RPr {
    let mut got = RPr::default();
    match name {
        b"b" => got.b = Some(on_off(e)),
        b"i" => got.i = Some(on_off(e)),
        b"u" => got.u = Some(on_off(e)),
        b"strike" => got.strike = Some(on_off(e)),
        b"color" => {
            if let Some(c) = attr(e, "val", None) {
                if !c.eq_ignore_ascii_case("auto") {
                    got.color = Some(c);
                }
            }
        }
        b"sz" => {
            if let Some(v) = attr(e, "val", None) {
                got.sz = v.parse().ok();
            }
        }
        b"rFonts" => {
            if let Some(f) = attr(e, "ascii", None) {
                got.font = Some(f);
            }
        }
        _ => {}
    }
    got
}

/// 读取 pPr 白名单叶子属性
pub fn read_ppr_leaf(e: &BytesStart, name: &[u8]) -> PPr {
    let mut got = PPr::default();
    match name {
        b"jc" => got.jc = attr(e, "val", None).and_then(normalize_jc),
        b"ind" => {
            got.left = twips(e, "left").or_else(|| twips(e, "start"));
            got.right = twips(e, "right").or_else(|| twips(e, "end"));
            got.first = twips(e, "firstLine");
            got.hanging = twips(e, "hanging");
        }
        _ => {}
    }
    got
}

/// w:b / w:i 等 on/off 属性语义：val 缺失 = on，"0"/"false"/"none"/"off" = off
pub fn on_off(e: &BytesStart) -> bool {
    match attr(e, "val", None).as_deref() {
        Some("0") | Some("false") | Some("none") | Some("off") => false,
        _ => true,
    }
}

/// jc 值归一化到输出枚举
pub fn normalize_jc(v: String) -> Option<String> {
    let out = match v.as_str() {
        "left" | "start" => "left",
        "center" => "center",
        "right" | "end" => "right",
        "both" | "distribute" => "justify",
        _ => return None,
    };
    Some(String::from(out))
}

/// 读 twip 数值属性
pub fn twips(e: &BytesStart, name: &str) -> Option<i64> {
    attr(e, name, None).and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr><w:sz w:val="24"/><w:rFonts w:ascii="Calibri"/></w:rPr>
    </w:rPrDefault>
    <w:pPrDefault><w:pPr/></w:pPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:styleId="Base">
    <w:name w:val="base"/>
    <w:pPr><w:ind w:left="720"/></w:pPr>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="Base"/>
    <w:pPr><w:ind w:left="0"/><w:jc w:val="center"/></w:pPr>
    <w:rPr><w:sz w:val="32"/><w:color w:val="FF0000"/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="Emph">
    <w:name w:val="emphasis"/>
    <w:rPr><w:i/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Loop1">
    <w:name w:val="loop"/>
    <w:basedOn w:val="Loop2"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Loop2">
    <w:basedOn w:val="Loop1"/>
    <w:rPr><w:i/></w:rPr>
  </w:style>
</w:styles>"#;

    #[test]
    fn chain_and_effective() {
        let st = parse(STYLES).unwrap();
        // docDefaults 合入 + 继承链覆盖
        let eff = st.effective_rpr(Some("Heading1"), None, &RPr::default());
        assert_eq!(eff.sz, Some(32)); // Heading1 覆盖 docDefaults 的 24
        assert_eq!(eff.color.as_deref(), Some("FF0000"));
        assert_eq!(eff.b, Some(true)); // 继承自 basedOn Base
        assert_eq!(eff.font.as_deref(), Some("Calibri"));

        // rStyle 链叠加
        let eff = st.effective_rpr(Some("Heading1"), Some("Emph"), &RPr::default());
        assert_eq!(eff.i, Some(true));
        assert_eq!(eff.b, Some(true));

        // 段落有效格式：Base(left=720) → Heading1(left=0,center)
        let pp = st.effective_ppr(Some("Heading1"), &PPr::default());
        assert_eq!(pp.jc.as_deref(), Some("center"));
        assert_eq!(pp.left, Some(0)); // 链尾覆盖链中

        // 直接格式优先级最高
        let pp = st.effective_ppr(
            Some("Heading1"),
            &PPr {
                jc: Some("right".into()),
                ..Default::default()
            },
        );
        assert_eq!(pp.jc.as_deref(), Some("right"));

        // basedOn 环不死循环
        let eff = st.effective_rpr(Some("Loop2"), None, &RPr::default());
        assert_eq!(eff.i, Some(true));
    }

    #[test]
    fn missing_style_degrades() {
        let st = parse(STYLES).unwrap();
        let pp = st.effective_ppr(Some("NoSuchStyle"), &PPr::default());
        assert_eq!(pp, PPr::default());
    }

    #[test]
    fn style_index_order() {
        let st = parse(STYLES).unwrap();
        // character 样式不入索引
        assert_eq!(st.index.len(), 4);
        assert_eq!(st.index[1], ("Heading1".into(), Some("heading 1".into())));
    }
}
