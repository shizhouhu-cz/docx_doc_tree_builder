var c=class extends Error{constructor(r,t){super(r);this.code=t}};var T=`
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
`;async function m(n,e){let r=new Map;if(e?.fetchImage){let t=[...new Set(x(n.body))];await Promise.all(t.map(async o=>{let i=await e.fetchImage(o);if(i&&i.byteLength>0){let s=i.slice().buffer;r.set(o,URL.createObjectURL(new Blob([s])))}}))}return u(n,n.body,r)}async function I(n,e){let r=await m(n,e),t=document.createElement("div");return t.className="docx-root",t.innerHTML=r,t}function x(n){let e=[],r=o=>{for(let i of o)i.type==="image"?e.push(i.target):i.type==="hyperlink"&&r(i.children)},t=o=>{for(let i of o)if(i.type==="paragraph")r(i.children);else for(let s of i.rows)for(let p of s.cells)t(p.blocks)};return t(n),e}function u(n,e,r){let t="";for(let o of e)t+=o.type==="paragraph"?h(n,o,r):b(n,o,r);return t}function h(n,e,r){let t=w(n,e),o=t>0?`h${t}`:"p",i=[];e.numbering&&i.push("docx-num"),e.alignment&&e.alignment!=="left"&&i.push(`docx-${e.alignment}`);let s="";if(e.indent){let a=[];e.indent.firstLine&&a.push(`text-indent:${(e.indent.firstLine/15).toFixed(2)}px`),e.indent.left&&a.push(`margin-left:${(e.indent.left/15).toFixed(2)}px`),e.indent.right&&a.push(`margin-right:${(e.indent.right/15).toFixed(2)}px`),a.length&&(s=a.join(";"))}let p=(i.length?` class="${i.join(" ")}"`:"")+(s?` style="${s}"`:""),g=e.numbering?.marker?`<span class="docx-marker">${l(e.numbering.marker)}</span>`:"";return`<${o}${p}>${g}${f(e.children,r)}</${o}>`}function b(n,e,r){let t="<table>";for(let o of e.rows){t+="<tr>";for(let i of o.cells){let s=(i.colSpan&&i.colSpan>1?` colspan="${i.colSpan}"`:"")+(i.rowSpan&&i.rowSpan>1?` rowspan="${i.rowSpan}"`:"");t+=`<td${s}>${u(n,i.blocks,r)}</td>`}t+="</tr>"}return t+"</table>"}function f(n,e){let r="";for(let t of n)switch(t.type){case"text":{let o=[];t.bold&&o.push("docx-b"),t.italic&&o.push("docx-i"),t.underline&&o.push("docx-u"),t.strike&&o.push("docx-strike");let i=[];t.color&&i.push(`color:#${t.color}`),t.sizePt&&i.push(`font-size:${t.sizePt}pt`),t.font&&i.push(`font-family:${k(t.font)}`);let s=(o.length?` class="${o.join(" ")}"`:"")+(i.length?` style="${i.join(";")}"`:"");r+=`<span${s}>${l(t.text)}</span>`;break}case"tab":r+='<span class="docx-tab"></span>';break;case"break":r+=t.breakType==="line"?"<br>":'<div class="docx-pagebreak"></div>';break;case"hyperlink":r+=`<a href="${l(t.target)}">${f(t.children,e)}</a>`;break;case"image":r+=y(t,e);break;case"field":case"footnoteRef":break}return r}function y(n,e){let r=e.get(n.target);return`<img${(r?` src="${r}"`:"")+(n.widthPx?` width="${n.widthPx}"`:"")+(n.heightPx?` height="${n.heightPx}"`:"")+(n.name?` alt="${l(n.name)}"`:"")}>`}function w(n,e){if(!e.style)return 0;let r=/^heading[ ]?([1-6])$/i.exec(e.style);if(r)return+r[1];let t=n.styles?.find(o=>o.id===e.style);if(t?.name){let o=/^heading[ ]?([1-6])$/i.exec(t.name);if(o)return+o[1]}return 0}function k(n){return`'${n.replace(/'/g,"\\'")}'`}function l(n){return n.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;")}var d=class{worker;pending=new Map;nextId=1;ready;disposed=!1;constructor(e,r){this.worker=new Worker(e,{type:"module"}),this.worker.onmessage=t=>this.onReply(t.data),this.worker.onerror=t=>{for(let o of this.pending.values())o.reject(new Error(`worker error: ${t.message??"unknown"}`));this.pending.clear()},this.ready=new Promise((t,o)=>{this.pending.set(-1,{resolve:t,reject:o})}),this.worker.postMessage({type:"init",wasmUrl:r})}onReply(e){if(e.type==="ready"){this.pending.get(-1)?.resolve(void 0),this.pending.delete(-1);return}let r=this.pending.get(e.id);if(r)switch(this.pending.delete(e.id),e.type){case"parsed":r.resolve(e.tree);break;case"entry":r.resolve(e.bytes?new Uint8Array(e.bytes):null);break;case"error":r.reject(new c(e.message,e.code));break}}request(e){return this.disposed?Promise.reject(new Error("parser disposed")):new Promise((r,t)=>{this.pending.set(e,{resolve:r,reject:t})})}async parse(e){await this.ready;let r=this.nextId++;return this.worker.postMessage({type:"parse",id:r,buffer:e},[e]),this.request(r)}async entry(e){await this.ready;let r=this.nextId++;return this.worker.postMessage({type:"entry",id:r,name:e}),this.request(r)}dispose(){this.disposed=!0;for(let e of this.pending.values())e.reject(new Error("parser disposed"));this.pending.clear(),this.worker.terminate()}};async function $(n,e,r){let t=new d(e,r);try{return await t.parse(n)}finally{t.dispose()}}export{T as DOCX_CSS,d as DocxTreeParser,$ as parseDocx,I as renderToDOM,m as renderToHTML};
//! wasm 胶水：加载 docx_tree_wasm.wasm，封装裸 FFI 为类型安全的 TS API。
//!
//! FFI 协议（design.md §3.4）：
//! - docx_alloc(len) -> ptr：分配线性内存，JS 侧用 Uint8Array 视图拷入字节
//! - docx_parse(in_ptr, in_len, out_ptr, out_len) -> 错误码（0 成功）
//! - docx_entry(name_ptr, name_len, out_ptr, out_len) -> 错误码（2 = 不存在）
//! - docx_err_msg(buf, cap) -> 写入字节数
//! - docx_free / docx_result_free：按 (ptr, len) 重建 Vec 后 drop
//! 注意：wasm 内存可能因分配而 grow，旧 ArrayBuffer 视图会失效
//! （表现为 byteLength 归零），所以每次 FFI 调用后都重建视图。
//! DocTree 前端类型声明（design.md §4.2，与 core JSON 序列化输出一一对应）。
//! 作为 Rust ↔ TS 的契约文件：字段可选性 = JSON 中省略 key 的语义。
//! 渲染层（design.md §11）：DocTree → HTML/DOM，纯 TS，零 wasm 体积成本。
//! 双输出形态共享同一条 HTML 生成管线：
//! - renderToHTML(tree) -> string
//! - renderToDOM(tree)  -> HTMLElement（容器 + innerHTML）
//! 图片：通过 opts.fetchImage（通常包一层 DocxTreeParser.entry）按需取
//! zip entry 字节，转 Blob → objectURL 作为 <img src>（design.md §11.2）。
//! 注意：objectURL 的 revoke 归调用方管理（dispose 时统一处理）。
//! 主线程 API：封装 Worker 通信，提供 parseDocx 与按需 entry 提取。
//! 典型用法：
//! ```ts
//! const parser = new DocxTreeParser("dist/docx_worker.js", "dist/docx_tree_wasm.wasm");
//! const tree = await parser.parse(await file.arrayBuffer());
//! const root = await renderToDOM(tree, {
//!   fetchImage: (t) => parser.entry(t), // 图片字节 → blob URL 一步到位
//! });
//! ```
