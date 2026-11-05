import type { DocTree } from "./types.js";
export interface RenderOptions {
    /** 取图片 zip entry 字节；返回 null/undefined 时 img 不带 src */
    fetchImage?: (target: string) => Promise<Uint8Array | null>;
}
/** 内置样式表（§11.4：类承载通用态，文档覆盖项用内联 style） */
export declare const DOCX_CSS = "\n.docx-root{line-height:1.5}\n.docx-root p{margin:0 0 .5em}\n.docx-root h1,.docx-root h2,.docx-root h3,.docx-root h4,.docx-root h5,.docx-root h6{margin:.8em 0 .4em}\n.docx-b{font-weight:700}.docx-i{font-style:italic}\n.docx-u{text-decoration:underline}.docx-strike{text-decoration:line-through}\n.docx-center{text-align:center}.docx-right{text-align:right}.docx-justify{text-align:justify}\n.docx-num{padding-left:2.5em;text-indent:-2.5em}\n.docx-marker{display:inline-block;min-width:2em}\n.docx-tab{display:inline-block;width:4ch}\n.docx-pagebreak{break-after:page;height:0}\n.docx-root table{border-collapse:collapse;margin:.5em 0}\n.docx-root td{border:1px solid #c0c0c0;padding:2px 6px;vertical-align:top}\n.docx-root img{max-width:100%}\n";
export declare function renderToHTML(tree: DocTree, opts?: RenderOptions): Promise<string>;
export declare function renderToDOM(tree: DocTree, opts?: RenderOptions): Promise<HTMLElement>;
