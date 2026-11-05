//! XML 事件流 → 语义树 状态机（design.md §5）。
//!
//! 策略：
//! - 显式帧栈；已知元素压帧，未知元素透明下钻（文本只在 Run 内的 w:t 中生效，
//!   透明下钻不会引入杂散文本）
//! - `mc:Fallback` 子树整体跳过（`mc:AlternateContent` 取 Choice 分支）
//! - 结构异常静默容错，只有 XML 无法继续才返回 Err

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::model::*;
use crate::numbering::Numbering as NumberingDefs;
use crate::rels::{self, Rel};
use crate::styles::{self, normalize_jc, PPr, RPr, Styles};
use crate::xml::{attr, local_name, unescape, NsMap};

/// vMerge 状态
#[derive(PartialEq, Clone, Copy)]
enum Vm {
    Restart,
    Continue,
}

struct ParaB {
    style: Option<String>,
    num: Option<(u32, u8)>,
    direct: PPr,
    children: Vec<Inline>,
    in_ppr: bool,
    in_numpr: bool,
}

struct RunB {
    direct: RPr,
    rstyle: Option<String>,
    in_rpr: bool,
    in_wt: bool,
    in_instr: bool,
    /// w:t 文本累积
    buf: String,
    instr_buf: String,
}

struct CellB {
    col_span: u32,
    row_span: u32,
    vm: Option<Vm>,
    in_tcpr: bool,
    blocks: Vec<Block>,
}

struct HyperB {
    target: String,
    children: Vec<Inline>,
}

struct DrawB {
    cx: Option<u64>,
    cy: Option<u64>,
    embed: Option<String>,
    name: Option<String>,
    extent_seen: bool,
}

enum Frame {
    Body(Vec<Block>),
    Table(Vec<Vec<CellB>>),
    Row(Vec<CellB>),
    Cell(CellB),
    Para(ParaB),
    Run(RunB),
    Hyper(HyperB),
    Draw(DrawB),
    /// 跳过子树（内数字为剩余深度）
    Skip(u32),
    Sect(Section),
}

pub struct Builder<'x> {
    rels: &'x HashMap<String, Rel>,
    styles: &'x Styles,
    numbering: Option<&'x NumberingDefs>,
    counters: HashMap<(u32, u8), i64>,
    stack: Vec<Frame>,
    section: Option<Section>,
    ns: NsMap,
}

/// 解析 document.xml 文本，产出 (body, section)
pub fn build(
    xml: &str,
    rels: &HashMap<String, Rel>,
    styles: &Styles,
    numbering: Option<&NumberingDefs>,
) -> Result<(Vec<Block>, Option<Section>)> {
    let mut b = Builder {
        rels,
        styles,
        numbering,
        counters: HashMap::new(),
        stack: vec![Frame::Body(Vec::new())],
        section: None,
        ns: NsMap::default(),
    };
    b.run(xml)?;
    // 容错：畸形 XML 可能留下未闭合帧，只取 Body
    let body = b
        .stack
        .drain(..)
        .find_map(|f| match f {
            Frame::Body(v) => Some(v),
            _ => None,
        })
        .unwrap_or_default();
    Ok((body, b.section))
}

impl<'x> Builder<'x> {
    fn run(&mut self, xml: &str) -> Result<()> {
        let mut r = Reader::from_str(xml);
        loop {
            match r.read_event().map_err(|_| Error::msg("xml: parse error"))? {
                Event::Start(e) => {
                    self.ns.note_decl(&e);
                    let name = local_name(e.name().as_ref()).to_vec();
                    self.start(&name, &e, false);
                }
                Event::Empty(e) => {
                    self.ns.note_decl(&e);
                    let name = local_name(e.name().as_ref()).to_vec();
                    self.start(&name, &e, true);
                }
                Event::Text(t) => self.text(t.as_ref()),
                // quick-xml 0.38 把实体拆成独立事件，需就地解码并入文本缓冲
                Event::GeneralRef(g) => self.general_ref(g.as_ref()),
                Event::End(e) => {
                    let name = local_name(e.name().as_ref()).to_vec();
                    self.end(&name);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(())
    }

    // ---- 事件分派 ----

    fn start(&mut self, name: &[u8], e: &BytesStart, empty: bool) {
        // Skip 模式：不处理任何内容，只数深度
        if let Some(Frame::Skip(d)) = self.stack.last_mut() {
            if !empty {
                *d += 1;
            }
            return;
        }

        match name {
            b"Fallback" => {
                // 只取 mc:Choice 分支
                if !empty {
                    self.stack.push(Frame::Skip(1));
                }
                return;
            }
            // 透明容器：直接下钻取内部内容
            b"AlternateContent" | b"sdt" | b"sdtContent" | b"ins" | b"smartTag" => return,
            b"tbl" => {
                if !empty {
                    self.stack.push(Frame::Table(Vec::new()));
                }
                return;
            }
            b"tr" => {
                if !empty {
                    self.stack.push(Frame::Row(Vec::new()));
                }
                return;
            }
            b"tc" => {
                if !empty {
                    self.stack.push(Frame::Cell(CellB {
                        col_span: 1,
                        row_span: 1,
                        vm: None,
                        in_tcpr: false,
                        blocks: Vec::new(),
                    }));
                }
                return;
            }
            b"p" => {
                if !empty {
                    self.stack.push(Frame::Para(ParaB {
                        style: None,
                        num: None,
                        direct: PPr::default(),
                        children: Vec::new(),
                        in_ppr: false,
                        in_numpr: false,
                    }));
                }
                return;
            }
            b"r" => {
                if !empty {
                    self.stack.push(Frame::Run(RunB {
                        direct: RPr::default(),
                        rstyle: None,
                        in_rpr: false,
                        in_wt: false,
                        in_instr: false,
                        buf: String::new(),
                        instr_buf: String::new(),
                    }));
                }
                return;
            }
            b"hyperlink" => {
                if !empty {
                    let target = self.resolve_link(e);
                    self.stack.push(Frame::Hyper(HyperB {
                        target,
                        children: Vec::new(),
                    }));
                }
                return;
            }
            b"drawing" => {
                if !empty {
                    self.stack.push(Frame::Draw(DrawB {
                        cx: None,
                        cy: None,
                        embed: None,
                        name: None,
                        extent_seen: false,
                    }));
                }
                return;
            }
            b"sectPr" => {
                if !empty {
                    self.stack.push(Frame::Sect(Section {
                        page_w: None,
                        page_h: None,
                        margin_top: None,
                        margin_right: None,
                        margin_bottom: None,
                        margin_left: None,
                    }));
                }
                return;
            }
            _ => {}
        }

        // 帧内拦截（叶子元素）
        match self.stack.last_mut() {
            Some(Frame::Para(p)) => {
                if name == b"pPr" && !empty {
                    p.in_ppr = true;
                    return;
                }
                if !p.in_ppr {
                    return;
                }
                match name {
                    b"numPr" if !empty => p.in_numpr = true,
                    b"pStyle" => p.style = attr(e, "val", Some(&self.ns)),
                    b"numId" if p.in_numpr => {
                        if let Some(v) = attr(e, "val", Some(&self.ns)).and_then(|v| v.parse().ok())
                        {
                            p.num = Some((v, p.num.map(|(_, l)| l).unwrap_or(0)));
                        }
                    }
                    b"ilvl" if p.in_numpr => {
                        if let Some(v) =
                            attr(e, "val", Some(&self.ns)).and_then(|v: String| v.parse().ok())
                        {
                            p.num = Some((p.num.map(|(i, _)| i).unwrap_or(0), v));
                        }
                    }
                    b"jc" => p.direct.jc = attr(e, "val", Some(&self.ns)).and_then(normalize_jc),
                    b"ind" => {
                        p.direct.left =
                            styles::twips(e, "left").or_else(|| styles::twips(e, "start"));
                        p.direct.right =
                            styles::twips(e, "right").or_else(|| styles::twips(e, "end"));
                        p.direct.first = styles::twips(e, "firstLine");
                        p.direct.hanging = styles::twips(e, "hanging");
                    }
                    _ => {}
                }
            }
            Some(Frame::Run(r)) => {
                if name == b"rPr" && !empty {
                    r.in_rpr = true;
                    return;
                }
                if r.in_rpr {
                    match name {
                        b"rStyle" => r.rstyle = attr(e, "val", Some(&self.ns)),
                        _ => {
                            let got = styles::read_rpr_leaf(e, name);
                            r.direct.overlay(&got);
                        }
                    }
                    return;
                }
                match name {
                    b"t" if !empty => r.in_wt = true,
                    b"instrText" if !empty => {
                        r.in_instr = true;
                        r.instr_buf.clear();
                    }
                    b"tab" => self.push_inline(Inline::Tab),
                    b"br" | b"cr" => {
                        let ty = match attr(e, "type", Some(&self.ns)).as_deref() {
                            Some("page") => BreakType::Page,
                            Some("column") => BreakType::Column,
                            _ => BreakType::Line,
                        };
                        self.push_inline(Inline::Break(ty));
                    }
                    b"footnoteReference" => {
                        if let Some(id) = attr(e, "id", Some(&self.ns)).and_then(|v| v.parse().ok())
                        {
                            self.push_inline(Inline::FootnoteRef(id));
                        }
                    }
                    b"noBreakHyphen" => r.buf.push('-'),
                    _ => {}
                }
            }
            Some(Frame::Draw(d)) => match name {
                b"extent" if !d.extent_seen => {
                    d.cx = attr(e, "cx", Some(&self.ns)).and_then(|v| v.parse().ok());
                    d.cy = attr(e, "cy", Some(&self.ns)).and_then(|v| v.parse().ok());
                    d.extent_seen = true;
                }
                b"blip" if d.embed.is_none() => {
                    d.embed = attr(e, "embed", Some(&self.ns));
                }
                b"docPr" | b"cNvPr" if d.name.is_none() => {
                    d.name = attr(e, "name", Some(&self.ns));
                }
                _ => {}
            },
            Some(Frame::Sect(s)) => match name {
                b"pgSz" => {
                    s.page_w = styles::twips(e, "w");
                    s.page_h = styles::twips(e, "h");
                }
                b"pgMar" => {
                    s.margin_top = styles::twips(e, "top");
                    s.margin_right = styles::twips(e, "right");
                    s.margin_bottom = styles::twips(e, "bottom");
                    s.margin_left = styles::twips(e, "left");
                }
                _ => {}
            },
            Some(Frame::Cell(c)) => {
                if name == b"tcPr" && !empty {
                    c.in_tcpr = true;
                    return;
                }
                if !c.in_tcpr {
                    return;
                }
                match name {
                    b"gridSpan" => {
                        c.col_span = attr(e, "val", Some(&self.ns))
                            .and_then(|v| v.parse().ok())
                            .filter(|v| *v >= 1)
                            .unwrap_or(1);
                    }
                    b"vMerge" => {
                        c.vm = Some(
                            if attr(e, "val", Some(&self.ns)).as_deref() == Some("restart") {
                                Vm::Restart
                            } else {
                                Vm::Continue
                            },
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, raw: &[u8]) {
        // 只有 Run 内的 w:t 文本才进树；instrText 文本单独累积
        if let Some(Frame::Run(r)) = self.stack.last_mut() {
            if r.in_wt {
                match unescape(raw) {
                    Ok(s) => r.buf.push_str(&s),
                    Err(_) => {} // 非法实体：丢弃该片段
                }
            } else if r.in_instr {
                if let Ok(s) = unescape(raw) {
                    r.instr_buf.push_str(&s);
                }
            }
        }
    }

    /// 实体引用事件（&amp; 之类）：解码后并入当前缓冲
    fn general_ref(&mut self, ent: &[u8]) {
        if let Some(Frame::Run(r)) = self.stack.last_mut() {
            if let Ok(c) = crate::xml::decode_entity(ent) {
                if r.in_wt {
                    r.buf.push(c);
                } else if r.in_instr {
                    r.instr_buf.push(c);
                }
            }
        }
    }

    fn end(&mut self, name: &[u8]) {
        // Skip 模式收口
        if let Some(Frame::Skip(d)) = self.stack.last_mut() {
            *d -= 1;
            if *d == 0 {
                self.stack.pop();
            }
            return;
        }
        match name {
            b"tbl" => {
                if matches!(self.stack.last(), Some(Frame::Table(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Table(rows) = f {
                        let t = finalize_table(rows);
                        self.push_block(Block::Table(t));
                    }
                }
            }
            b"tr" => {
                if matches!(self.stack.last(), Some(Frame::Row(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Row(cells) = f {
                        if let Some(Frame::Table(rows)) = self.stack.last_mut() {
                            rows.push(cells);
                        }
                    }
                }
            }
            b"tc" => {
                if matches!(self.stack.last(), Some(Frame::Cell(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Cell(c) = f {
                        if let Some(Frame::Row(cells)) = self.stack.last_mut() {
                            cells.push(c);
                        }
                    }
                }
            }
            b"p" => {
                if matches!(self.stack.last(), Some(Frame::Para(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Para(p) = f {
                        self.finish_para(p);
                    }
                }
            }
            b"r" => {
                if matches!(self.stack.last(), Some(Frame::Run(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Run(r) = f {
                        self.finish_run(r);
                    }
                }
            }
            b"hyperlink" => {
                if matches!(self.stack.last(), Some(Frame::Hyper(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Hyper(h) = f {
                        self.finish_hyper(h);
                    }
                }
            }
            b"drawing" => {
                if matches!(self.stack.last(), Some(Frame::Draw(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Draw(d) = f {
                        self.finish_draw(d);
                    }
                }
            }
            b"sectPr" => {
                if matches!(self.stack.last(), Some(Frame::Sect(_))) {
                    let f = self.stack.pop().unwrap();
                    if let Frame::Sect(s) = f {
                        self.section = Some(s); // 多 section 文档取最后一个
                    }
                }
            }
            b"t" => {
                if let Some(Frame::Run(r)) = self.stack.last_mut() {
                    r.in_wt = false;
                }
            }
            b"instrText" => {
                if let Some(Frame::Run(r)) = self.stack.last_mut() {
                    if r.in_instr {
                        r.in_instr = false;
                        let instr = std::mem::take(&mut r.instr_buf);
                        if !instr.is_empty() {
                            self.push_inline(Inline::Field(Field { instr }));
                        }
                    }
                }
            }
            b"pPr" => {
                if let Some(Frame::Para(p)) = self.stack.last_mut() {
                    p.in_ppr = false;
                }
            }
            b"numPr" => {
                if let Some(Frame::Para(p)) = self.stack.last_mut() {
                    p.in_numpr = false;
                }
            }
            b"rPr" => {
                if let Some(Frame::Run(r)) = self.stack.last_mut() {
                    r.in_rpr = false;
                }
            }
            b"tcPr" => {
                if let Some(Frame::Cell(c)) = self.stack.last_mut() {
                    c.in_tcpr = false;
                }
            }
            _ => {}
        }
    }

    // ---- 收尾 ----

    /// Run 结束：把累积文本按有效格式推入段落
    fn finish_run(&mut self, r: RunB) {
        if !r.buf.is_empty() {
            let eff = self.run_effective(&r);
            self.push_text(&r.buf, &eff);
        }
    }

    fn finish_hyper(&mut self, h: HyperB) {
        if h.target.is_empty() {
            // 无 rels 项：降级为普通文本，直接拼接 children
            for c in h.children {
                self.push_inline(c);
            }
            return;
        }
        self.push_inline(Inline::Hyperlink(Hyperlink {
            target: h.target,
            children: h.children,
        }));
    }

    fn finish_draw(&mut self, d: DrawB) {
        let embed = match d.embed {
            Some(id) => id,
            None => return, // 无内嵌位图（如形状）：跳过
        };
        // rId → zip entry 路径
        let target = match self.rels.get(&embed) {
            Some(rel) if !rel.external => rels::resolve("word/", &rel.target),
            _ => return, // 外链图片/未知关系：跳过
        };
        const EMU_PER_PX: u64 = 9525; // 96dpi
        self.push_inline(Inline::Image(Image {
            target,
            name: d.name,
            width_px: d.cx.filter(|v| *v > 0).map(|v| (v / EMU_PER_PX) as u32),
            height_px: d.cy.filter(|v| *v > 0).map(|v| (v / EMU_PER_PX) as u32),
        }));
    }

    fn finish_para(&mut self, p: ParaB) {
        let eff = self.styles.effective_ppr(p.style.as_deref(), &p.direct);
        // 空段裁剪：无内容且无任何有效属性（Word 常在表格后留结构性空段）
        if p.children.is_empty() && p.style.is_none() && p.num.is_none() && eff == PPr::default() {
            return;
        }
        let numbering = p.num.map(|(id, level)| {
            let marker = self
                .numbering
                .and_then(|n| n.marker(id, level, &mut self.counters));
            Numbering { id, level, marker }
        });
        self.push_block(Block::Paragraph(Paragraph {
            style: p.style,
            numbering,
            alignment: eff.jc,
            indent_left: eff.left,
            indent_right: eff.right,
            indent_first_line: eff.first,
            indent_hanging: eff.hanging,
            children: p.children,
        }));
    }

    // ---- 公共辅助 ----

    /// 就近计算 Run 有效格式
    fn run_effective(&mut self, r: &RunB) -> RPr {
        let para_style = self.stack.iter().rev().find_map(|f| match f {
            Frame::Para(p) => p.style.clone(),
            _ => None,
        });
        self.styles
            .effective_rpr(para_style.as_deref(), r.rstyle.as_deref(), &r.direct)
    }

    /// 把 inline 节点推进最近的 Hyperlink/Paragraph 容器
    fn push_inline(&mut self, inl: Inline) {
        // 先尝试与容器内最后一个节点就地合并
        let merged = match (&inl, self.last_inline_mut()) {
            // 相邻同格式文本合并（Word 常把一句话拆成多个 w:t）
            (Inline::Text(t), Some(Inline::Text(lt))) if lt.same_props(t) => {
                lt.text.push_str(&t.text);
                true
            }
            // 同域的 instrText 分片合并
            (Inline::Field(f), Some(Inline::Field(lf))) => {
                lf.instr.push_str(&f.instr);
                true
            }
            _ => false,
        };
        if merged {
            return;
        }
        for f in self.stack.iter_mut().rev() {
            match f {
                Frame::Hyper(h) => {
                    h.children.push(inl);
                    return;
                }
                Frame::Para(p) => {
                    p.children.push(inl);
                    return;
                }
                _ => {}
            }
        }
        // 无接收容器（如游离 run）：丢弃
    }

    /// 就近找最近 inline 容器里的最后一个节点
    fn last_inline_mut(&mut self) -> Option<&mut Inline> {
        for f in self.stack.iter_mut().rev() {
            match f {
                Frame::Hyper(h) => return h.children.last_mut(),
                Frame::Para(p) => return p.children.last_mut(),
                _ => {}
            }
        }
        None
    }

    fn push_text(&mut self, text: &str, eff: &RPr) {
        self.push_inline(Inline::Text(TextRun {
            text: String::from(text),
            b: eff.b,
            i: eff.i,
            u: eff.u,
            strike: eff.strike,
            color: eff.color.clone(),
            sz: eff.sz,
            font: eff.font.clone(),
        }));
    }

    /// 把 block 推进最近的 Body/Cell 容器
    fn push_block(&mut self, blk: Block) {
        for f in self.stack.iter_mut().rev() {
            match f {
                Frame::Body(v) => {
                    v.push(blk);
                    return;
                }
                Frame::Cell(c) => {
                    c.blocks.push(blk);
                    return;
                }
                _ => {}
            }
        }
    }

    /// 解析 w:hyperlink 的目标：r:id → rels；无则用 w:anchor → "#锚点"
    fn resolve_link(&self, e: &BytesStart) -> String {
        if let Some(rid) = attr(e, "id", Some(&self.ns)) {
            if let Some(rel) = self.rels.get(&rid) {
                if rel.external {
                    return rel.target.clone();
                }
                return rels::resolve("word/", &rel.target);
            }
        }
        if let Some(anchor) = attr(e, "anchor", Some(&self.ns)) {
            let mut s = String::with_capacity(anchor.len() + 1);
            s.push('#');
            s.push_str(&anchor);
            return s;
        }
        String::new()
    }
}

/// 表格收尾：vMerge → rowSpan 计算，并移除被合并吞掉的 continue 单元格
fn finalize_table(rows: Vec<Vec<CellB>>) -> Table {
    let mut rows = rows;
    let mut removed: Vec<Vec<bool>> = rows.iter().map(|r| vec![false; r.len()]).collect();
    for i in 0..rows.len() {
        let mut col = 0u32;
        for j in 0..rows[i].len() {
            let span = rows[i][j].col_span.max(1);
            if rows[i][j].vm == Some(Vm::Restart) {
                // 向下扫描同列的 continue 单元格
                let mut row_span = 1u32;
                let mut k = i + 1;
                while k < rows.len() {
                    match cell_at_col(&rows[k], col) {
                        Some(idx) if rows[k][idx].vm == Some(Vm::Continue) => {
                            removed[k][idx] = true;
                            row_span += 1;
                            k += 1;
                        }
                        _ => break,
                    }
                }
                rows[i][j].row_span = row_span;
            }
            col += span;
        }
    }
    let out_rows = rows
        .into_iter()
        .enumerate()
        .map(|(i, cells)| TableRow {
            cells: cells
                .into_iter()
                .enumerate()
                .filter(|(j, _)| !removed[i][*j])
                .map(|(_, c)| TableCell {
                    col_span: c.col_span,
                    row_span: c.row_span,
                    blocks: c.blocks,
                })
                .collect(),
        })
        .collect();
    Table { rows: out_rows }
}

/// 行内列位定位
fn cell_at_col(row: &[CellB], col: u32) -> Option<usize> {
    let mut cur = 0u32;
    for (i, c) in row.iter().enumerate() {
        if cur == col {
            return Some(i);
        }
        cur += c.col_span.max(1);
    }
    None
}
