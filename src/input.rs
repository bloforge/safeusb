use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use xz2::read::XzDecoder;
use zstd::stream::read::Decoder as ZstdDecoder;

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const XZ_MAGIC: [u8; 6] = [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00];
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const ISO_OFFSET: u64 = 32769;
const ISO_SIGNATURE: &[u8; 5] = b"CD001";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Iso,
    Gzip,
    Xz,
    Zstd,
}

pub struct Source {
    pub kind: Kind,
    pub size: Option<u64>,
    pub reader: Box<dyn Read>,
}

fn fill(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = file.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

pub fn detect(path: &Path) -> Result<Kind> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

    let mut head = [0u8; 6];
    let n = fill(&mut file, &mut head)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let head = &head[..n];

    if head.starts_with(&GZIP_MAGIC) {
        return Ok(Kind::Gzip);
    }
    if head.starts_with(&XZ_MAGIC) {
        return Ok(Kind::Xz);
    }
    if head.starts_with(&ZSTD_MAGIC) {
        return Ok(Kind::Zstd);
    }

    file.seek(SeekFrom::Start(ISO_OFFSET))
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut sig = [0u8; 5];
    let n = fill(&mut file, &mut sig)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if n == sig.len() && &sig == ISO_SIGNATURE {
        return Ok(Kind::Iso);
    }

    Ok(Kind::Raw)
}

pub fn open(path: &Path) -> Result<Source> {
    let kind = detect(path)?;
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

    let source = match kind {
        Kind::Raw | Kind::Iso => {
            let len = file
                .metadata()
                .with_context(|| format!("failed to open {}", path.display()))?
                .len();
            Source {
                kind,
                size: Some(len),
                reader: Box::new(BufReader::new(file)),
            }
        }
        Kind::Gzip => Source {
            kind,
            size: None,
            reader: Box::new(GzDecoder::new(BufReader::new(file))),
        },
        Kind::Xz => Source {
            kind,
            size: None,
            reader: Box::new(XzDecoder::new(BufReader::new(file))),
        },
        Kind::Zstd => {
            let decoder = ZstdDecoder::new(BufReader::new(file))
                .with_context(|| format!("failed to open {}", path.display()))?;
            Source {
                kind,
                size: None,
                reader: Box::new(decoder),
            }
        }
    };

    Ok(source)
}
