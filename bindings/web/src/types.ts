//! DocTree 前端类型声明（design.md §4.2，与 core JSON 序列化输出一一对应）。
//! 作为 Rust ↔ TS 的契约文件：字段可选性 = JSON 中省略 key 的语义。

export interface StyleIndex {
  id: string;
  name?: string;
}

export interface SectionMargin {
  top?: number;
  right?: number;
  bottom?: number;
  left?: number;
}

export interface SectionProps {
  pageW?: number; // twip
  pageH?: number; // twip
  margin?: SectionMargin;
}

export interface NumberingInfo {
  id: number;
  level: number;
  /** 现场生成的编号文本，如 "1."、"a)"、"一、"（bullet 时为空串） */
  marker?: string;
}

export type Alignment = "left" | "center" | "right" | "justify";

/** 缩进，单位 twip（1/20 磅；96dpi 下 1px = 15 twip） */
export interface Indent {
  left?: number;
  right?: number;
  firstLine?: number;
  hanging?: number;
}

export interface TextInline {
  type: "text";
  text: string;
  bold?: true;
  italic?: true;
  underline?: true;
  strike?: true;
  color?: string; // 十六进制 RGB，如 "1F4E79"
  sizePt?: number; // 字号（磅，半磅值已换算）
  font?: string; // 主字体名
}

export interface TabInline {
  type: "tab";
}

export interface BreakInline {
  type: "break";
  breakType: "line" | "page" | "column";
}

export interface HyperlinkInline {
  type: "hyperlink";
  target: string; // rId 已在解析期解析为 URL（内链为 #anchor）
  children: Inline[];
}

export interface ImageInline {
  type: "image";
  target: string; // zip entry 名，如 "word/media/image1.png"
  name?: string;
  widthPx?: number;
  heightPx?: number;
}

export interface FootnoteRefInline {
  type: "footnoteRef";
  id: number;
}

export interface FieldInline {
  type: "field";
  instr: string;
}

export type Inline =
  | TextInline
  | TabInline
  | BreakInline
  | HyperlinkInline
  | ImageInline
  | FootnoteRefInline
  | FieldInline;

export interface ParagraphBlock {
  type: "paragraph";
  style?: string; // pStyle 引用的样式 id
  numbering?: NumberingInfo;
  alignment?: Alignment;
  indent?: Indent;
  children: Inline[];
}

export interface TableCell {
  colSpan?: number; // gridSpan
  rowSpan?: number; // vMerge(restart)
  blocks: Block[];
}

export interface TableRow {
  cells: TableCell[];
}

export interface TableBlock {
  type: "table";
  rows: TableRow[];
}

export type Block = ParagraphBlock | TableBlock;

export interface DocTree {
  version: "1";
  styles?: StyleIndex[];
  body: Block[];
  section?: SectionProps;
}
