# docx-doc-tree-builder 技术方案

> 状态：已评审（三项关键决策已确认，见 §10）
> 版本：v0.2
> 目标读者：项目开发者本人

---

## 1. 背景与目标

### 1.1 一句话描述

用 Rust 实现一个**极小体积**的 docx 解析库：输入 `.docx` 文件字节流，输出结构化的文档树（Document Tree），编译为 WASM 后运行在浏览器 Web Worker 中。

### 1.2 核心约束（按优先级排序）

1. **体积**：这是本项目的存在理由。现有开源方案（mammoth、docx4js、docx-preview 等）功能臃肿，本方案拒绝一切非必需依赖。
2. **正确性**：对常见 Word 生成/保存的 docx（Word 桌面版、WPS、python-docx、pandoc 导出）还原出正确的文档树。
3. **健壮性**：恶意/畸形输入不允许 panic 跨 FFI 边界，返回结构化错误。
4. **可测试**：核心逻辑是纯 Rust 库，可在原生环境用 `cargo test` 覆盖，不依赖浏览器。

### 1.3 非目标（明确不做）

- 不做 docx **写入/编辑**（只读解析）。
- 核心库不做渲染（输出是中立的结构化 JSON；DocTree → HTML 渲染作为独立 TS 模块规划，见 §11）。
- 不追求 OOXML 全覆盖（ECMA-376 数百个元素，v1 只覆盖主干，见 §6）。
- 不解析 `.doc`（二进制旧格式）。
- 不提取图片二进制内容（只输出图片引用关系，图片本身由上层按需读取）。

---

## 2. 总体架构

### 2.1 数据流水线

```
JS (Web Worker)
  │  ArrayBuffer (整个 .docx 文件字节)
  ▼
┌─────────────────── WASM 模块 ───────────────────┐
│                                                  │
│  ① ZIP 解包    提取指定 entry 的字节              │
│       │                                         │
│  ② 关系解析    word/_rels/document.xml.rels       │
│       │         → { rId → target } 映射          │
│  ③ 样式/编号    word/styles.xml + numbering.xml   │
│       │         → 有效格式解析表（§5.2）          │
│  ④ XML 流式解析  word/document.xml                │
│       │         (quick-xml 事件流)               │
│  ⑤ 树构建      语义模型 DocumentTree             │
│       │         （引用样式表计算有效格式）        │
│  ⑥ JSON 序列化  手写序列化器 → String            │
│                                                  │
└──────────────────────────────────────────────────┘
  │  JSON 字符串（UTF-8）
  ▼
JS: JSON.parse → 结构化 DocTree → 供上层消费
```

设计要点：

- **跨边界只传两次数据**：进（文件字节）+ 出（JSON 字符串）。文件字节一次性拷入 WASM 线性 内存，后续全部在 WASM 内零拷贝引用，避免 JS/WASM 间多次穿梭。
- **输出选 JSON 而非 JS 对象逐个传回**：一次字符串传输 + JS 侧 `JSON.parse`（JIT 优化过），远快于也小于通过 wasm-bindgen 逐对象构造。
- **核心库不依赖任何 wasm 相关 crate**：`docx-tree-core` 是纯 Rust 库，可编译到原生用于 CLI 调试和测试；`-wasm` 只是薄薄一层绑定。

### 2.2 工程结构

```
docx_doc_tree_builder/
├── Cargo.toml                  # workspace
├── crates/
│   ├── docx-tree-core/         # 纯 Rust 核心库（无 wasm 依赖）
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── zip/            # 极简 zip 读取器
│   │       │   ├── mod.rs      #   EOCD / central directory 解析
│   │       │   └── inflate.rs  #   deflate 解压封装
│   │       ├── xml.rs          # XML 事件流封装与命名空间处理
│   │       ├── rels.rs         # .rels 关系文件解析
│   │       ├── model.rs        # 语义树数据模型（§4）
│   │       ├── builder.rs      # XML 事件流 → 语义树 状态机
│   │       ├── json.rs         # 手写 JSON 序列化器
│   │       └── error.rs
│   ├── docx-tree-wasm/         # WASM 绑定层（薄）
│   │   └── src/lib.rs          #   ~150 行：内存管理 + FFI 导出
│   └── docx-tree-cli/          # 原生 CLI（仅开发/测试用，不进产物）
│       └── src/main.rs         #   解析本地 docx → stdout JSON / 调试
├── bindings/
│   └── web/                    # 手写 TS 胶水 + Worker 封装
│       ├── src/index.ts        #   parseDocx(buffer): Promise<DocTree>
│       ├── src/docx_worker.ts
│       └── src/render.ts       #   DocTree → DOM/HTML 渲染层（§11，纯 TS）
├── fixtures/                   # 测试用 docx 样本 + golden JSON
└── docs/
```

---

## 3. 关键技术选型

### 3.1 ZIP 解包 —— 手写极简读取器（决策：✅ 已确认，自研，~350 行）

docx 本质是 ZIP 包。选项对比：

| 方案 | 依赖 | 体积代价 | 评估 |
|---|---|---|---|
| `zip` crate（default-features=false, deflate） | miniz_oxide + crc32fast + 若干 | 较大，含大量我们用不到的逻辑（加密、zip64 全量、注释、多卷） | ❌ 功能远超所需 |
| `rc-zip` / 其它 | 更多 | 更大 | ❌ |
| **手写 ZIP 目录解析 + `miniz_oxide`** | 仅 miniz_oxide（inflate-only feature） | 最小 | ✅ |

**实现要点**（ZIP 格式只需要一小部分）：

1. 从文件**末尾** 64KB 范围内搜索 EOCD 签名 `PK\x05\x06`（docx 通常很小，一页就够）。
2. 解析 central directory（`PK\x01\x02`），得到每个 entry 的名称、压缩方法、压缩/原始大小、local header 偏移。**只解析目录，不顺序扫描**——docx 只需要其中 4 个 entry（`word/document.xml`、`word/_rels/document.xml.rels`、`word/styles.xml`、`word/numbering.xml`），跳过所有 media 二进制。
3. 对需要的 entry，跳到 local file header（`PK\x03\x04`），校验文件名一致，读取方法 8（deflate）→ `miniz_oxide::inflate::decompress_to_vec`（ZIP 用的是 raw deflate，无 zlib 头）或方法 0（stored）→ 直接切片。
4. **CRC32 校验默认跳过**（省掉 crc32 表 + 计算）：解压时 miniz_oxide 已保证 deflate 流完整性；上层有自己的结构校验。若后续需要可加开关。
5. 支持 ZIP64 的 EOCD 定位与大小字段（`PK\x06\x06`），因为 >4GB 或 >65535 entry 理论上可能出现（docx 极少，但代码只有几十行，防患于未然）。

**inflate-only**：`miniz_oxide` 开 `std` 关闭其它 feature，只引入解压路径，压缩路径不进产物。

### 3.2 XML 解析 —— `quick-xml` 流式拉取（决策：quick-xml）

| 方案 | 模式 | 体积 | 评估 |
|---|---|---|---|
| `roxmltree` | 一次性构建 DOM | 小 | ❌ 多一层中间 DOM，我们只要自己的语义树 |
| `xml-rs` | 拉取事件 | 较大 | ❌ |
| `serde-xml` 系列 | — | 拖入 serde | ❌ |
| **`quick-xml`** | 零拷贝拉取事件流 | 最小 | ✅ |

- 关闭默认 feature（`serde`、`escape-html` 等全不要），零拷贝借用模式。
- 解析状态机把 `Event::{Start, Empty, Text, End, Eof}` 直接喂给树构建器（§5），不落地中间 DOM。
- 原始字节直接借用自输入 buffer，仅文本内容与必要属性做最小拷贝。

**命名空间处理**：不实现完整 XML namespace 规范（体积与复杂度都不值）。采取务实策略——

- 元素按**局部名**匹配（`w:p` → `p`）。docx 中的关键前缀（`w`、`r`、`a`、`wp`、`pic`）在 Word 实际产物中高度稳定。
- 两个例外需要前缀感知：`r:id` / `r:embed`（关系引用，`r` 前缀）与 `w:val` 等 `w` 前缀属性。构建器维护一个 **前缀→URI 映射**（读取 `document.xml` 根元素的 xmlns 声明，几十行代码），按 URI 而非硬编码前缀判断，防止某些生成器用非标准前缀。

### 3.3 JSON 输出 —— 手写序列化器（决策：不引入 serde/serde_json）

serde + serde_derive + serde_json 三件套在 wasm 中约带来 30~50KB（gz）开销，且 derive 宏编译慢。我们的输出模型是**封闭的小集合**（§4，约 10 个节点类型），手写序列化器约 200 行、零依赖、输出字段顺序稳定（利于 golden 测试 diff）。

注意点：字符串转义（`"`、`\`、控制字符、非 ASCII 直接输出 UTF-8）、数字不用浮点（行号/索引全是整数）、`None` 字段直接省略 key（省字节）。

### 3.4 WASM 绑定 —— 裸 FFI（决策：✅ 已确认，不用 wasm-bindgen）

| 方案 | 体积代价 | 评估 |
|---|---|---|
| `wasm-bindgen` + `wasm-bindgen-cli` 胶水 | wasm-bindgen 运行时 + 生成的 JS 胶水（数十 KB 级） | ❌ 我们只有一个导出函数，不划算 |
| **裸 `extern "C"` + 手写 TS 胶水** | JS 侧 ~100 行 | ✅ |

导出接口（wasm32-unknown-unknown）：

```rust
// 分配/释放线性内存，供 JS 拷入文件字节与拷出结果
#[no_mangle] pub extern "C" fn docx_alloc(len: u32) -> *mut u8;
#[no_mangle] pub extern "C" fn docx_free(ptr: *mut u8, len: u32);

// 核心 API：输入 buffer，输出 (ptr, len) 指向 JSON UTF-8 字符串
// 返回 0 表示成功；非 0 为错误码，错误详情通过 docx_err_msg 获取
#[no_mangle] pub extern "C" fn docx_parse(input: *const u8, len: u32,
                                            out_ptr: *mut u32, out_len: *mut u32) -> u32;
#[no_mangle] pub extern "C" fn docx_result_free(ptr: *mut u8, len: u32);

// 按 entry 名提取字节（如 word/media/image3.png 的图片二进制）
// 依赖 docx_parse 保留的文件字节 + 中央目录；返回 0 成功
#[no_mangle] pub extern "C" fn docx_entry(name: *const u8, name_len: u32,
                                          out_ptr: *mut u32, out_len: *mut u32) -> u32;
```

**状态生命周期**：`docx_parse` 成功后，wasm 内部保留文件字节与中央目录（不释放），`docx_entry` 复用它们按需解压单个 entry；再次 `docx_parse` 覆盖旧状态。典型用法：`docx_entry("word/media/image3.png")` → JS 拷出 `Uint8Array` → `URL.createObjectURL(new Blob([...]))` 作为 `<img src>`。

TS 胶水负责：加载 wasm、`WebAssembly.Memory` 读写、UTF-8 编解码（`TextEncoder/Decoder`）、Worker 封装（`postMessage(ArrayBuffer, [transfer])` 零拷贝转移）。

**防 panic 泄漏**：`docx_parse` 内部整体 `catch_unwind` + `panic = "abort"` 关闭（abort 会直接 kill worker，改为 unwind + 捕获转错误码），任何 panic 都降级为 `Err(code, msg)`。

### 3.5 编译与体积优化配置

```toml
# Cargo.toml (wasm crate)
[profile.release]
opt-level = "z"        # 极限体积优化
lto = "fat"
codegen-units = 1
panic = "unwind"       # 配合 catch_unwind，见 §3.4
strip = "symbols"
```

构建链：`cargo build --target wasm32-unknown-unknown --release` → `wasm-opt -Oz --strip-debug`（binaryen）→ 产物体积 CI 门禁。

不使用：`wasm-pack`（其模板强制 wasm-bindgen 生态）、`wee_alloc`（已停止维护，默认分配器近期版本已足够好；若实测 malloc 元数据占比高，再评估 `talc`）。

### 3.6 体积预算

| 阶段 | 预算（gzip 后） |
|---|---|
| v1 交付 | **≤ 60 KB**（wasm + TS 胶水合计） |
| 长期目标 | ≤ 40 KB |

CI 中用脚本构建产物 + `gzip -9` 计量，超预算即失败。逐项预估：miniz_oxide inflate-only ~25-30KB gz、quick-xml ~8-12KB gz、自研 zip/tree/json ~5-8KB gz、样式/编号解析逻辑 ~2-3KB gz、胶水 ~1KB。

---

## 4. 语义数据模型（输出 Schema）

### 4.1 设计原则

- **语义树而非 XML 树**：输出不是 `document.xml` 的镜像 DOM，而是消解了 OOXML 怪癖后的干净树（如 `<w:t>` 合并为文本节点、`<w:tab/>` 变为 Tab 节点、hyperlink 的 rId 直接解析为 target URL）。
- **属性白名单**：只保留渲染/结构有意义的属性，其余丢弃。需要逃生舱时，v2 可加 `raw: string` 字段保存原始 XML（默认关闭以省体积）。

### 4.2 节点定义

```
DocTree {
  version: "1",
  body: Block[],            // 文档主体
  section: SectionProps?    // 页面设置（页宽/页高/边距，来自 sectPr）
}

Block =
  | Paragraph {
      type: "paragraph",
      style?: string,          // pStyle 引用的样式 id（保留原始引用，便于上层定位）
      numbering?: { id, level, marker },
        // id/level 来自 numPr；marker 是根据 numbering.xml 的 numFmt+lvlText
        // 现场生成的编号文本，如 "1."、"a)"、"一、"
      alignment?: "left"|"center"|"right"|"justify",   // 有效值（样式链解析后，§5.2）
      indent?: { left?, right?, firstLine?, hanging? },   // 有效值，单位: twip
      children: Inline[]
    }
  | Table {
      type: "table",
      rows: TableRow[]         // TableRow { cells: TableCell[] }
      // TableCell 含 gridColumnSpan/rowSpan 合并信息、内部 Block[]
    }

Inline =
  | { type: "text", text: string,
      bold?, italic?, underline?, strike?, code?,          // 有效值（样式链解析后）
      color?: string,        // 十六进制 RGB，如 "FF0000"
      sizePt?: number,       // 字号（磅，整数）
      font?: string }        // 主字体名（w:rFonts 的 ascii 属性）
  | { type: "tab" }
  | { type: "break", breakType: "line"|"page"|"column" }
  | { type: "hyperlink", target: string, children: Inline[] }   // rId 已解析为 URL
  | { type: "image", target: string, name?, widthPx?, heightPx? } // 指向 media 引用
  | { type: "footnoteRef", id: number } | { type: "field", instr: string }  // v1 保留骨架
```

> 有效格式的计算规则见 §5.2。原则：**文档内直接设置的属性优先，样式链补齐其余**；文本节点输出的开关属性（bold 等）只在该属性有效值为 true 时输出（省字节），color/sizePt/font 在有值时输出。

### 4.3 JSON 输出示例

```json
{
  "version": "1",
  "body": [
    {
      "type": "paragraph",
      "style": "Heading1",
      "children": [
        { "type": "text", "text": "第一章 ", "bold": true },
        { "type": "text", "text": "概述" }
      ]
    },
    {
      "type": "paragraph",
      "numbering": { "id": 3, "level": 0, "marker": "1." },
      "children": [{ "type": "text", "text": "第一个列表项" }]
    },
    {
      "type": "table",
      "rows": [{ "cells": [{ "blocks": [ /* ... */ ] }] }]
    }
  ]
}
```

TS 侧由胶水包内提供的 `types.ts` 给出完整的类型声明，作为前后端契约。

---

## 5. 树构建状态机（核心解析逻辑）

### 5.1 状态机

`builder.rs` 是本项目唯一的"业务逻辑"，用显式状态机处理 quick-xml 事件流：

```
状态栈: Vec<Frame>
Frame = Document | Paragraph(p) | Run(r) | Hyperlink | TableCell | ...
```

处理规则示例：

| 事件 | 动作 |
|---|---|
| `Start(w:body)` | push Document frame |
| `Start(w:p)` | push Paragraph frame |
| `Start(w:r)` | push Run frame（继承当前 hyperlink 上下文） |
| `Text`（在 Run 内） | 追加到当前 Run 的 text buffer；**相邻同格式文本节点合并**（Word 常把一句话拆成多个 `w:t`，包含 `xml:space="preserve"` 处理） |
| `Empty(w:tab)` / `Empty(w:br)` | 向当前 Run 插入 Tab / Break inline 节点 |
| `Start(w:hyperlink)` | 读 `r:id`，查 rels 表得到 target；无 rels 项则降级为普通文本 |
| `Start(w:tbl)` / `w:tr` / `w:tc` | 建表结构；`w:tc` 内重新进入 Block 收集模式；处理 `gridSpan`/`vMerge` |
| `Start(w:drawing)` | 深入提取 `a:blip/@r:embed` → rels → 图片 target；`wp:extent` → 尺寸（EMU→px） |
| `End(...)` | pop frame，paragraph 结束时做**空段落/空 Run 裁剪** |
| 未知元素 | **跳过子树**（进入 skip 模式直到配对 End），保证未知结构不致崩溃 |

异常输入策略：任何一处结构不符合预期（如 `w:r` 出现在表格外）→ 记录 warning（不中断），按最大兼容策略继续解析。只有 ZIP/XML 层面无法继续（损坏、解压失败）才返回 Err。

### 5.2 有效格式解析（styles.xml + numbering.xml）

在解析 `document.xml` **之前**，先把两个辅助 part 预解析成查找表（均为一次性 O(n) 流式扫描）：

**① 样式表（styles.xml → effective style chain）**

- 提取每个 `w:style` 的：`w:styleId`、`w:type`（paragraph/character）、`w:basedOn` 链、pPr/rPr 白名单属性、`w:docDefaults` 的默认 rPr/pPr。
- 解析段落有效格式时按 OOXML 规定的优先级自高到低合并（低覆盖为高所覆盖，即**从低优先级向高优先级逐层覆盖**）：
  ```
  docDefaults → pStyle 的 basedOn 继承链（从链根到链尾）→ 文档内直接 pPr
  ```
  Run 的有效格式：`docDefaults rPr → 段落样式的 rPr → rStyle 链 → 文档内直接 rPr`。
- `basedOn` 环检测：链长上限（如 32）防死循环，超限按链断开处截断。
- 样式缺失（pStyle 引用了不存在的 styleId）：静默降级为仅直接格式。
- 表格样式（`w:tblStyle`）v1 只解析 `w:tblLook` 相关的最小子集，其余视为不继承（记录到 known-limitations）。

**② 编号表（numbering.xml → marker 生成）**

- 建立 `numId → abstractNumId` 映射，`abstractNum` 的每个 `w:lvl` 存 `numFmt`（decimal/lowerLetter/upperLetter/lowerRoman/upperRoman/chineseCounting/bullet 等 v1 常用子集）、`lvlText`（含 `%1..%9` 占位符）、`start`。
- 遇到带 `numPr` 的段落时维护每个 `(numId, level)` 的计数器（注意 `lvlOverride`/`startOverride`），用当前计数器值替换 `lvlText` 中的占位符生成 `marker`（如 `"第%1章"` → `"第3章"`）；罗马数字/中文数字实现为纯函数转换表（几十行）。
- `numFmt = bullet` 时 marker 为符号字符本身；v1 未覆盖的 numFmt 降级为十进制计数。

这两个表在解析 `document.xml` 期间只做查表，不做任何 XML 回读。

---

## 6. 解析范围（v1 明确支持清单）

**支持**：`w:body`、`w:p`（pStyle/numPr/jc/ind）、`w:r` + rPr 白名单（b/i/u/strike/color/sz/rFonts/rStyle）、`w:t`/`w:tab`/`w:br`/`w:cr`、`w:hyperlink`、`w:tbl`/`w:tr`/`w:tc`（含 gridSpan/vMerge 基本合并）、`w:drawing`（inline 图片引用）、`w:sectPr`（pgSz/pgMar）、`w:bookmarkStart/End`（丢弃）、footnote/endnote **引用点**骨架、`word/styles.xml`（basedOn 继承链 + docDefaults，§5.2①）、`word/numbering.xml`（marker 生成，§5.2②）、**图片字节按需提取**（`docx_entry` 导出，§3.4，供渲染层生成 `<img>` blob URL——应用户要求从 v2 提前至 v1）。

**显式降级**：`w:sdt`（内容控件）→ 直接下钻取内部内容；`mc:AlternateContent` → 取 `mc:Choice` 分支；`w:fldSimple`/`w:instrText` → 输出 field 节点存指令文本（TOC 等由上层决定渲染）；headers/footers/footnotes 的**正文内容**不进 v1 树（仅保留引用点）。

**v2 路线图**（按需启动，不在本期）：表格样式完整继承、headers/footers/footnotes 全文、`w:sdt` 语义化、原始 XML 逃生舱字段。

---

## 7. 测试策略

1. **单元测试**（core crate，原生 `cargo test`）：
   - zip 模块：构造边界样本（空 entry、stored、deflate、zip64、伪 EOCD 前有垃圾数据）。
   - builder：XML 片段直接喂事件流，断言树形状（覆盖 §5 每条规则）。
   - json：转义、空值省略。
2. **Golden 测试**：`fixtures/` 放入代表性 docx（Word 桌面版 / WPS / python-docx / pandoc 各生成一份，含表格、列表、超链接、图片、脚注），CLI 跑出 JSON 与 golden 文件 diff。**golden diff 是回归主防线**。
3. **差分测试**：抽样对比 mammoth.js 对同一文件的输出（文本级），验证无丢字/错序。
4. **模糊测试**（上线前）：`cargo-fuzz` 对 zip 与 xml 入口，断言"不 panic"不变量。
5. **WASM 集成测试**：`vitest` + headless 浏览器，Worker 中跑真实文件断言 JSON 可 parse。

---

## 8. 里程碑

| 阶段 | 交付物 |
|---|---|
| M1 核心骨架 | workspace + zip 模块（含 zip64）+ quick-xml 接入 + CLI 能从 docx dump 出 XML 事件流 |
| M2 树构建 | model + builder + json 序列化（块级结构先行），CLI 输出完整 JSON，golden 测试就位 |
| M3 有效格式 | styles.xml 继承链解析 + numbering marker 生成，接入 builder（§5.2），golden 扩充样式/列表用例 |
| M4 WASM 化 | 裸 FFI 绑定 + TS 胶水 + Worker 封装 + 浏览器集成测试 + 体积门禁 CI |
| M5 加固 | fuzz、畸形样本集、体积与性能调优（wasm-opt 报告）、发布（npm 包 or 纯 wasm 资产） |

---

## 9. 风险与对策

| 风险 | 对策 |
|---|---|
| 手写 ZIP 读取器踩格式坑 | 只实现必要的最小子集；§7 边界单测 + fuzz；ZIP 是文档极其充分的格式 |
| 非标准前缀的 docx（生成器自由改前缀） | 按 URI 判定命名空间而非硬编码前缀（§3.2） |
| quick-xml 对畸形 XML 的行为差异 | 所有解析路径禁止 `unwrap`；builder 按容错策略处理（§5）；fuzz 兜底 |
| 体积超标 | 体积预算为 CI 硬门禁；依赖只允许 miniz_oxide + quick-xml 两个，新增依赖需评审 |
| 超大 docx（几十 MB media） | 只解压需要的 entry，media 永不解压；JSON 输出用 `String` 一次分配，避免中间碎片 |

---

## 10. 决策记录与遗留点

### 已确认（2026-11-05 评审）

1. **ZIP 解包**：✅ 自研极简读取器（§3.1）。
2. **WASM 绑定**：✅ 裸 `extern "C"` FFI + 手写 TS 胶水（§3.4）。
3. **有效格式**：✅ v1 即解析 styles.xml 继承链与 numbering marker 生成（§5.2），输出"渲染后"的有效格式而非仅样式引用。

### 遗留点（可在开发中按需调整）

4. **rPr 属性白名单**：当前为 b/i/u/strike/color/sz/rFonts/rStyle（§4.2）。若上层有明确渲染目标需要更多属性（如高亮、上下标、字符间距），按需求增补——增补只影响 builder 的属性收集与 JSON 序列化，架构不变。

---

## 11. 渲染层规划（DocTree → HTML，独立 TS 模块）

> 定位：**不属于 wasm 的一部分**。核心库保持"中立数据结构输出"，渲染作为纯 TS 模块（`bindings/web/src/render.ts`）随上层使用，不增加一行 wasm 体积。

### 11.1 为什么放 TS 而不是 wasm

- 树→HTML 是薄映射（查表驱动），放 TS 零 wasm 体积成本；
- DOM API 是浏览器原生能力，在 Rust 里拼 HTML 字符串需自行处理转义，且丢掉 DOM 能力；
- 渲染策略（接框架、虚拟列表、主题化）应随上层自由变化，不应固化在 wasm 里。

### 11.2 双输出形态

```ts
renderToDOM(tree: DocTree): HTMLElement;   // createElement 逐节点构建
renderToHTML(tree: DocTree): string;       // 拼 HTML 字符串，一次 innerHTML
```

两者共享同一张映射表。图片 src 通过 `docx_entry` 取字节后 `URL.createObjectURL(new Blob([...]))` 生成。

### 11.3 节点映射表（v1）

| DocTree 节点 | HTML | 说明 |
|---|---|---|
| Paragraph(style=Heading1-6) | `<h1>`–`<h6>` | 标题样式映射语义标签，利于无障碍 |
| Paragraph（其余） | `<p>` | alignment/indent → class |
| text | `<span>` | b/i/u/strike → class；color/sizePt/font → 内联 style |
| hyperlink | `<a href>` | target 已在解析期解析为 URL |
| image | `<img>` | src = blob URL（docx_entry 按需取字节） |
| table | `<table><tr><td>` | gridSpan → `colspan`，vMerge → `rowspan` |
| break(page) | 分页占位 `<div>` | break(line) → `<br>` |
| tab | 定宽 `<span>` | 按 tab stop 用 CSS 模拟 |
| numbering | 悬挂缩进 + marker 前缀 | v1 不重建 `<ol>/<ul>`（真实列表重建放后续） |

### 11.4 样式策略

- 生成一小段内置 stylesheet（如 `.docx-b{font-weight:700}`），文档内覆盖项用内联 style，避免每个 span 输出冗长内联样式导致 DOM 膨胀；
- 主题/字号缩放由上层 CSS 变量控制，渲染层不写死视觉样式。
