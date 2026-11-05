import type { DocTree } from "./types.js";
export declare class DocxWasmError extends Error {
    /** wasm FFI 错误码：1 一般错误 / 2 NotFound / 3 panic / 4 非 UTF-8 */
    code: number;
    constructor(message: string, 
    /** wasm FFI 错误码：1 一般错误 / 2 NotFound / 3 panic / 4 非 UTF-8 */
    code: number);
}
export declare class DocxWasm {
    private ex;
    private decoder;
    private constructor();
    /** 从 URL 或已取得的字节实例化（wasm 无 imports，能力边界即线性内存） */
    static instantiate(source: string | ArrayBuffer): Promise<DocxWasm>;
    /** 解析 docx 字节 → DocTree；失败抛 DocxWasmError */
    parse(input: Uint8Array): DocTree;
    /** 按 entry 名提取解压字节（依赖上次 parse 保留的状态）；不存在返回 null */
    entry(name: string): Uint8Array | null;
    private errMsg;
}
