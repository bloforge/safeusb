use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use safeusb::input::{detect, open, Kind};

struct TmpFile(PathBuf);

impl TmpFile {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "safeusb-input-{}-{}",
            name,
            std::process::id()
        ));
        TmpFile(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TmpFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sample_bytes() -> Vec<u8> {
    (0..40_000u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
}

fn write_tmp(name: &str, bytes: &[u8]) -> TmpFile {
    let tmp = TmpFile::new(name);
    let mut f = fs::File::create(tmp.path()).unwrap();
    f.write_all(bytes).unwrap();
    tmp
}

fn read_all(path: &Path) -> Vec<u8> {
    let mut src = open(path).unwrap();
    let mut out = Vec::new();
    src.reader.read_to_end(&mut out).unwrap();
    out
}

#[test]
fn detects_raw() {
    let tmp = write_tmp("raw", &sample_bytes());
    assert_eq!(detect(tmp.path()).unwrap(), Kind::Raw);
}

#[test]
fn detects_iso() {
    let mut bytes = vec![0u8; 40_000];
    bytes[32769..32774].copy_from_slice(b"CD001");
    let tmp = write_tmp("iso", &bytes);
    assert_eq!(detect(tmp.path()).unwrap(), Kind::Iso);

    let src = open(tmp.path()).unwrap();
    assert_eq!(src.kind, Kind::Iso);
    assert_eq!(src.size, Some(40_000));
}

#[test]
fn raw_size_matches_file_length() {
    let bytes = sample_bytes();
    let tmp = write_tmp("raw_size", &bytes);
    let src = open(tmp.path()).unwrap();
    assert_eq!(src.kind, Kind::Raw);
    assert_eq!(src.size, Some(bytes.len() as u64));
}

#[test]
fn detects_gzip() {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&sample_bytes()).unwrap();
    let bytes = enc.finish().unwrap();
    let tmp = write_tmp("gz", &bytes);
    assert_eq!(detect(tmp.path()).unwrap(), Kind::Gzip);
    let src = open(tmp.path()).unwrap();
    assert_eq!(src.kind, Kind::Gzip);
    assert_eq!(src.size, None);
}

fn xz_bytes(data: &[u8]) -> Vec<u8> {
    let mut enc = xz2::write::XzEncoder::new(Vec::new(), 6);
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

#[test]
fn detects_xz() {
    let bytes = xz_bytes(&sample_bytes());
    let tmp = write_tmp("xz", &bytes);
    assert_eq!(detect(tmp.path()).unwrap(), Kind::Xz);
    let src = open(tmp.path()).unwrap();
    assert_eq!(src.kind, Kind::Xz);
    assert_eq!(src.size, None);
}

#[test]
fn detects_zstd() {
    let bytes = zstd::stream::encode_all(&sample_bytes()[..], 3).unwrap();
    let tmp = write_tmp("zst", &bytes);
    assert_eq!(detect(tmp.path()).unwrap(), Kind::Zstd);
    let src = open(tmp.path()).unwrap();
    assert_eq!(src.kind, Kind::Zstd);
    assert_eq!(src.size, None);
}

#[test]
fn round_trip_raw() {
    let bytes = sample_bytes();
    let tmp = write_tmp("rt_raw", &bytes);
    assert_eq!(read_all(tmp.path()), bytes);
}

#[test]
fn round_trip_gzip() {
    let bytes = sample_bytes();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&bytes).unwrap();
    let tmp = write_tmp("rt_gz", &enc.finish().unwrap());
    assert_eq!(read_all(tmp.path()), bytes);
}

#[test]
fn round_trip_xz() {
    let bytes = sample_bytes();
    let compressed = xz_bytes(&bytes);
    let tmp = write_tmp("rt_xz", &compressed);
    assert_eq!(read_all(tmp.path()), bytes);
}

#[test]
fn round_trip_zstd() {
    let bytes = sample_bytes();
    let compressed = zstd::stream::encode_all(&bytes[..], 3).unwrap();
    let tmp = write_tmp("rt_zst", &compressed);
    assert_eq!(read_all(tmp.path()), bytes);
}

#[test]
fn open_missing_file_fails_with_context() {
    let tmp = TmpFile::new("missing");
    let err = match open(tmp.path()) {
        Ok(_) => panic!("expected error for missing file"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("failed to open"));
}
