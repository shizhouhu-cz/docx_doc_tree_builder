//! 渲染层（design.md §11）：DocTree → HTML/DOM，纯 TS，零 wasm 体积成本。
//!
//! 双输出形态共享同一条 HTML 生成管线：
//! - renderToHTML(tree) -> string
//! - renderToDOM(tree)  -> HTMLElement（容器 + innerHTML）
//!
//! 图片：通过 opts.fetchImage（通常包一层 DocxTreeParser.entry）按需取
//! zip entry 字节，转 Blob → objectURL 作为 <img src>（design.md §11.2）。
//! 注意：objectURL 的 revoke 归调用方管理（dispose 时统一处理）。

import type {
  Block,
  DocTree,
  ImageInline,
  Inline,
  ParagraphBlock,
  TableBlock,
} from "./types.js";

export interface RenderOptions {
  /** 取图片 zip entry 字节；返回 null/undefined 时 img 不带 src */
  fetchImage?: (target: string) => Promise<Uint8Array | null>;
}

/** 内置样式表（§11.4：类承载通用态，文档覆盖项用内联 style） */
export const DOCX_CSS = `
.docx-root{line-height:1.5}
.docx-root p{margin:0 0 .5em}
.docx-root h1,.docx-root h2,.docx-root h3,.docx-root h4,.docx-root h5,.docx-root h6{margin:.8em 0 .4em}
.docx-b{font-weight:700}.docx-i{font-style:italic}
.docx-u{text-decoration:underline}.docx-strike{text-decoration:line-through}
.docx-center{text-align:center}.docx-right{text-align:right}.docx-justify{text-align:justify}
.docx-num{padding-left:2.5em;text-indent:-2.5em}
.docx-marker{display:inline-block;min-width:2em}
.docx-tab{display:inline-block;width:4ch}
.docx-pagebreak{break-after:page;height:0}
.docx-root table{border-collapse:collapse;margin:.5em 0}
.docx-root td{border:1px solid #c0c0c0;padding:2px 6px;vertical-align:top}
.docx-root img{max-width:100%}
`;

const TWIPS_PER_PX = 15; // 96dpi：1px = 15 twip

export async function renderToHTML(
  tree: DocTree,
  opts?: RenderOptions,
): Promise<string> {
  // 预取全部图片字节 → blob URL（并发、按 target 去重）
  const imgUrls = new Map<string, string>();
  if (opts?.fetchImage) {
    const targets = [...new Set(collectImageTargets(tree.body))];
    await Promise.all(
      targets.map(async (t) => {
        const bytes = await opts.fetchImage!(t);
        if (bytes && bytes.byteLength > 0) {
          // slice() 保证独立缓冲（防视图共享大 buffer），BlobPart 需纯 ArrayBuffer
          const ab = bytes.slice().buffer as ArrayBuffer;
          imgUrls.set(t, URL.createObjectURL(new Blob([ab])));
        }
      }),
    );
  }
  return blocksHTML(tree, tree.body, imgUrls);
}

export async function renderToDOM(
  tree: DocTree,
  opts?: RenderOptions,
): Promise<HTMLElement> {
  const html = await renderToHTML(tree, opts);
  const root = document.createElement("div");
  root.className = "docx-root";
  root.innerHTML = html;
  return root;
}

// ---- 收集图片引用（预取用） ----

function collectImageTargets(blocks: Block[]): string[] {
  const out: string[] = [];
  const walkInline = (cs: Inline[]) => {
    for (const c of cs) {
      if (c.type === "image") out.push(c.target);
      else if (c.type === "hyperlink") walkInline(c.children);
    }
  };
  const walkBlocks = (bs: Block[]) => {
    for (const b of bs) {
      if (b.type === "paragraph") walkInline(b.children);
      else {
        for (const row of b.rows) {
          for (const cell of row.cells) walkBlocks(cell.blocks);
        }
      }
    }
  };
  walkBlocks(blocks);
  return out;
}

// ---- HTML 生成 ----

function blocksHTML(tree: DocTree, bs: Block[], imgs: Map<string, string>): string {
  let s = "";
  for (const b of bs) {
    s += b.type === "paragraph"
      ? paraHTML(tree, b, imgs)
      : tableHTML(tree, b, imgs);
  }
  return s;
}

function paraHTML(
  tree: DocTree,
  p: ParagraphBlock,
  imgs: Map<string, string>,
): string {
  const level = headingLevel(tree, p);
  const tag = level > 0 ? `h${level}` : "p";
  const cls: string[] = [];
  if (p.numbering) cls.push("docx-num");
  if (p.alignment && p.alignment !== "left") cls.push(`docx-${p.alignment}`);
  let style = "";
  if (p.indent) {
    const parts: string[] = [];
    // 首行缩进优先；hanging（悬挂缩进）与 firstLine 互斥，hanging 更常见于列表
    if (p.indent.firstLine) parts.push(`text-indent:${(p.indent.firstLine / TWIPS_PER_PX).toFixed(2)}px`);
    if (p.indent.left) parts.push(`margin-left:${(p.indent.left / TWIPS_PER_PX).toFixed(2)}px`);
    if (p.indent.right) parts.push(`margin-right:${(p.indent.right / TWIPS_PER_PX).toFixed(2)}px`);
    if (parts.length) style = parts.join(";");
  }
  const attr =
    (cls.length ? ` class="${cls.join(" ")}"` : "") +
    (style ? ` style="${style}"` : "");
  const marker = p.numbering?.marker
    ? `<span class="docx-marker">${esc(p.numbering.marker)}</span>`
    : "";
  return `<${tag}${attr}>${marker}${inlinesHTML(p.children, imgs)}</${tag}>`;
}

function tableHTML(tree: DocTree, t: TableBlock, imgs: Map<string, string>): string {
  let s = "<table>";
  for (const row of t.rows) {
    s += "<tr>";
    for (const cell of row.cells) {
      const attr =
        (cell.colSpan && cell.colSpan > 1 ? ` colspan="${cell.colSpan}"` : "") +
        (cell.rowSpan && cell.rowSpan > 1 ? ` rowspan="${cell.rowSpan}"` : "");
      s += `<td${attr}>${blocksHTML(tree, cell.blocks, imgs)}</td>`;
    }
    s += "</tr>";
  }
  return s + "</table>";
}

function inlinesHTML(cs: Inline[], imgs: Map<string, string>): string {
  let s = "";
  for (const c of cs) {
    switch (c.type) {
      case "text": {
        const cls: string[] = [];
        if (c.bold) cls.push("docx-b");
        if (c.italic) cls.push("docx-i");
        if (c.underline) cls.push("docx-u");
        if (c.strike) cls.push("docx-strike");
        const parts: string[] = [];
        if (c.color) parts.push(`color:#${c.color}`);
        if (c.sizePt) parts.push(`font-size:${c.sizePt}pt`);
        if (c.font) parts.push(`font-family:${cssString(c.font)}`);
        const attr =
          (cls.length ? ` class="${cls.join(" ")}"` : "") +
          (parts.length ? ` style="${parts.join(";")}"` : "");
        s += `<span${attr}>${esc(c.text)}</span>`;
        break;
      }
      case "tab":
        s += `<span class="docx-tab"></span>`;
        break;
      case "break":
        s += c.breakType === "line" ? "<br>" : `<div class="docx-pagebreak"></div>`;
        break;
      case "hyperlink":
        s += `<a href="${esc(c.target)}">${inlinesHTML(c.children, imgs)}</a>`;
        break;
      case "image":
        s += imgHTML(c, imgs);
        break;
      // v1 骨架节点：field（域指令）/ footnoteRef 无可渲染内容
      case "field":
      case "footnoteRef":
        break;
    }
  }
  return s;
}

function imgHTML(img: ImageInline, imgs: Map<string, string>): string {
  const src = imgs.get(img.target);
  const attr =
    (src ? ` src="${src}"` : "") +
    (img.widthPx ? ` width="${img.widthPx}"` : "") +
    (img.heightPx ? ` height="${img.heightPx}"` : "") +
    (img.name ? ` alt="${esc(img.name)}"` : "");
  return `<img${attr}>`;
}

/** 标题级别：style id "HeadingN" 或样式索引 name "heading N" 均识别 */
function headingLevel(tree: DocTree, p: ParagraphBlock): number {
  if (!p.style) return 0;
  const m = /^heading[ ]?([1-6])$/i.exec(p.style);
  if (m) return +m[1];
  const s = tree.styles?.find((st) => st.id === p.style);
  if (s?.name) {
    const mn = /^heading[ ]?([1-6])$/i.exec(s.name);
    if (mn) return +mn[1];
  }
  return 0;
}

/** CSS 字体名（含引号包裹，防字体名带空格/特殊字符） */
function cssString(font: string): string {
  return `'${font.replace(/'/g, "\\'")}'`;
}

/** HTML 文本转义（属性值同样适用） */
function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
