//! docx-tree-core：极小体积 docx → DocumentTree 解析核心库。
//! 纯 Rust，无 wasm 依赖（wasm 绑定在 docx-tree-wasm crate）。

pub mod builder;
pub mod error;
pub mod json;
pub mod model;
pub mod numbering;
pub mod rels;
pub mod styles;
pub mod xml;
pub mod zip;

pub use error::{Error, Result};

use std::collections::HashMap;

pub const ENTRY_DOCUMENT: &str = "word/document.xml";
pub const ENTRY_RELS: &str = "word/_rels/document.xml.rels";
pub const ENTRY_STYLES: &str = "word/styles.xml";
pub const ENTRY_NUMBERING: &str = "word/numbering.xml";

/// 解析 docx 字节流 → 语义树
pub fn parse(input: &[u8]) -> Result<model::DocTree> {
    let zip = zip::ZipArchive::new(input)?;
    let doc = zip.read(ENTRY_DOCUMENT).map_err(|e| match e {
        Error::NotFound => {
            let mut m = String::from("missing ");
            m.push_str(ENTRY_DOCUMENT);
            Error::msg(m)
        }
        other => other,
    })?;
    let doc = as_str(doc)?;

    // 辅助 part：缺失不致命（python-docx 生成的最简 docx 可能没有 numbering）
    let rels = match zip.read(ENTRY_RELS) {
        Ok(d) => rels::map(&as_str(d)?)?,
        Err(_) => HashMap::new(),
    };
    let styles = match zip.read(ENTRY_STYLES) {
        Ok(d) => styles::parse(&as_str(d)?)?,
        Err(_) => styles::Styles::default(),
    };
    let numbering = match zip.read(ENTRY_NUMBERING) {
        Ok(d) => Some(numbering::parse(&as_str(d)?)?),
        Err(_) => None,
    };

    let (body, section) = builder::build(&doc, &rels, &styles, numbering.as_ref())?;
    Ok(model::DocTree {
        body,
        section,
        styles: styles
            .index
            .into_iter()
            .map(|(id, name)| model::StyleIndex { id, name })
            .collect(),
    })
}

/// 序列化语义树 → JSON
pub fn to_json(t: &model::DocTree) -> String {
    json::to_json(t)
}

/// 字节 → UTF-8 字符串（含 BOM 处理）
fn as_str(v: Vec<u8>) -> Result<String> {
    let v = if v.starts_with(&[0xEF, 0xBB, 0xBF]) {
        v[3..].to_vec()
    } else {
        v
    };
    String::from_utf8(v).map_err(|_| Error::msg("xml: invalid utf-8"))
}
