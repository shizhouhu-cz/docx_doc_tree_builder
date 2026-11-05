var c=class extends Error{constructor(t,r){super(t);this.code=r}},y=class u{ex;decoder=new TextDecoder;constructor(e){this.ex=e}static async instantiate(e){let t=e instanceof ArrayBuffer?e:await(await fetch(e)).arrayBuffer(),{instance:r}=await WebAssembly.instantiate(t,{});return new u(r.exports)}parse(e){let t=e.byteLength;if(t===0)throw new c("empty input",1);let r=this.ex.docx_alloc(t);new Uint8Array(this.ex.memory.buffer,r,t).set(e);let s=this.ex.docx_alloc(8),n=this.ex.docx_parse(r,t,s,s+4);if(this.ex.docx_free(r,t),n!==0)throw this.ex.docx_free(s,8),new c(this.errMsg(),n);let o=new DataView(this.ex.memory.buffer),i=o.getUint32(s,!0),a=o.getUint32(s+4,!0),d=this.decoder.decode(new Uint8Array(this.ex.memory.buffer,i,a));return this.ex.docx_result_free(i,a),this.ex.docx_free(s,8),JSON.parse(d)}entry(e){let t=new TextEncoder().encode(e),r=this.ex.docx_alloc(t.byteLength);new Uint8Array(this.ex.memory.buffer,r,t.byteLength).set(t);let s=this.ex.docx_alloc(8),n=this.ex.docx_entry(r,t.byteLength,s,s+4);if(this.ex.docx_free(r,t.byteLength),n===2)return this.ex.docx_free(s,8),null;if(n!==0)throw this.ex.docx_free(s,8),new c(this.errMsg(),n);let o=new DataView(this.ex.memory.buffer),i=o.getUint32(s,!0),a=o.getUint32(s+4,!0),d=new Uint8Array(this.ex.memory.buffer,i,a).slice();return this.ex.docx_result_free(i,a),this.ex.docx_free(s,8),d}errMsg(){let e=this.ex.docx_alloc(1024),t=this.ex.docx_err_msg(e,1024),r=this.decoder.decode(new Uint8Array(this.ex.memory.buffer,e,t));return this.ex.docx_free(e,1024),r}};var m=null;self.onmessage=async u=>{let e=u.data;switch(e.type){case"init":{try{m=await y.instantiate(e.wasmUrl),self.postMessage({type:"ready"})}catch(t){self.postMessage({type:"error",id:-1,message:`wasm init failed: ${t.message}`,code:0})}break}case"parse":{if(!m){self.postMessage({type:"error",id:e.id,message:"worker not initialized",code:1});return}try{let t=m.parse(new Uint8Array(e.buffer));self.postMessage({type:"parsed",id:e.id,tree:t})}catch(t){self.postMessage({type:"error",id:e.id,message:t.message,code:t.code??1})}break}case"entry":{if(!m){self.postMessage({type:"entry",id:e.id,bytes:null});return}try{let t=m.entry(e.name);if(t){let r=t.buffer;self.postMessage({type:"entry",id:e.id,bytes:r},[r])}else self.postMessage({type:"entry",id:e.id,bytes:null})}catch{self.postMessage({type:"entry",id:e.id,bytes:null})}break}}};
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
//! Worker 入口：持有唯一 wasm 实例（docx_parse 后保留的字节状态
//! 与 docx_entry 共享，因此 parse 与图片提取必须在同一线程）。
//! 协议（主线程 <-> worker，全部带递增 id 配对）：
//! - { type: "init", wasmUrl }            → { type: "ready" }
//! - { type: "parse", id, buffer }        → { type: "parsed", id, tree }
//!                                          | { type: "error", id, message, code }
//! - { type: "entry", id, name }          → { type: "entry", id, bytes: ArrayBuffer | null }
//!                                          （bytes 为 transferable 零拷贝转移）
