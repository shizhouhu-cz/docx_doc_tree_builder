//! deflate 解压封装（miniz_oxide inflate-only 路径）。
//!
//! 防 zip 炸弹：解压上限取 max(声明大小, 1MB)，同时绝对上限 256MB。
//! 声明大小来自中央目录（攻击者可控），故仍需绝对上限兜底。

use crate::error::{Error, Result};

const FLOOR: usize = 1 << 20; // 1MB
const CEILING: usize = 256 << 20; // 256MB

pub fn inflate(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let limit = expected.clamp(FLOOR, CEILING);
    miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit)
        .map_err(|_| Error::msg("zip: inflate failed"))
}
