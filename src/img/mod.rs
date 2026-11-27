mod wire;

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use wire::{
    Entry, Footer, Header, BLOB_PREFIX_LEN, FOOTER_LEN, HASH_LEN, HEADER_LEN, INDEX_ENTRY_LEN,
};
use zstd::zstd_safe::{CParameter, DParameter};

#[derive(Debug, Clone)]
pub struct BackupOptions {
    pub chunk_size: u32,
    pub zstd_level: i32,
    pub long_window_log: u32,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            chunk_size: 1 << 20,
            zstd_level: 9,
            long_window_log: 31,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub image_size: u64,
    pub chunks_total: u64,
    pub chunks_unique: u64,
    pub zero_chunks: u64,
    pub archive_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub image_size: u64,
    pub chunk_size: u32,
    pub zstd_level: i32,
}

pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

const PROGRESS_STEP: u64 = 4 << 20;

pub fn backup(
     src: &mut dyn Read,
     src_len: u64,
     archive_path: &Path,
     opts: &BackupOptions,
     progress: Progress,
) -> anyhow::Result<Stats> {
    ensure!(opts.chunk_size > 0, "chunk size must be at least 1");
    ensure!(
         (10..=31).contains(&opts.long_window_log),
         "long window log must be 10..31, got {}",
         opts.long_window_log
     );

    let header = Header {
        image_size: src_len,
        chunk_size: opts.chunk_size,
        zstd_level: opts.zstd_level,
        long_window_log: opts.long_window_log,
    };
    let header_bytes = header.encode();
    let header_hash = blake3::hash(&header_bytes);

    let mut partial = Partial::new(archive_path)?;
    let mut out = BufWriter::new(
         File::create(partial.path())
             .with_context(|| format!("failed to create {}", partial.path().display()))?,
     );
    out.write_all(&header_bytes)?;
    let mut pos = HEADER_LEN as u64;

    let mut comp = zstd::bulk::Compressor::new(opts.zstd_level)?;
    comp.set_parameter(CParameter::WindowLog(opts.long_window_log))?;
    comp.set_parameter(CParameter::EnableLongDistanceMatching(true))?;

    let count = wire::chunk_count(src_len, opts.chunk_size);
    let mut index: Vec<u8> = Vec::with_capacity(count as usize * INDEX_ENTRY_LEN);
    let mut seen: HashMap<[u8; HASH_LEN], (u64, u32)> = HashMap::new();
    let mut buf = vec![0u8; (opts.chunk_size as u64).min(src_len) as usize];
    let mut stats = Stats {
        image_size: src_len,
        chunks_total: count,
        ..Stats::default()
    };

    let mut done = 0u64;
    let mut last = 0u64;
    progress(0, src_len);
    for i in 0..count {
        let len = wire::chunk_len(src_len, opts.chunk_size, i) as usize;
        let chunk = &mut buf[..len];
        let got = fill_chunk(src, chunk).with_context(|| format!("failed to read chunk {i}"))?;
        ensure!(
             got == len,
             "source ended early: got {} bytes, expected {src_len}",
             done + got as u64
         );

        let entry = if chunk.iter().all(|&b| b == 0) {
            stats.zero_chunks += 1;
            Entry::Zero
        } else {
            let hash = *blake3::hash(chunk).as_bytes();
            match seen.get(&hash) {
                Some(&(offset, comp_len)) => Entry::Blob { offset, comp_len },
                None => {
                    let packed = comp
                        .compress(chunk)
                        .with_context(|| format!("failed to compress chunk {i}"))?;
                    let comp_len =
                        u32::try_from(packed.len()).context("compressed chunk is too large")?;
                    out.write_all(&hash)?;
                    out.write_all(&packed)?;
                    let offset = pos;
                    pos += BLOB_PREFIX_LEN + packed.len() as u64;
                    seen.insert(hash, (offset, comp_len));
                    Entry::Blob { offset, comp_len }
                }
            }
        };
        index.extend_from_slice(&wire::encode_entry(entry));

        done += len as u64;
        maybe_report(progress, done, src_len, &mut last);
    }

    let index_offset = pos;
    out.write_all(&index)?;
    pos += index.len() as u64;
    let footer = Footer {
        index_offset,
        header_hash: *header_hash.as_bytes(),
        index_hash: *blake3::hash(&index).as_bytes(),
    };
    out.write_all(&footer.encode())?;
    pos += FOOTER_LEN as u64;

    out.flush()?;
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);
    partial.commit()?;

    stats.chunks_unique = seen.len() as u64;
    stats.archive_bytes = pos;
    Ok(stats)
}

pub fn restore(archive_path: &Path, dst: &mut dyn Write, progress: Progress) -> anyhow::Result<Stats> {
    restore_inner(archive_path, dst, progress)
}

pub fn info(archive_path: &Path) -> anyhow::Result<ArchiveInfo> {
    let mut file = open_file(archive_path)?;
    let frame = read_frame(&mut file)?;
    Ok(ArchiveInfo {
         image_size: frame.header.image_size,
         chunk_size: frame.header.chunk_size,
         zstd_level: frame.header.zstd_level,
     })
}

pub fn verify(archive_path: &Path, progress: Progress) -> anyhow::Result<Stats> {
    restore_inner(archive_path, &mut io::sink(), progress)
}

struct Partial {
    tmp: PathBuf,
    dest: PathBuf,
    committed: bool,
}

impl Partial {
    fn new(dest: &Path) -> Result<Self> {
        let name = dest.file_name().context("archive path has no name")?;
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".partial-{}", std::process::id()));
        let tmp = match dest.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir.join(tmp_name),
            _ => PathBuf::from(tmp_name),
        };
        Ok(Self {
             tmp,
             dest: dest.to_path_buf(),
             committed: false,
         })
    }

    fn path(&self) -> &Path {
        &self.tmp
    }

    fn commit(&mut self) -> Result<()> {
        fs::rename(&self.tmp, &self.dest).with_context(|| {
             format!("failed to rename {} to {}", self.tmp.display(), self.dest.display())
         })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

struct Frame {
    len: u64,
    header: Header,
    footer: Footer,
}

fn open_file(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("failed to open archive {}", path.display()))
}

fn read_frame(file: &mut File) -> Result<Frame> {
    let len = file.metadata()?.len();
    ensure!(
         len >= (HEADER_LEN + FOOTER_LEN) as u64,
         "file too small to be a safeusb archive"
     );

    file.seek(SeekFrom::Start(0))?;
    let mut header_buf = [0u8; HEADER_LEN];
    file.read_exact(&mut header_buf)?;
    let header = Header::decode(&header_buf)?;

    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut footer_buf = [0u8; FOOTER_LEN];
    file.read_exact(&mut footer_buf)?;
    let footer = Footer::decode(&footer_buf)?;

    ensure!(
         blake3::hash(&header_buf).as_bytes() == &footer.header_hash,
         "header hash mismatch, archive is corrupt"
     );
    Ok(Frame {
         len,
         header,
         footer,
     })
}

fn restore_inner(
     archive_path: &Path,
     dst: &mut dyn Write,
     progress: &mut dyn FnMut(u64, u64),
) -> Result<Stats> {
    let mut file = open_file(archive_path)?;
    let frame = read_frame(&mut file)?;
    let header = frame.header;
    let index_offset = frame.footer.index_offset;

    let count = wire::chunk_count(header.image_size, header.chunk_size);
    let index_len = count
        .checked_mul(INDEX_ENTRY_LEN as u64)
        .context("index is too large, archive is corrupt")?;
    let index_end = index_offset
        .checked_add(index_len)
        .context("index offset overflows, archive is corrupt")?;
    ensure!(
         index_offset >= HEADER_LEN as u64 && index_end == frame.len - FOOTER_LEN as u64,
         "index position does not match file size, archive is truncated or corrupt"
     );

    file.seek(SeekFrom::Start(index_offset))?;
    let mut index = vec![0u8; index_len as usize];
    file.read_exact(&mut index)?;
    ensure!(
         blake3::hash(&index).as_bytes() == &frame.footer.index_hash,
         "index hash mismatch, archive is corrupt"
     );

    let entries = index
        .chunks(INDEX_ENTRY_LEN)
        .map(wire::decode_entry)
        .collect::<Result<Vec<Entry>>>()?;

    let mut stats = Stats {
        image_size: header.image_size,
        chunks_total: count,
        archive_bytes: frame.len,
        ..Stats::default()
    };
    let mut blob_offsets = HashSet::new();
    for entry in &entries {
        match *entry {
            Entry::Zero => stats.zero_chunks += 1,
            Entry::Blob { offset, .. } => {
                blob_offsets.insert(offset);
            }
        }
    }
    stats.chunks_unique = blob_offsets.len() as u64;

    let mut dec = zstd::bulk::Decompressor::new()?;
    dec.set_parameter(DParameter::WindowLogMax(header.long_window_log))?;

    let zeros = vec![0u8; (header.chunk_size as u64).min(header.image_size) as usize];
    let mut record = Vec::new();
    let mut done = 0u64;
    let mut last = 0u64;
    progress(0, header.image_size);
    for (i, entry) in entries.iter().enumerate() {
        let i = i as u64;
        let len = wire::chunk_len(header.image_size, header.chunk_size, i) as usize;
        match *entry {
            Entry::Zero => dst.write_all(&zeros[..len])?,
            Entry::Blob { offset, comp_len } => {
                let end = offset.checked_add(BLOB_PREFIX_LEN + u64::from(comp_len));
                ensure!(
                     offset >= HEADER_LEN as u64 && end.is_some_and(|e| e <= index_offset),
                     "chunk {i}: blob offset out of range, archive is corrupt"
                 );
                file.seek(SeekFrom::Start(offset))?;
                let mut stored = [0u8; HASH_LEN];
                file.read_exact(&mut stored)?;
                record.resize(comp_len as usize, 0);
                file.read_exact(&mut record)?;

                let raw = dec
                    .decompress(&record, len)
                    .with_context(|| format!("chunk {i}: failed to decompress, archive is corrupt"))?;
                ensure!(
                     raw.len() == len,
                     "chunk {i}: decompressed to {} bytes, expected {len}",
                     raw.len()
                 );
                ensure!(
                     blake3::hash(&raw).as_bytes() == &stored,
                     "chunk {i}: hash mismatch, not written"
                 );
                dst.write_all(&raw)?;
            }
        }
        done += len as u64;
        maybe_report(progress, done, header.image_size, &mut last);
    }
    dst.flush()?;
    Ok(stats)
}

fn fill_chunk(src: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match src.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn maybe_report(progress: &mut dyn FnMut(u64, u64), done: u64, total: u64, last: &mut u64) {
    if done == total || done - *last >= PROGRESS_STEP {
        progress(done, total);
        *last = done;
    }
}
