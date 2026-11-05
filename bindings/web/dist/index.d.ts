import type { DocTree } from "./types.js";
export * from "./types.js";
export * from "./render.js";
export declare class DocxTreeParser {
    private worker;
    private pending;
    private nextId;
    private ready;
    private disposed;
    constructor(
    /** 编译后的 docx_worker.js URL（module worker） */
    workerUrl: string, 
    /** wasm 二进制 URL（worker 内 fetch） */
    wasmUrl: string);
    private onReply;
    private request;
    /** 解析 docx（ArrayBuffer 所有权转移给 worker，零拷贝） */
    parse(buffer: ArrayBuffer): Promise<DocTree>;
    /** 按 entry 名提取字节（如 "word/media/image1.png"）；不存在返回 null */
    entry(name: string): Promise<Uint8Array | null>;
    dispose(): void;
}
/** 一次性便捷入口（design.md §2.2 的 parseDocx 形态）；
 *  用后即弃 worker，故图片按需提取不可用——需要图片请持有 DocxTreeParser */
export declare function parseDocx(buffer: ArrayBuffer, workerUrl: string, wasmUrl: string): Promise<DocTree>;
