//! Worker 入口：持有唯一 wasm 实例（docx_parse 后保留的字节状态
//! 与 docx_entry 共享，因此 parse 与图片提取必须在同一线程）。
//!
//! 协议（主线程 <-> worker，全部带递增 id 配对）：
//! - { type: "init", wasmUrl }            → { type: "ready" }
//! - { type: "parse", id, buffer }        → { type: "parsed", id, tree }
//!                                          | { type: "error", id, message, code }
//! - { type: "entry", id, name }          → { type: "entry", id, bytes: ArrayBuffer | null }
//!                                          （bytes 为 transferable 零拷贝转移）

import { DocxWasm } from "./wasm.js";
import type { DocTree } from "./types.js";

let wasm: DocxWasm | null = null;

type Req =
  | { type: "init"; wasmUrl: string }
  | { type: "parse"; id: number; buffer: ArrayBuffer }
  | { type: "entry"; id: number; name: string };

self.onmessage = async (ev: MessageEvent<Req>) => {
  const msg = ev.data;
  switch (msg.type) {
    case "init": {
      try {
        wasm = await DocxWasm.instantiate(msg.wasmUrl);
        self.postMessage({ type: "ready" });
      } catch (e) {
        self.postMessage({
          type: "error",
          id: -1,
          message: `wasm init failed: ${(e as Error).message}`,
          code: 0,
        });
      }
      break;
    }
    case "parse": {
      if (!wasm) {
        self.postMessage({
          type: "error",
          id: msg.id,
          message: "worker not initialized",
          code: 1,
        });
        return;
      }
      try {
        const tree: DocTree = wasm.parse(new Uint8Array(msg.buffer));
        self.postMessage({ type: "parsed", id: msg.id, tree });
      } catch (e) {
        self.postMessage({
          type: "error",
          id: msg.id,
          message: (e as Error).message,
          code: (e as { code?: number }).code ?? 1,
        });
      }
      break;
    }
    case "entry": {
      if (!wasm) {
        self.postMessage({ type: "entry", id: msg.id, bytes: null });
        return;
      }
      try {
        const bytes = wasm.entry(msg.name);
        if (bytes) {
          const ab = bytes.buffer as ArrayBuffer;
          self.postMessage({ type: "entry", id: msg.id, bytes: ab }, [ab]);
        } else {
          self.postMessage({ type: "entry", id: msg.id, bytes: null });
        }
      } catch {
        self.postMessage({ type: "entry", id: msg.id, bytes: null });
      }
      break;
    }
  }
};
