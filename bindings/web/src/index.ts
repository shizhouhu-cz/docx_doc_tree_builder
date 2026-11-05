//! 主线程 API：封装 Worker 通信，提供 parseDocx 与按需 entry 提取。
//!
//! 典型用法：
//! ```ts
//! const parser = new DocxTreeParser("dist/docx_worker.js", "dist/docx_tree_wasm.wasm");
//! const tree = await parser.parse(await file.arrayBuffer());
//! const root = await renderToDOM(tree, {
//!   fetchImage: (t) => parser.entry(t), // 图片字节 → blob URL 一步到位
//! });
//! ```

import type { DocTree } from "./types.js";
import { DocxWasmError } from "./wasm.js";

export * from "./types.js";
export * from "./render.js";

interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
}

type WorkerReply =
  | { type: "ready" }
  | { type: "error"; id: number; message: string; code: number }
  | { type: "parsed"; id: number; tree: DocTree }
  | { type: "entry"; id: number; bytes: ArrayBuffer | null };

export class DocxTreeParser {
  private worker: Worker;
  private pending = new Map<number, Pending>();
  private nextId = 1;
  private ready: Promise<void>;
  private disposed = false;

  constructor(
    /** 编译后的 docx_worker.js URL（module worker） */
    workerUrl: string,
    /** wasm 二进制 URL（worker 内 fetch） */
    wasmUrl: string,
  ) {
    this.worker = new Worker(workerUrl, { type: "module" });
    this.worker.onmessage = (ev: MessageEvent<WorkerReply>) =>
      this.onReply(ev.data);
    this.worker.onerror = (e) => {
      for (const p of this.pending.values()) {
        p.reject(new Error(`worker error: ${e.message ?? "unknown"}`));
      }
      this.pending.clear();
    };
    this.ready = new Promise<void>((resolve, reject) => {
      this.pending.set(-1, {
        resolve: resolve as unknown as (v: unknown) => void,
        reject,
      });
    });
    this.worker.postMessage({ type: "init", wasmUrl });
  }

  private onReply(msg: WorkerReply) {
    if (msg.type === "ready") {
      this.pending.get(-1)?.resolve(undefined);
      this.pending.delete(-1);
      return;
    }
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    switch (msg.type) {
      case "parsed":
        p.resolve(msg.tree);
        break;
      case "entry":
        p.resolve(msg.bytes ? new Uint8Array(msg.bytes) : null);
        break;
      case "error":
        p.reject(new DocxWasmError(msg.message, msg.code));
        break;
    }
  }

  private request<T>(id: number): Promise<T> {
    if (this.disposed) return Promise.reject(new Error("parser disposed"));
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as never, reject });
    });
  }

  /** 解析 docx（ArrayBuffer 所有权转移给 worker，零拷贝） */
  async parse(buffer: ArrayBuffer): Promise<DocTree> {
    await this.ready;
    const id = this.nextId++;
    this.worker.postMessage({ type: "parse", id, buffer }, [buffer]);
    return this.request<DocTree>(id);
  }

  /** 按 entry 名提取字节（如 "word/media/image1.png"）；不存在返回 null */
  async entry(name: string): Promise<Uint8Array | null> {
    await this.ready;
    const id = this.nextId++;
    this.worker.postMessage({ type: "entry", id, name });
    return this.request<Uint8Array | null>(id);
  }

  dispose() {
    this.disposed = true;
    for (const p of this.pending.values()) p.reject(new Error("parser disposed"));
    this.pending.clear();
    this.worker.terminate();
  }
}

/** 一次性便捷入口（design.md §2.2 的 parseDocx 形态）；
 *  用后即弃 worker，故图片按需提取不可用——需要图片请持有 DocxTreeParser */
export async function parseDocx(
  buffer: ArrayBuffer,
  workerUrl: string,
  wasmUrl: string,
): Promise<DocTree> {
  const parser = new DocxTreeParser(workerUrl, wasmUrl);
  try {
    return await parser.parse(buffer);
  } finally {
    parser.dispose();
  }
}
