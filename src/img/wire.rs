use anyhow::{bail, ensure, Result};

pub(super) const MAGIC: [u8; 8] = *b"SAFEUSB\0";
pub(super) const VERSION: u16 = 1;
pub(super) const HASH_LEN: usize = 32;
pub(super) const HEADER_LEN: usize = 30;
pub(super) const FOOTER_LEN: usize = 80;
pub(super) const INDEX_ENTRY_LEN: usize = 13;
pub(super) const BLOB_PREFIX_LEN: u64 = HASH_LEN as u64;

const KIND_ZERO: u8 = 0;
const KIND_BLOB: u8 = 1;

fn arr<const N: usize>(s: &[u8]) -> [u8; N] {
    s.try_into()
        .expect("caller passes a slice of exactly N bytes")
}

fn le64(s: &[u8]) -> u64 {
    u64::from_le_bytes(arr(s))
}

fn le32(s: &[u8]) -> u32 {
    u32::from_le_bytes(arr(s))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    pub image_size: u64,
    pub chunk_size: u32,
    pub zstd_level: i32,
    pub long_window_log: u32,
}

impl Header {
    pub(super) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..8].copy_from_slice(&MAGIC);
        b[8..10].copy_from_slice(&VERSION.to_le_bytes());
        b[10..18].copy_from_slice(&self.image_size.to_le_bytes());
        b[18..22].copy_from_slice(&self.chunk_size.to_le_bytes());
        b[22..26].copy_from_slice(&self.zstd_level.to_le_bytes());
        b[26..30].copy_from_slice(&self.long_window_log.to_le_bytes());
        b
    }

    pub(super) fn decode(b: &[u8; HEADER_LEN]) -> Result<Self> {
        ensure!(b[0..8] == MAGIC, "not a safeusb archive (bad magic)");
        let version = u16::from_le_bytes(arr(&b[8..10]));
        ensure!(version == VERSION, "unsupported archive version {version}");
        let header = Header {
            image_size: le64(&b[10..18]),
            chunk_size: le32(&b[18..22]),
            zstd_level: i32::from_le_bytes(arr(&b[22..26])),
            long_window_log: le32(&b[26..30]),
        };
        ensure!(
             header.chunk_size > 0,
             "header has chunk size 0, archive is corrupt"
         );
        ensure!(
             (10..=31).contains(&header.long_window_log),
             "invalid long window log {} in header, archive is corrupt",
             header.long_window_log
         );
        Ok(header)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Footer {
    pub index_offset: u64,
    pub header_hash: [u8; HASH_LEN],
    pub index_hash: [u8; HASH_LEN],
}

impl Footer {
    pub(super) fn encode(&self) -> [u8; FOOTER_LEN] {
        let mut b = [0u8; FOOTER_LEN];
        b[0..8].copy_from_slice(&self.index_offset.to_le_bytes());
        b[8..40].copy_from_slice(&self.header_hash);
        b[40..72].copy_from_slice(&self.index_hash);
        b[72..80].copy_from_slice(&MAGIC);
        b
    }

    pub(super) fn decode(b: &[u8; FOOTER_LEN]) -> Result<Self> {
        ensure!(
             b[72..80] == MAGIC,
             "footer missing, file is truncated or not a safeusb archive"
         );
        Ok(Footer {
             index_offset: le64(&b[0..8]),
             header_hash: arr(&b[8..40]),
             index_hash: arr(&b[40..72]),
         })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Entry {
    Zero,
    Blob { offset: u64, comp_len: u32 },
}

pub(super) fn encode_entry(entry: Entry) -> [u8; INDEX_ENTRY_LEN] {
    let (kind, offset, comp_len) = match entry {
        Entry::Zero => (KIND_ZERO, 0, 0),
        Entry::Blob { offset, comp_len } => (KIND_BLOB, offset, comp_len),
    };
    let mut b = [0u8; INDEX_ENTRY_LEN];
    b[0] = kind;
    b[1..9].copy_from_slice(&offset.to_le_bytes());
    b[9..13].copy_from_slice(&comp_len.to_le_bytes());
    b
}

pub(super) fn decode_entry(b: &[u8]) -> Result<Entry> {
    let offset = le64(&b[1..9]);
    let comp_len = le32(&b[9..13]);
    match b[0] {
        KIND_ZERO => {
            ensure!(
                 offset == 0 && comp_len == 0,
                 "invalid zero entry in index, archive is corrupt"
             );
            Ok(Entry::Zero)
        }
        KIND_BLOB => Ok(Entry::Blob { offset, comp_len }),
        kind => bail!("unknown chunk kind {kind} in index, archive is corrupt"),
    }
}

pub(super) fn chunk_count(image_size: u64, chunk_size: u32) -> u64 {
    image_size.div_ceil(u64::from(chunk_size))
}

pub(super) fn chunk_len(image_size: u64, chunk_size: u32, index: u64) -> u64 {
    let start = index * u64::from(chunk_size);
    (image_size - start).min(u64::from(chunk_size))
}
