use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

const BUF_SIZE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Sha256,
    Blake3,
}

impl FromStr for Algo {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "sha256" => Ok(Algo::Sha256),
            "blake3" => Ok(Algo::Blake3),
            _ => bail!("unknown algo {s}, use sha256 or blake3"),
        }
    }
}

fn stream_file<F: FnMut(&[u8])>(path: &Path, mut update: F) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if n == 0 {
            break;
        }
        update(&buf[..n]);
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    stream_file(path, |chunk| hasher.update(chunk))?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn blake3_file(path: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    stream_file(path, |chunk| {
        hasher.update(chunk);
    })?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn check_digest(path: &Path, expected: &str, algo: Algo) -> Result<()> {
    let actual = match algo {
        Algo::Sha256 => sha256_file(path)?,
        Algo::Blake3 => blake3_file(path)?,
    };
    if !expected.trim().eq_ignore_ascii_case(&actual) {
        bail!("digest mismatch, expected {expected} got {actual}");
    }
    Ok(())
}
