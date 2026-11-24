//! Chunk objects as stored in the cloud.
//!
//! ```text
//! "LNCK"  version (1)  codec (0 = raw, 1 = zstd)  data
//! ```
//!
//! A chunk is named by the BLAKE3 hash of its raw (uncompressed) content, so
//! equal chunks are stored once. The codec is part of the object, not only
//! of the manifest, because one stored chunk can belong to many versions.
//! Later codecs (encryption) get new numbers; the layout stays.

use anyhow::{anyhow, bail, Result};

const MAGIC: &[u8; 4] = b"LNCK";
const VERSION: u8 = 1;
const RAW: u8 = 0;
const ZSTD: u8 = 1;
/// zstd level: about 300 MB/s per thread, a good ratio for game data.
const LEVEL: i32 = 3;

/// Name of a chunk: BLAKE3 of its raw content, 64 hex digits.
pub fn id(raw: &[u8]) -> String {
    blake3::hash(raw).to_hex().to_string()
}

/// The stored object for `raw`: zstd, or the raw bytes when compression
/// saves less than 3% (already compressed archives and videos).
pub fn encode(raw: &[u8]) -> Result<Vec<u8>> {
    let packed = zstd::bulk::compress(raw, LEVEL)?;
    let (codec, body) = if (packed.len() as u64) * 100 < (raw.len() as u64) * 97 {
        (ZSTD, packed.as_slice())
    } else {
        (RAW, raw)
    };
    let mut out = Vec::with_capacity(body.len() + 6);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(codec);
    out.extend_from_slice(body);
    Ok(out)
}

/// The raw content of a stored object, checked against its name and size.
pub fn decode(obj: &[u8], expected_id: &str, expected_len: u64) -> Result<Vec<u8>> {
    let bad = || anyhow!(crate::tr!("云端的数据块 {expected_id} 已损坏", "The chunk {expected_id} in the cloud is damaged"));
    if obj.len() < 6 || &obj[..4] != MAGIC {
        return Err(bad());
    }
    if obj[4] != VERSION {
        bail!(crate::tr!(
            "数据块 {expected_id} 的格式版本 {} 不认识，请更新 Ludonimbus",
            "Chunk {expected_id} has format version {}, which this Ludonimbus doesn't know. Update Ludonimbus",
            obj[4]
        ));
    }
    let body = &obj[6..];
    let raw = match obj[5] {
        RAW => body.to_vec(),
        ZSTD => zstd::bulk::decompress(body, expected_len as usize).map_err(|_| bad())?,
        c => bail!(crate::tr!(
            "数据块 {expected_id} 的编码 {c} 不认识，请更新 Ludonimbus",
            "Chunk {expected_id} uses encoding {c}, which this Ludonimbus doesn't know. Update Ludonimbus"
        )),
    };
    if raw.len() as u64 != expected_len || id(&raw) != expected_id {
        return Err(bad());
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    #[test]
    fn compressible_data_is_zstd() {
        let raw = b"game data ".repeat(10_000);
        let obj = encode(&raw).unwrap();
        assert_eq!(&obj[..6], b"LNCK\x01\x01");
        assert!(obj.len() < raw.len() / 10);
        assert_eq!(decode(&obj, &id(&raw), raw.len() as u64).unwrap(), raw);
    }

    #[test]
    fn incompressible_data_is_stored_raw() {
        let mut raw = vec![0u8; 100_000];
        rand::thread_rng().fill_bytes(&mut raw);
        let obj = encode(&raw).unwrap();
        assert_eq!(&obj[..6], b"LNCK\x01\x00");
        assert_eq!(&obj[6..], &raw[..]);
        assert_eq!(decode(&obj, &id(&raw), raw.len() as u64).unwrap(), raw);
    }

    #[test]
    fn damage_is_detected() {
        let raw = b"hello hello hello hello".repeat(100);
        let name = id(&raw);
        let mut obj = encode(&raw).unwrap();
        assert!(decode(&obj, &name, raw.len() as u64 + 1).is_err());
        assert!(decode(&obj, &id(b"other"), raw.len() as u64).is_err());
        let last = obj.len() - 1;
        obj[last] ^= 0xff;
        assert!(decode(&obj, &name, raw.len() as u64).is_err());
        assert!(decode(b"LN", &name, 1).is_err());
        let mut future = encode(&raw).unwrap();
        future[5] = 9;
        assert!(format!("{:#}", decode(&future, &name, raw.len() as u64).unwrap_err()).contains('9'));
        assert_eq!(id(b""), blake3::hash(b"").to_hex().to_string());
    }
}
