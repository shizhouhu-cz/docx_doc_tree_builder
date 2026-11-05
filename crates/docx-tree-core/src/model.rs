//! 语义数据模型（design.md §4）。
//! 刻意与 JSON schema 一一对应，序列化零转换。

/// 文档树根
pub struct DocTree {
    pub body: Vec<Block>,
    /// 页面设置（来自 sectPr），仅一个 section（多 section 文档取最后一个）
    pub section: Option<Section>,
    /// 段落样式索引 [(id, name)]，供上层把本地化 styleId 映射为 heading N
    pub styles: Vec<StyleIndex>,
}

pub struct StyleIndex {
    pub id: String,
    pub name: Option<String>,
}

/// 页面设置，单位 twip
pub struct Section {
    pub page_w: Option<i64>,
    pub page_h: Option<i64>,
    pub margin_top: Option<i64>,
    pub margin_right: Option<i64>,
    pub margin_bottom: Option<i64>,
    pub margin_left: Option<i64>,
}

pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

pub struct Paragraph {
    /// pStyle 引用的样式 id（保留原始引用）
    pub style: Option<String>,
    pub numbering: Option<Numbering>,
    /// 有效值（样式链解析后），"left"|"center"|"right"|"justify"
    pub alignment: Option<String>,
    /// 有效值，单位 twip
    pub indent_left: Option<i64>,
    pub indent_right: Option<i64>,
    pub indent_first_line: Option<i64>,
    pub indent_hanging: Option<i64>,
    pub children: Vec<Inline>,
}

pub struct Numbering {
    /// numId
    pub id: u32,
    /// ilvl（0 起）
    pub level: u8,
    /// 按 numFmt+lvlText 现场生成的编号文本（如 "1."、"a)"、"一、"）
    pub marker: Option<String>,
}

pub struct Table {
    pub rows: Vec<TableRow>,
}

pub struct TableRow {
    pub cells: Vec<TableCell>,
}

pub struct TableCell {
    /// gridSpan：跨列数（1 时序列化省略）
    pub col_span: u32,
    /// vMerge 计算出的跨行数（1 时序列化省略）
    pub row_span: u32,
    pub blocks: Vec<Block>,
}

pub enum Inline {
    Text(TextRun),
    Tab,
    Break(BreakType),
    Hyperlink(Hyperlink),
    Image(Image),
    FootnoteRef(u32),
    Field(Field),
}

pub struct TextRun {
    pub text: String,
    /// 有效格式（docDefaults → 样式链 → 直接格式 合并后）
    pub b: Option<bool>,
    pub i: Option<bool>,
    pub u: Option<bool>,
    pub strike: Option<bool>,
    /// 十六进制 RGB，如 "FF0000"
    pub color: Option<String>,
    /// 半磅（w:sz 原始值），序列化为 sizePt
    pub sz: Option<u32>,
    pub font: Option<String>,
}

impl TextRun {
    /// 相邻文本节点合并判定：有效格式完全相同
    pub fn same_props(&self, o: &TextRun) -> bool {
        self.b == o.b
            && self.i == o.i
            && self.u == o.u
            && self.strike == o.strike
            && self.color == o.color
            && self.sz == o.sz
            && self.font == o.font
    }
}

pub enum BreakType {
    Line,
    Page,
    Column,
}

pub struct Hyperlink {
    /// rId 已解析：外链为 URL；内链为 entry 路径或 "#锚点"
    pub target: String,
    pub children: Vec<Inline>,
}

pub struct Image {
    /// 解析为 zip entry 路径，如 "word/media/image1.png"
    pub target: String,
    pub name: Option<String>,
    /// EMU → px（96dpi）
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
}

pub struct Field {
    /// 域指令文本（如 TOC、PAGE）
    pub instr: String,
}
