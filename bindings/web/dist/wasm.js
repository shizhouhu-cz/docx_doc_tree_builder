//! wasm 胶水：加载 docx_tree_wasm.wasm，封装裸 FFI 为类型安全的 TS API。
//!
//! FFI 协议（design.md §3.4）：
//! - docx_alloc(len) -> ptr：分配线性内存，JS 侧用 Uint8Array 视图拷入字节
//! - docx_parse(in_ptr, in_len, out_ptr, out_len) -> 错误码（0 成功）
//! - docx_entry(name_ptr, name_len, out_ptr, out_len) -> 错误码（2 = 不存在）
//! - docx_err_msg(buf, cap) -> 写入字节数
//! - docx_free / docx_result_free：按 (ptr, len) 重建 Vec 后 drop
//!
//! 注意：wasm 内存可能因分配而 grow，旧 ArrayBuffer 视图会失效
//! （表现为 byteLength 归零），所以每次 FFI 调用后都重建视图。
const ERR_CAP = 1024; // 错误消息缓冲（core 侧均为短静态字符串，足够）
export class DocxWasmError extends Error {
    code;
    constructor(message, 
    /** wasm FFI 错误码：1 一般错误 / 2 NotFound / 3 panic / 4 非 UTF-8 */
    code) {
        super(message);
        this.code = code;
    }
}
export class DocxWasm {
    ex;
    decoder = new TextDecoder();
    constructor(ex) {
        this.ex = ex;
    }
    /** 从 URL 或已取得的字节实例化（wasm 无 imports，能力边界即线性内存） */
    static async instantiate(source) {
        const bytes = source instanceof ArrayBuffer
            ? source
            : await (await fetch(source)).arrayBuffer();
        const { instance } = await WebAssembly.instantiate(bytes, {});
        return new DocxWasm(instance.exports);
    }
    /** 解析 docx 字节 → DocTree；失败抛 DocxWasmError */
    parse(input) {
        const len = input.byteLength;
        if (len === 0)
            throw new DocxWasmError("empty input", 1);
        const inPtr = this.ex.docx_alloc(len);
        new Uint8Array(this.ex.memory.buffer, inPtr, len).set(input);
        // 8 字节 out 槽位：(ptr: u32, len: u32) 小端
        const outPtr = this.ex.docx_alloc(8);
        const code = this.ex.docx_parse(inPtr, len, outPtr, outPtr + 4);
        // core 侧已在 STATE.retained 保留副本，输入缓冲可立即释放
        this.ex.docx_free(inPtr, len);
        if (code !== 0) {
            this.ex.docx_free(outPtr, 8);
            throw new DocxWasmError(this.errMsg(), code);
        }
        const view = new DataView(this.ex.memory.buffer);
        const ptr = view.getUint32(outPtr, true);
        const jsonLen = view.getUint32(outPtr + 4, true);
        const json = this.decoder.decode(new Uint8Array(this.ex.memory.buffer, ptr, jsonLen));
        this.ex.docx_result_free(ptr, jsonLen);
        this.ex.docx_free(outPtr, 8);
        return JSON.parse(json);
    }
    /** 按 entry 名提取解压字节（依赖上次 parse 保留的状态）；不存在返回 null */
    entry(name) {
        const nameBytes = new TextEncoder().encode(name);
        const namePtr = this.ex.docx_alloc(nameBytes.byteLength);
        new Uint8Array(this.ex.memory.buffer, namePtr, nameBytes.byteLength).set(nameBytes);
        const outPtr = this.ex.docx_alloc(8);
        const code = this.ex.docx_entry(namePtr, nameBytes.byteLength, outPtr, outPtr + 4);
        this.ex.docx_free(namePtr, nameBytes.byteLength);
        if (code === 2) {
            this.ex.docx_free(outPtr, 8);
            return null;
        }
        if (code !== 0) {
            this.ex.docx_free(outPtr, 8);
            throw new DocxWasmError(this.errMsg(), code);
        }
        const view = new DataView(this.ex.memory.buffer);
        const ptr = view.getUint32(outPtr, true);
        const len = view.getUint32(outPtr + 4, true);
        // slice 拷出（独立 ArrayBuffer），随后释放 wasm 侧缓冲
        const bytes = new Uint8Array(this.ex.memory.buffer, ptr, len).slice();
        this.ex.docx_result_free(ptr, len);
        this.ex.docx_free(outPtr, 8);
        return bytes;
    }
    errMsg() {
        const bufPtr = this.ex.docx_alloc(ERR_CAP);
        const n = this.ex.docx_err_msg(bufPtr, ERR_CAP);
        const msg = this.decoder.decode(new Uint8Array(this.ex.memory.buffer, bufPtr, n));
        this.ex.docx_free(bufPtr, ERR_CAP);
        return msg;
    }
}
