//! 极简 ZIP 读取器（design.md §3.1）。
//!
//! 只实现 docx 解析所需的最小子集：
//! - 从文件末尾搜索 EOCD（含 ZIP64 定位）
//! - 解析中央目录拿到 entry 元数据（只建目录，不顺序扫描）
//! - 按需读取 entry：校验 local header 后按 method 0/8 解压
//! - 全部多字节字段用 `from_le_bytes` 显式小端读取
//!
//! 刻意不做：CRC32 校验（inflate 已保证流完整性）、加密、多卷、注释、写 zip。

mod inflate;

use crate::error::{Error, Result};

/// EOCD 签名 `PK\x05\x06`
const SIG_EOCD: &[u8; 4] = b"PK\x05\x06";
/// 中央目录 entry 签名 `PK\x01\x02`
const SIG_CDIR: &[u8; 4] = b"PK\x01\x02";
/// local file header 签名 `PK\x03\x04`
const SIG_LFH: &[u8; 4] = b"PK\x03\x04";
/// ZIP64 EOCD record 签名 `PK\x06\x06`
const SIG_Z64_REC: &[u8; 4] = b"PK\x06\x06";
/// ZIP64 EOCD locator 签名 `PK\x06\x07`
const SIG_Z64_LOC: &[u8; 4] = b"PK\x06\x07";

/// EOCD 最大回扫窗口：自身 22 字节 + 最大注释 65535 字节
const MAX_EOCD_SCAN: usize = 22 + 65535;

/// 中央目录 entry 固定头长度
const CDIR_HDR: usize = 46;
/// local file header 固定头长度
const LFH_HDR: usize = 30;

/// 单个 entry 的元数据（全部来自中央目录）
pub struct Entry {
    /// entry 名（不透明字符串，docx 内部是 "word/xxx" 形式）
    pub name: String,
    /// 压缩方法：0=stored，8=deflate
    method: u16,
    /// 压缩后大小
    comp: u64,
    /// 原始大小
    uncomp: u64,
    /// local file header 在文件中的偏移
    lho: u64,
}

pub struct ZipArchive<'a> {
    data: &'a [u8],
    pub entries: Vec<Entry>,
}

// ---- 小端读取辅助（全部带边界检查） ----

fn u16le(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::msg("zip: unexpected end of data"))
}

fn u32le(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::msg("zip: unexpected end of data"))
}

fn u64le(d: &[u8], o: usize) -> Result<u64> {
    d.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::msg("zip: unexpected end of data"))
}

fn eq_sig(d: &[u8], o: usize, sig: &[u8; 4]) -> bool {
    d.get(o..o + 4).map(|s| s == sig).unwrap_or(false)
}

/// 从末尾向前搜索 4 字节签名，限制回扫窗口
fn find_last(d: &[u8], sig: &[u8; 4], max_scan: usize) -> Option<usize> {
    if d.len() < 4 {
        return None;
    }
    let min = d.len().saturating_sub(max_scan);
    let mut i = d.len() - 4;
    loop {
        if &d[i..i + 4] == sig {
            return Some(i);
        }
        if i <= min {
            return None;
        }
        i -= 1;
    }
}

/// 解析 EOCD，必要时走 ZIP64。返回 (cd_size, cd_offset, entry_count)
fn parse_eocd(d: &[u8]) -> Result<(u64, u64, u64)> {
    let pos = find_last(d, SIG_EOCD, MAX_EOCD_SCAN)
        .ok_or_else(|| Error::msg("zip: EOCD not found (not a zip/docx file?)"))?;
    if d.len() < pos + 22 {
        return Err(Error::msg("zip: truncated EOCD"));
    }
    let count = u16le(d, pos + 10)? as u64;
    let cd_size = u32le(d, pos + 12)? as u64;
    let cd_off = u32le(d, pos + 16)? as u64;
    if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF {
        // ZIP64：locator 紧贴 EOCD 之前（20 字节）
        let loc = pos
            .checked_sub(20)
            .filter(|p| eq_sig(d, *p, SIG_Z64_LOC))
            .ok_or_else(|| Error::msg("zip: zip64 EOCD locator missing"))?;
        let rec = u64le(d, loc + 8)? as usize;
        if !eq_sig(d, rec, SIG_Z64_REC) {
            return Err(Error::msg("zip: bad zip64 EOCD record"));
        }
        // record 布局：sig4 size8 vmake2 vneed2 disk4 cddisk4
        //            cnt_disk8 cnt_total8 cd_size8 cd_off8
        if d.len() < rec + 56 {
            return Err(Error::msg("zip: truncated zip64 EOCD"));
        }
        let count = u64le(d, rec + 32)?;
        let cd_size = u64le(d, rec + 40)?;
        let cd_off = u64le(d, rec + 48)?;
        return Ok((cd_size, cd_off, count));
    }
    Ok((cd_size, cd_off, count))
}

/// 解析 zip64 扩展字段（extra field id 0x0001），替换 0xFFFFFFFF 占位值
fn apply_zip64_extra(extra: &[u8], comp: &mut u64, uncomp: &mut u64, lho: &mut u64) {
    let mut e = 0;
    while e + 4 <= extra.len() {
        let id = u16::from_le_bytes(extra[e..e + 2].try_into().unwrap());
        let sz = u16::from_le_bytes(extra[e + 2..e + 4].try_into().unwrap()) as usize;
        let body_end = (e + 4 + sz).min(extra.len());
        let mut q = e + 4;
        if id == 0x0001 {
            // 字段顺序：uncomp(如被占位)、comp(如被占位)、lho(如被占位)
            if *uncomp == 0xFFFF_FFFF && q + 8 <= body_end {
                *uncomp = u64::from_le_bytes(extra[q..q + 8].try_into().unwrap());
                q += 8;
            }
            if *comp == 0xFFFF_FFFF && q + 8 <= body_end {
                *comp = u64::from_le_bytes(extra[q..q + 8].try_into().unwrap());
                q += 8;
            }
            if *lho == 0xFFFF_FFFF && q + 8 <= body_end {
                *lho = u64::from_le_bytes(extra[q..q + 8].try_into().unwrap());
            }
        }
        e = e + 4 + sz;
    }
}

impl<'a> ZipArchive<'a> {
    /// 建目录：只解析中央目录，不解压任何数据。
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let (cd_size, cd_off, count) = parse_eocd(data)?;
        let mut entries = Vec::new();
        let mut p = cd_off as usize;
        let end = p.saturating_add(cd_size as usize).min(data.len());
        let mut seen = 0u64;
        while seen < count && p + CDIR_HDR <= end && eq_sig(data, p, SIG_CDIR) {
            seen += 1;
            let method = u16le(data, p + 10)?;
            let mut comp = u32le(data, p + 20)? as u64;
            let mut uncomp = u32le(data, p + 24)? as u64;
            let nlen = u16le(data, p + 28)? as usize;
            let elen = u16le(data, p + 30)? as usize;
            let clen = u16le(data, p + 32)? as usize;
            let mut lho = u32le(data, p + 42)? as u64;
            if p + CDIR_HDR + nlen > data.len() {
                break; // 目录被截断，容错停止
            }
            let name =
                String::from_utf8_lossy(&data[p + CDIR_HDR..p + CDIR_HDR + nlen]).into_owned();
            let extra_end = (p + CDIR_HDR + nlen + elen).min(data.len());
            apply_zip64_extra(
                &data[(p + CDIR_HDR + nlen)..extra_end],
                &mut comp,
                &mut uncomp,
                &mut lho,
            );
            if !name.ends_with('/') {
                entries.push(Entry {
                    name,
                    method,
                    comp,
                    uncomp,
                    lho,
                });
            }
            p += CDIR_HDR + nlen + elen + clen;
        }
        Ok(ZipArchive { data, entries })
    }

    /// 按名查找 entry
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 按名读取并解压 entry；不存在返回 Err(NotFound)
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let e = self.find(name).ok_or(Error::NotFound)?;
        self.read_entry(e)
    }

    /// 读取并解压指定 entry
    pub fn read_entry(&self, e: &Entry) -> Result<Vec<u8>> {
        let d = self.data;
        let lho = e.lho as usize;
        if lho + LFH_HDR > d.len() || !eq_sig(d, lho, SIG_LFH) {
            return Err(Error::msg("zip: bad local file header"));
        }
        let nlen = u16le(d, lho + 26)? as usize;
        let elen = u16le(d, lho + 28)? as usize;
        // 设计要求校验 local header 文件名与中央目录一致
        let name_end = (lho + LFH_HDR + nlen).min(d.len());
        if &d[(lho + LFH_HDR)..name_end] != e.name.as_bytes() {
            return Err(Error::msg("zip: local header name mismatch"));
        }
        let start = lho + LFH_HDR + nlen + elen;
        let end = start
            .checked_add(e.comp as usize)
            .filter(|&end| end <= d.len())
            .ok_or_else(|| Error::msg("zip: entry data out of range"))?;
        let raw = &d[start..end];
        match e.method {
            0 => Ok(raw.to_vec()),
            8 => inflate::inflate(raw, e.uncomp as usize),
            m => {
                let mut s = String::from("zip: unsupported method ");
                crate::json::put_i64(m as i64, &mut s);
                Err(Error::msg(s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::deflate::compress_to_vec;

    /// 构造一个最小 zip（stored 或 deflate）
    fn build_zip(parts: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut cdir: Vec<u8> = Vec::new();
        let mut offsets = Vec::new();
        for (name, data) in parts {
            offsets.push(out.len() as u32);
            let (method, payload) = if deflate {
                (8u16, compress_to_vec(data, 6))
            } else {
                (0u16, data.to_vec())
            };
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0]); // version needed
            out.extend_from_slice(&[0, 0]); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]); // time/date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc（我们不校验）
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&payload);
        }
        let cd_off = out.len() as u32;
        for (i, (name, data)) in parts.iter().enumerate() {
            let (method, payload) = if deflate {
                (8u16, compress_to_vec(data, 6))
            } else {
                (0u16, data.to_vec())
            };
            cdir.extend_from_slice(b"PK\x01\x02");
            cdir.extend_from_slice(&[20, 0]); // version made by
            cdir.extend_from_slice(&[20, 0]); // version needed
            cdir.extend_from_slice(&[0, 0]); // flags
            cdir.extend_from_slice(&method.to_le_bytes());
            cdir.extend_from_slice(&[0, 0, 0, 0]);
            cdir.extend_from_slice(&0u32.to_le_bytes()); // crc
            cdir.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            cdir.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cdir.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cdir.extend_from_slice(&0u16.to_le_bytes()); // extra
            cdir.extend_from_slice(&0u16.to_le_bytes()); // comment
            cdir.extend_from_slice(&0u16.to_le_bytes()); // disk start
            cdir.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            cdir.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            cdir.extend_from_slice(&offsets[i].to_le_bytes());
            cdir.extend_from_slice(name.as_bytes());
        }
        out.extend_from_slice(&cdir);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
        out.extend_from_slice(&(parts.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cdir.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out
    }

    fn check(parts: &[(&str, &[u8])], deflate: bool) {
        let zip = build_zip(parts, deflate);
        let arc = ZipArchive::new(&zip).unwrap();
        for (name, data) in parts {
            assert_eq!(&arc.read(name).unwrap(), data, "entry {name}");
        }
        assert!(arc.read("no/such.bin").is_err());
    }

    #[test]
    fn stored_entries() {
        check(
            &[
                ("word/document.xml", b"<w:document/>".as_slice()),
                ("word/media/image1.png", &[0x89, b'P', b'N', b'G', 1, 2, 3]),
                ("empty.bin", b""),
            ],
            false,
        );
    }

    #[test]
    fn deflate_entries() {
        let big = vec![b'x'; 100_000]; // 重复数据确保压缩生效
        check(
            &[
                (
                    "word/document.xml",
                    b"<w:document>hello</w:document>".as_slice(),
                ),
                ("word/media/big.bin", &big),
                ("empty.xml", b""),
            ],
            true,
        );
    }

    #[test]
    fn trailing_garbage_after_eocd() {
        let mut zip = build_zip(&[("a.txt", b"hello")], false);
        zip.extend_from_slice(b"trailing garbage after EOCD");
        let arc = ZipArchive::new(&zip).unwrap();
        assert_eq!(arc.read("a.txt").unwrap(), b"hello");
    }

    #[test]
    fn zip64_eocd() {
        // 手工构造 zip64：普通小数据 + zip64 EOCD record + locator + EOCD(占位值)
        let data = b"hello zip64".to_vec();
        let mut out: Vec<u8> = Vec::new();
        let lho = 0u32;
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[
            20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        // 注意：上面不够严谨，重新手动拼
        out.clear();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(b"dir.txt");
        out.extend_from_slice(&data);

        let cd_off = out.len() as u64;
        let mut cd: Vec<u8> = Vec::new();
        cd.extend_from_slice(b"PK\x01\x02");
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&7u16.to_le_bytes());
        cd.extend_from_slice(&12u16.to_le_bytes()); // extra：zip64 头 4 字节 + 体 8 字节
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // lho 占位 -> 走 zip64 extra
        cd.extend_from_slice(b"dir.txt");
        // zip64 extra：id 0x0001，8 字节真实偏移
        cd.extend_from_slice(&1u16.to_le_bytes());
        cd.extend_from_slice(&8u16.to_le_bytes());
        cd.extend_from_slice(&(lho as u64).to_le_bytes());
        out.extend_from_slice(&cd);

        // zip64 EOCD record
        let z64_off = out.len() as u64;
        out.extend_from_slice(b"PK\x06\x06");
        out.extend_from_slice(&44u64.to_le_bytes()); // record size
        out.extend_from_slice(&45u16.to_le_bytes()); // version made
        out.extend_from_slice(&45u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u32.to_le_bytes()); // disk
        out.extend_from_slice(&0u32.to_le_bytes()); // cd disk
        out.extend_from_slice(&1u64.to_le_bytes()); // entries on disk
        out.extend_from_slice(&1u64.to_le_bytes()); // total entries
        out.extend_from_slice(&(cd.len() as u64).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        // zip64 EOCD locator
        out.extend_from_slice(b"PK\x06\x07");
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&z64_off.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        // 普通EOCD：占位值触发 zip64 路径
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());

        let arc = ZipArchive::new(&out).unwrap();
        assert_eq!(arc.read("dir.txt").unwrap(), data);
    }

    #[test]
    fn rejects_non_zip() {
        assert!(ZipArchive::new(b"not a zip file at all").is_err());
        assert!(ZipArchive::new(b"").is_err());
    }

    #[test]
    fn directory_entries_skipped() {
        let zip = build_zip(&[("folder/", b""), ("folder/a.txt", b"ok")], false);
        let arc = ZipArchive::new(&zip).unwrap();
        assert_eq!(arc.entries.len(), 1);
        assert_eq!(arc.read("folder/a.txt").unwrap(), b"ok");
    }

    #[test]
    fn bad_local_name_rejected() {
        let mut zip = build_zip(&[("a.txt", b"hello")], false);
        // 篡改 local header 名
        let pos = zip.windows(4).rposition(|w| w == b"PK\x03\x04").unwrap();
        zip[pos + 30] = b'z';
        assert!(ZipArchive::new(&zip).unwrap().read("a.txt").is_err());
    }
}
