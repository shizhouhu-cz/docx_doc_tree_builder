//! 关系文件（.rels）解析：rId → target 映射。

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::Result;
use crate::xml::{attr, local_name};

/// 一条关系
pub struct Rel {
    pub id: String,
    pub target: String,
    /// TargetMode="External"（超链接外链等）
    pub external: bool,
}

/// 解析 .rels XML，返回关系列表
pub fn parse(xml: &str) -> Result<Vec<Rel>> {
    let mut r = Reader::from_str(xml);
    let mut rels = Vec::new();
    loop {
        match r
            .read_event()
            .map_err(|_| crate::error::Error::msg("rels: parse error"))?
        {
            Event::Start(e) | Event::Empty(e) => {
                if local_name(e.name().as_ref()) == b"Relationship" {
                    let id = attr(&e, "Id", None);
                    let target = attr(&e, "Target", None);
                    let mode = attr(&e, "TargetMode", None);
                    if let (Some(id), Some(target)) = (id, target) {
                        rels.push(Rel {
                            id,
                            target,
                            external: mode.as_deref() == Some("External"),
                        });
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(rels)
}

/// 解析为 id → Rel 的映射表
pub fn map(xml: &str) -> Result<HashMap<String, Rel>> {
    let mut m = HashMap::new();
    for rel in parse(xml)? {
        m.insert(rel.id.clone(), rel);
    }
    Ok(m)
}

/// 将关系 target（相对路径）解析为 zip 内 entry 名。
/// - `base` 是关系文件所属 part 的目录（document.xml.rels 的 base 是 "word/"）
/// - 绝对路径（`/word/x.xml`）从 zip 根开始
/// - `..` 段逐级回退
pub fn resolve(base: &str, target: &str) -> String {
    let abs = target.starts_with('/');
    // 以 base 目录段为起点，逐段消化 target（".." 相对当前累积路径回退）
    let mut full: Vec<&str> = if abs {
        Vec::new()
    } else {
        base.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                full.pop();
            }
            s => full.push(s),
        }
    }
    full.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELS: &str = r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type=".../styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type=".../hyperlink" Target="https://example.com/a" TargetMode="External"/>
  <Relationship Id="rId3" Type=".../image" Target="media/image3.png"/>
  <Relationship Id="rId4" Type=".../x" Target="../customXml/item1.xml"/>
</Relationships>"#;

    #[test]
    fn parse_rels() {
        let m = map(RELS).unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m["rId1"].target, "styles.xml");
        assert!(!m["rId1"].external);
        assert!(m["rId2"].external);
        assert_eq!(m["rId2"].target, "https://example.com/a");
    }

    #[test]
    fn resolve_paths() {
        assert_eq!(resolve("word/", "styles.xml"), "word/styles.xml");
        assert_eq!(
            resolve("word/", "media/image3.png"),
            "word/media/image3.png"
        );
        assert_eq!(
            resolve("word/", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(resolve("word/", "/word/media/x.png"), "word/media/x.png");
        assert_eq!(resolve("word/", "./a/../b.xml"), "word/b.xml");
    }
}
