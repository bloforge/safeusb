use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use safeusb::img::{self, BackupOptions, Stats};

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("safeusb-img-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TmpDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn rand_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.max(1);
    (0..len)
        .map(|_| {
             state ^= state >> 12;
             state ^= state << 25;
             state ^= state >> 27;
             (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
         })
        .collect()
}

fn opts(chunk_size: u32) -> BackupOptions {
    BackupOptions {
        chunk_size,
        zstd_level: 3,
        long_window_log: 27,
    }
}

fn backup_to(data: &[u8], archive: &Path, chunk_size: u32) -> Stats {
    let mut src = Cursor::new(data);
    let mut progress = |_done: u64, _total: u64| {};
    img::backup(
         &mut src,
         data.len() as u64,
         archive,
         &opts(chunk_size),
         &mut progress,
     )
    .unwrap()
}

fn restore_bytes(archive: &Path) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut progress = |_done: u64, _total: u64| {};
    img::restore(archive, &mut out, &mut progress)?;
    Ok(out)
}

fn roundtrip(name: &str, data: &[u8], chunk_size: u32) -> Stats {
    let dir = TmpDir::new(name);
    let archive = dir.path().join("image.safeusb");
    let stats = backup_to(data, &archive, chunk_size);

    let restored = restore_bytes(&archive).expect("restore failed");
    assert_eq!(restored.len(), data.len(), "length is off");
    assert!(restored == data, "bytes do not match the source");

    let entries: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
         entries.len(),
         1,
         "only the final file should be left, found {entries:?}"
     );
    assert_eq!(stats.archive_bytes, fs::metadata(&archive).unwrap().len());
    stats
}

#[test]
fn random_roundtrip() {
    let chunk = 64 * 1024;
    let data = rand_bytes(1024 * 1024 + 4567, 0x1234_5678);
    let stats = roundtrip("random", &data, chunk);
    assert_eq!(stats.image_size, data.len() as u64);
    assert_eq!(stats.chunks_total, 17);
    assert_eq!(stats.chunks_unique, 17);
    assert_eq!(stats.zero_chunks, 0);
}

#[test]
fn zeros_use_markers() {
    let chunk = 64 * 1024;
    let data = vec![0u8; 1024 * 1024];
    let stats = roundtrip("zeros", &data, chunk);
    assert_eq!(stats.chunks_total, 16);
    assert_eq!(stats.zero_chunks, 16);
    assert_eq!(stats.chunks_unique, 0);
    assert!(
         stats.archive_bytes < 1024,
         "zero image stored {} bytes",
         stats.archive_bytes
     );
}

#[test]
fn dedup_works() {
    let chunk = 64 * 1024;
    let pattern = rand_bytes(chunk as usize, 42);
    let data: Vec<u8> = pattern
        .iter()
        .cycle()
        .take(chunk as usize * 8)
        .copied()
        .collect();
    let stats = roundtrip("repeated", &data, chunk);
    assert_eq!(stats.chunks_total, 8);
    assert_eq!(stats.chunks_unique, 1);
    assert_eq!(stats.zero_chunks, 0);
    assert!(
         stats.archive_bytes < 2 * chunk as u64,
         "repeats got stored, {} bytes",
         stats.archive_bytes
     );
}

#[test]
fn odd_length() {
    let chunk = 4096;
    let data = rand_bytes(100_003, 7);
    let stats = roundtrip("odd_length", &data, chunk);
    assert_eq!(stats.chunks_total, 25);
    assert_eq!(stats.chunks_unique, 25);
}

#[test]
fn zero_tail_mixed() {
    let chunk = 4096;
    let mut data = rand_bytes(2 * 4096, 99);
    data.extend(std::iter::repeat_n(0u8, 1000));
    let stats = roundtrip("zero_tail", &data, chunk);
    assert_eq!(stats.chunks_total, 3);
    assert_eq!(stats.zero_chunks, 1);
}

#[test]
fn empty_src() {
    let stats = roundtrip("empty", &[], 4096);
    assert_eq!(stats.chunks_total, 0);
    assert_eq!(stats.image_size, 0);
}

#[test]
fn default_opts() {
    let dir = TmpDir::new("defaults");
    let archive = dir.path().join("image.safeusb");
    let data = rand_bytes(300_001, 5);
    let mut src = Cursor::new(&data);
    let mut progress = |_done: u64, _total: u64| {};
    img::backup(
         &mut src,
         data.len() as u64,
         &archive,
         &BackupOptions::default(),
         &mut progress,
     )
    .unwrap();
    assert_eq!(restore_bytes(&archive).unwrap(), data);

    let info = img::info(&archive).unwrap();
    assert_eq!(info.image_size, data.len() as u64);
    assert_eq!(info.chunk_size, 1 << 20);
    assert_eq!(info.zstd_level, 9);
}

#[test]
fn info_and_verify() {
    let dir = TmpDir::new("info_verify");
    let archive = dir.path().join("image.safeusb");
    let data = rand_bytes(10_000, 3);
    backup_to(&data, &archive, 4096);

    let info = img::info(&archive).unwrap();
    assert_eq!(info.image_size, 10_000);
    assert_eq!(info.chunk_size, 4096);
    assert_eq!(info.zstd_level, 3);

    let mut progress = |_done: u64, _total: u64| {};
    let stats = img::verify(&archive, &mut progress).unwrap();
    assert_eq!(stats.chunks_total, 3);
}

#[test]
fn progress_hits_total() {
    let dir = TmpDir::new("progress");
    let archive = dir.path().join("image.safeusb");
    let data = rand_bytes(9000, 11);
    let mut last = (0u64, 0u64);
    let mut progress = |done: u64, total: u64| last = (done, total);
    img::backup(
         &mut Cursor::new(&data),
         9000,
         &archive,
         &opts(4096),
         &mut progress,
     )
    .unwrap();
    assert_eq!(last, (9000, 9000));
}

#[test]
fn short_src_fails() {
    let dir = TmpDir::new("short_source");
    let archive = dir.path().join("image.safeusb");
    let data = rand_bytes(5000, 13);
    let mut progress = |_done: u64, _total: u64| {};
    let result = img::backup(
         &mut Cursor::new(&data),
         9000,
         &archive,
         &opts(4096),
         &mut progress,
     );
    assert!(result.is_err());
    assert!(!archive.exists(), "no archive at the final path");
    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert!(entries.is_empty(), "partial file should be cleaned up");
}

#[test]
fn flipped_byte_caught() {
    let dir = TmpDir::new("corrupt");
    let archive = dir.path().join("image.safeusb");
    let mut data = rand_bytes(512 * 3, 21);
    data.extend(vec![0u8; 512]);
    data.extend(rand_bytes(512, 21));
    backup_to(&data, &archive, 512);
    let good = fs::read(&archive).unwrap();
    assert_eq!(restore_bytes(&archive).unwrap(), data);

    let mut missed = Vec::new();
    for offset in 0..good.len() {
        let mut bad = good.clone();
        bad[offset] ^= 0x5A;
        fs::write(&archive, &bad).unwrap();
        if restore_bytes(&archive).is_ok() {
            missed.push(offset);
        }
    }
    fs::write(&archive, &good).unwrap();
    assert!(
         missed.is_empty(),
         "missed corruption at {missed:?} (of {})",
         good.len()
     );
}

#[test]
fn truncated_fails() {
    let dir = TmpDir::new("truncated");
    let archive = dir.path().join("image.safeusb");
    backup_to(&rand_bytes(20_000, 8), &archive, 4096);
    let mut bytes = fs::read(&archive).unwrap();
    bytes.pop();
    fs::write(&archive, &bytes).unwrap();
    assert!(restore_bytes(&archive).is_err());
}
