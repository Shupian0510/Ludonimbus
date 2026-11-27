//! Content-defined chunking (FastCDC with normalized chunking).
//!
//! Cut points depend on the bytes, not on offsets: after data is inserted
//! or removed, the chunk boundaries further on fall in the same places again,
//! so a game update only produces new chunks around what changed.
//!
//! The gear table and the masks are generated from fixed constants and must
//! never change: chunks of earlier backups would no longer match (nothing
//! breaks, but every chunk would be uploaded again). [`Params`] are stored in
//! each manifest.

use serde::{Deserialize, Serialize};
use std::io::Read;

const MIB: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Always `fastcdc2020`.
    pub algo: Algo,
    pub min: usize,
    pub avg: usize,
    pub max: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Algo {
    #[serde(rename = "fastcdc2020")]
    FastCdc2020,
}

impl Default for Params {
    /// 2 / 8 / 24 MiB: about 6400 chunks for a 50 GB game.
    fn default() -> Self {
        Params {
            algo: Algo::FastCdc2020,
            min: 2 * MIB,
            avg: 8 * MIB,
            max: 24 * MIB,
        }
    }
}

impl Params {
    /// Small chunks, for tests.
    pub fn tiny() -> Self {
        Params {
            algo: Algo::FastCdc2020,
            min: 4 * 1024,
            avg: 16 * 1024,
            max: 64 * 1024,
        }
    }

    fn masks(&self) -> (u64, u64) {
        let bits = (self.avg.max(2) as f64).log2().round() as u32;
        // Normalization level 2: harder to cut before the average size,
        // easier after it, so sizes cluster around the average.
        (top_bits(bits + 2), top_bits(bits.saturating_sub(2).max(1)))
    }
}

/// A mask of the `n` highest bits: with `fp = (fp << 1) + gear[b]`, bit k
/// depends on the last k + 1 bytes, so the high bits see a window of about
/// 40 to 64 bytes.
fn top_bits(n: u32) -> u64 {
    let n = n.clamp(1, 63);
    !0u64 << (64 - n)
}

/// 256 pseudo-random values from splitmix64 with a fixed seed.
const fn gear_table() -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut state: u64 = 0x4C55_444F_4E49_4D42; // "LUDONIMB"
    let mut i = 0;
    while i < 256 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        t[i] = z ^ (z >> 31);
        i += 1;
    }
    t
}

static GEAR: [u64; 256] = gear_table();

/// Length of the first chunk of `data`. `data` holds at least `max` bytes,
/// or everything that is left (`eof`).
fn cut(data: &[u8], p: &Params, masks: (u64, u64)) -> usize {
    let n = data.len();
    if n <= p.min {
        return n;
    }
    let limit = n.min(p.max);
    let normal = p.avg.min(limit);
    let (mask_s, mask_l) = masks;
    let mut fp: u64 = 0;
    let mut i = p.min;
    while i < normal {
        fp = (fp << 1).wrapping_add(GEAR[data[i] as usize]);
        if fp & mask_s == 0 {
            return i + 1;
        }
        i += 1;
    }
    while i < limit {
        fp = (fp << 1).wrapping_add(GEAR[data[i] as usize]);
        if fp & mask_l == 0 {
            return i + 1;
        }
        i += 1;
    }
    limit
}

/// Splits a byte stream into chunks.
pub struct Chunker<R: Read> {
    reader: R,
    params: Params,
    masks: (u64, u64),
    buf: Vec<u8>,
    start: usize,
    eof: bool,
}

impl<R: Read> Chunker<R> {
    pub fn new(reader: R, params: Params) -> Self {
        Self::with_buffer(reader, params, Vec::new())
    }

    /// Like [`Chunker::new`], reusing the buffer of an earlier chunker
    /// ([`Chunker::into_buffer`]): it holds twice the largest chunk, 48 MiB
    /// by default, too much to allocate again for every big file.
    pub fn with_buffer(reader: R, params: Params, mut buf: Vec<u8>) -> Self {
        buf.clear();
        buf.reserve(params.max * 2);
        Chunker {
            reader,
            masks: params.masks(),
            params,
            buf,
            start: 0,
            eof: false,
        }
    }

    pub fn into_buffer(self) -> Vec<u8> {
        self.buf
    }

    /// The next chunk, `None` at the end of the stream.
    pub fn next_chunk(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        // Keep at least `max` bytes after `start` (or everything left).
        while !self.eof && self.buf.len() - self.start < self.params.max {
            if self.start > 0 {
                self.buf.drain(..self.start);
                self.start = 0;
            }
            let have = self.buf.len();
            self.buf.resize(self.params.max * 2, 0);
            let got = loop {
                match self.reader.read(&mut self.buf[have..]) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        self.buf.truncate(have);
                        return Err(e);
                    }
                }
            };
            self.buf.truncate(have + got);
            if got == 0 {
                self.eof = true;
            }
        }
        let rest = &self.buf[self.start..];
        if rest.is_empty() {
            return Ok(None);
        }
        let len = cut(rest, &self.params, self.masks);
        let chunk = rest[..len].to_vec();
        self.start += len;
        Ok(Some(chunk))
    }
}

impl<R: Read> Iterator for Chunker<R> {
    type Item = std::io::Result<Vec<u8>>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_chunk().transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngCore, SeedableRng};

    const PIN0: u64 = 0xa6b1_1a22_8ebc_e1bd;
    const PIN255: u64 = 0xff72_e6e3_509d_aa33;
    const PINSUM: u64 = 0xd413_1af3_cf06_3f09;

    fn data(len: usize, seed: u64) -> Vec<u8> {
        let mut v = vec![0u8; len];
        rand::rngs::StdRng::seed_from_u64(seed).fill_bytes(&mut v);
        v
    }

    fn chunks(d: &[u8], p: Params) -> Vec<Vec<u8>> {
        Chunker::new(d, p).collect::<std::io::Result<Vec<_>>>().unwrap()
    }

    /// Reads at most 1000 bytes at a time, to check refilling the buffer.
    struct Trickle<'a>(&'a [u8]);
    impl Read for Trickle<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = out.len().min(self.0.len()).min(1000);
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    #[test]
    fn gear_table_is_fixed() {
        // Changing the table would make every earlier chunk mismatch.
        let sum = GEAR.iter().fold(0u64, |a, &g| a.rotate_left(1) ^ g);
        assert_eq!((GEAR[0], GEAR[255], sum), (PIN0, PIN255, PINSUM));
    }

    #[test]
    fn chunks_cover_the_stream_within_bounds() {
        let p = Params::tiny();
        let d = data(2 * 1024 * 1024 + 123, 1);
        let c = chunks(&d, p);
        assert_eq!(c.concat(), d);
        for x in &c[..c.len() - 1] {
            assert!(x.len() >= p.min && x.len() <= p.max, "{}", x.len());
        }
        let avg = d.len() / c.len();
        assert!(avg > p.avg / 2 && avg < p.avg * 2, "average {avg}");
        // Same cuts whatever the read sizes are.
        let trickled = Chunker::new(Trickle(&d), p).collect::<std::io::Result<Vec<_>>>().unwrap();
        assert_eq!(trickled, c);
    }

    #[test]
    fn a_reused_buffer_cuts_the_same() {
        let p = Params::tiny();
        let (a, b) = (data(300_000, 6), data(200_000, 7));
        let mut first = Chunker::new(&a[..], p);
        let from_a: Vec<Vec<u8>> = first.by_ref().collect::<std::io::Result<_>>().unwrap();
        let buf = first.into_buffer();
        assert!(buf.capacity() >= p.max * 2);
        let from_b: Vec<Vec<u8>> = Chunker::with_buffer(&b[..], p, buf).collect::<std::io::Result<_>>().unwrap();
        assert_eq!((from_a, from_b), (chunks(&a, p), chunks(&b, p)));
    }

    #[test]
    fn small_and_empty_streams() {
        let p = Params::tiny();
        assert!(chunks(&[], p).is_empty());
        assert_eq!(chunks(b"abc", p), vec![b"abc".to_vec()]);
        // Data that never matches the mask is cut at the maximum size.
        let zeros = vec![0u8; p.max * 3 + 5];
        let c = chunks(&zeros, p);
        assert_eq!(c.concat(), zeros);
        assert!(c.iter().all(|x| x.len() <= p.max));
    }

    #[test]
    fn an_insertion_only_changes_nearby_chunks() {
        let p = Params::tiny();
        let d = data(4 * 1024 * 1024, 2);
        let mut changed = d.clone();
        let at = d.len() / 3;
        changed.splice(at..at, data(777, 3));
        let ids = |c: &[Vec<u8>]| -> std::collections::HashSet<String> {
            c.iter().map(|x| blake3::hash(x).to_hex().to_string()).collect()
        };
        let before = ids(&chunks(&d, p));
        let after_chunks = chunks(&changed, p);
        let new = ids(&after_chunks).difference(&before).count();
        assert!(new <= 3, "{new} new chunks out of {}", after_chunks.len());
    }

    /// `cargo test --release -p ludonimbus-core --lib chunker::tests::throughput -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn throughput() {
        let d = data(512 * 1024 * 1024, 4);
        let t = std::time::Instant::now();
        let c = chunks(&d, Params::default());
        let cut = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let ids: Vec<String> = c.iter().map(|x| crate::gamebackup::chunk::id(x)).collect();
        let hash = t.elapsed().as_secs_f64();
        let mb = d.len() as f64 / 1048576.0;
        println!(
            "{} chunks, average {:.1} MiB; chunking {:.0} MB/s, BLAKE3 {:.0} MB/s",
            ids.len(),
            mb / ids.len() as f64,
            mb / cut,
            mb / hash
        );
        // Words in random order with random numbers: compresses roughly like game text and tables.
        let words = ["entity", "dialogue", "level", "texture", "0.75", "spawn", "door", "{", "}", "true", "npc", "quest"];
        let r = data(8 * 1024 * 1024, 5);
        let text: Vec<u8> = r
            .chunks(2)
            .flat_map(|b| format!("{} {} ", words[b[0] as usize % words.len()], b[1]).into_bytes())
            .take(8 * 1024 * 1024)
            .collect();
        let t = std::time::Instant::now();
        let obj = crate::gamebackup::chunk::encode(&text).unwrap();
        println!(
            "zstd: {:.0} MB/s, {:.1}% of the size",
            text.len() as f64 / 1048576.0 / t.elapsed().as_secs_f64(),
            obj.len() as f64 * 100.0 / text.len() as f64
        );
    }
}
