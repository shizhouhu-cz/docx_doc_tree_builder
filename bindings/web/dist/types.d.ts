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
    pageW?: number;
    pageH?: number;
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
    color?: string;
    sizePt?: number;
    font?: string;
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
    target: string;
    children: Inline[];
}
export interface ImageInline {
    type: "image";
    target: string;
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
export type Inline = TextInline | TabInline | BreakInline | HyperlinkInline | ImageInline | FootnoteRefInline | FieldInline;
export interface ParagraphBlock {
    type: "paragraph";
    style?: string;
    numbering?: NumberingInfo;
    alignment?: Alignment;
    indent?: Indent;
    children: Inline[];
}
export interface TableCell {
    colSpan?: number;
    rowSpan?: number;
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
