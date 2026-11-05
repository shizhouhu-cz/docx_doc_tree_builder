//! XML 公共辅助：局部名拆解、命名空间映射、属性查找、实体反转义。
//!
//! 设计 §3.2：不实现完整 XML namespace 规范，务实策略——
//! 元素按局部名匹配；`w:`/`r:` 前缀属性通过"前缀→URI"映射按 URI 判定，
//! 防止生成器使用非标准前缀。

use std::borrow::Cow;

use crate::error::{Error, Result};

/// 主命名空间
pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// 关系命名空间（r:id / r:embed）
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// 拆出 QName 的局部名（`w:p` -> `p`，无前缀则原样）
pub fn local_name(q: &[u8]) -> &[u8] {
    match q.iter().position(|&b| b == b':') {
        Some(i) => &q[i + 1..],
        None => q,
    }
}

/// 拆出 QName 的前缀（`w:p` -> `w`；无前缀返回 None）
pub fn prefix(q: &[u8]) -> Option<&[u8]> {
    q.iter().position(|&b| b == b':').map(|i| &q[..i])
}

/// 前缀→URI 映射（"" 表示默认命名空间）。
/// 从已见过的元素的 xmlns 声明累积（docx 的声明几乎全在根元素上）。
#[derive(Default)]
pub struct NsMap {
    map: Vec<(Cow<'static, str>, Cow<'static, str>)>,
}

impl NsMap {
    /// 记录元素上出现的 xmlns 声明
    pub fn note_decl(&mut self, e: &quick_xml::events::BytesStart) {
        for a in e.attributes().flatten() {
            let key = a.key.as_ref();
            let (pfx, uri) = match key.strip_prefix(b"xmlns".as_slice()) {
                Some(rest) => {
                    if rest.is_empty() {
                        // xmlns="..." -> 默认命名空间
                        (b"".as_slice(), a.value.as_ref())
                    } else if let Some(p) = rest.strip_prefix(b":".as_slice()) {
                        (p, a.value.as_ref())
                    } else {
                        continue; // 类似 xmlns-xxx 的怪异属性，忽略
                    }
                }
                None => continue,
            };
            let pfx = String::from_utf8_lossy(pfx).into_owned();
            let uri = String::from_utf8_lossy(uri).into_owned();
            match self.map.iter_mut().find(|(p, _)| p.as_ref() == pfx) {
                Some(slot) => slot.1 = Cow::Owned(uri),
                None => self.map.push((Cow::Owned(pfx), Cow::Owned(uri))),
            }
        }
    }

    /// 前缀是否解析到期望的 URI（映射缺失时退化为字面前缀比较）
    pub fn prefix_matches(&self, pfx: Option<&[u8]>, want_prefix: &str, want_uri: &str) -> bool {
        match pfx {
            None => {
                // 无前缀：默认命名空间是否等于期望 URI
                self.map
                    .iter()
                    .find(|(p, _)| p.is_empty())
                    .map(|(_, u)| u.as_ref() == want_uri)
                    .unwrap_or(false)
            }
            Some(p) => {
                let p = String::from_utf8_lossy(p);
                match self.map.iter().find(|(k, _)| *k == p) {
                    Some((_, uri)) => uri.as_ref() == want_uri,
                    None => p == want_prefix, // 未见声明，退化为约定前缀
                }
            }
        }
    }
}

/// 在元素属性里按局部名查找；若同名属性有多个，优先取前缀能对上期望
/// 命名空间（或退化字面前缀）的那个。返回未反转义的值。
pub fn attr(e: &quick_xml::events::BytesStart, local: &str, ns: Option<&NsMap>) -> Option<String> {
    let want: &[u8] = local.as_bytes();
    let mut fallback: Option<Vec<u8>> = None;
    let mut fallback_ok = false;
    for a in e.attributes().flatten() {
        if local_name(a.key.as_ref()) != want {
            continue;
        }
        let pfx = prefix(a.key.as_ref());
        let matched = match ns {
            // w: 属性按 W URI 判定，r: 属性按 R URI 判定
            Some(ns) => {
                let uri = if local == "id" || local == "embed" || local == "pict" {
                    NS_R
                } else {
                    NS_W
                };
                let wp = if uri == NS_R { "r" } else { "w" };
                ns.prefix_matches(pfx, wp, uri)
            }
            None => false,
        };
        if matched {
            if fallback_ok {
                continue; // 已有更强的匹配
            }
            fallback = Some(a.value.to_vec());
            fallback_ok = true;
        } else if fallback.is_none() {
            fallback = Some(a.value.to_vec());
        }
    }
    fallback.and_then(|v| unescape(&v).ok().map(|c| c.into_owned()))
}

/// 解码单个实体内容（`&` 与 `;` 之间的部分，如 `amp`、`#65`、`#x42`）
pub fn decode_entity(ent: &[u8]) -> Result<char> {
    match ent {
        b"amp" => Ok('&'),
        b"lt" => Ok('<'),
        b"gt" => Ok('>'),
        b"quot" => Ok('"'),
        b"apos" => Ok('\''),
        _ if ent.first() == Some(&b'#') => {
            let body = &ent[1..];
            let cp = if body.first() == Some(&b'x') {
                u32::from_str_radix(
                    std::str::from_utf8(&body[1..]).map_err(|_| Error::msg("xml: bad entity"))?,
                    16,
                )
            } else {
                std::str::from_utf8(body)
                    .map_err(|_| Error::msg("xml: bad entity"))?
                    .parse::<u32>()
            }
            .map_err(|_| Error::msg("xml: bad entity"))?;
            char::from_u32(cp).ok_or_else(|| Error::msg("xml: bad entity"))
        }
        _ => Err(Error::msg("xml: unknown entity")),
    }
}

/// XML 实体反转义（自带实现，避免依赖 quick-xml 的 escape feature 行为）。
/// 无 `&` 时零拷贝借用。
pub fn unescape(d: &[u8]) -> Result<Cow<'_, str>> {
    if !d.contains(&b'&') {
        return std::str::from_utf8(d)
            .map(Cow::Borrowed)
            .map_err(|_| Error::msg("xml: invalid utf-8"));
    }
    let mut s = String::with_capacity(d.len());
    let mut i = 0;
    while i < d.len() {
        if d[i] == b'&' {
            let semi = d[i + 1..]
                .iter()
                .position(|&b| b == b';')
                .map(|p| p + i + 1)
                .ok_or_else(|| Error::msg("xml: unterminated entity"))?;
            s.push(decode_entity(&d[i + 1..semi])?);
            i = semi + 1;
        } else {
            // 找到下一个 '&' 前的整体段，避免逐字节处理
            let next = d[i..]
                .iter()
                .position(|&b| b == b'&')
                .map(|p| p + i)
                .unwrap_or(d.len());
            s.push_str(
                std::str::from_utf8(&d[i..next]).map_err(|_| Error::msg("xml: invalid utf-8"))?,
            );
            i = next;
        }
    }
    Ok(Cow::Owned(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_names() {
        assert_eq!(local_name(b"w:p"), b"p");
        assert_eq!(local_name(b"p"), b"p");
        assert_eq!(prefix(b"w:p"), Some(&b"w"[..]));
        assert_eq!(prefix(b"p"), None);
    }

    #[test]
    fn unescape_entities() {
        assert_eq!(unescape(b"a&amp;b").unwrap(), "a&b");
        assert_eq!(unescape(b"&lt;x&gt;").unwrap(), "<x>");
        assert_eq!(unescape(b"&#65;&#x42;").unwrap(), "AB");
        assert_eq!(unescape(b"plain").unwrap(), "plain");
        assert!(unescape(b"&unknown;").is_err());
        assert!(unescape(b"&unterminated").is_err());
    }

    #[test]
    fn ns_map_matching() {
        use quick_xml::events::BytesStart;
        let mut m = NsMap::default();
        let e = BytesStart::new("w:p").with_attributes([
            (
                "xmlns:w",
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            ),
            (
                "xmlns:r",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
            ),
            (
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/relationships",
            ),
        ]);
        m.note_decl(&e);
        // 非标准前缀 zz 也映射到 W 命名空间
        let e2 = BytesStart::new("zz:p").with_attributes([(
            "xmlns:zz",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        )]);
        m.note_decl(&e2);
        assert!(m.prefix_matches(Some(b"zz"), "w", NS_W));
        assert!(m.prefix_matches(Some(b"r"), "r", NS_R));
        assert!(m.prefix_matches(
            None,
            "w",
            "http://schemas.openxmlformats.org/package/2006/relationships"
        ));
        assert!(!m.prefix_matches(Some(b"r"), "w", NS_W));
        // 未见声明的前缀退化为字面比较
        assert!(m.prefix_matches(Some(b"w"), "w", NS_W));
        assert!(!m.prefix_matches(Some(b"x"), "w", NS_W));
    }

    #[test]
    fn attr_lookup_prefers_ns_match() {
        use quick_xml::events::BytesStart;
        let mut m = NsMap::default();
        let e = BytesStart::new("root").with_attributes([(
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        )]);
        m.note_decl(&e);
        // 正确前缀 + 假前缀同名（id），优先取 URI 匹配的
        let e2 = BytesStart::new("a:blip").with_attributes([("r:id", "good"), ("bad:id", "wrong")]);
        assert_eq!(attr(&e2, "id", Some(&m)).unwrap(), "good");
    }
}
