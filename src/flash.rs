use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::sync::mpsc::{self, Receiver};

const QUEUE_DEPTH: usize = 4;

#[derive(Debug, Clone)]
pub struct FlashOpts {
    pub block_size: usize,
    pub verify: bool,
}

impl Default for FlashOpts {
    fn default() -> Self {
        Self {
            block_size: 4 << 20,
            verify: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Write,
    Verify,
}

#[derive(Debug, Clone, Default)]
pub struct FlashReport {
    pub written: u64,
    pub sha256: String,
    pub verified: bool,
}

pub fn flash(
    src: &mut dyn Read,
    target: &mut File,
    opts: &FlashOpts,
    progress: &mut dyn FnMut(Phase, u64, u64),
) -> Result<FlashReport> {
    let block_size = opts.block_size.max(1);
    let mut hasher = Sha256::new();
    let mut read_total: u64 = 0;
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE_DEPTH);

    let written = std::thread::scope(|s| -> Result<u64> {
        let tgt = &mut *target;
        let writer = s.spawn(move || write_blocks(rx, tgt));
        let mut read_err: Option<anyhow::Error> = None;
        loop {
            let mut buf = vec![0u8; block_size];
            let n = match read_some(src, &mut buf) {
                Ok(n) => n,
                Err(e) => {
                    read_err = Some(anyhow!(e).context("failed to read source"));
                    break;
                }
            };
            if n == 0 {
                break;
            }
            buf.truncate(n);
            hasher.update(&buf);
            read_total += n as u64;
            if tx.send(buf).is_err() {
                break;
            }
            progress(Phase::Write, read_total, 0);
        }
        drop(tx);
        let written = writer
            .join()
            .map_err(|_| anyhow!("writer thread panicked"))??;
        if let Some(e) = read_err {
            return Err(e);
        }
        Ok(written)
    })?;

    if written == 0 {
        bail!("source is empty");
    }

    progress(Phase::Write, written, written);
    let sha256 = to_hex(&hasher.finalize());

    let verified = if opts.verify {
        verify_target(target, block_size, written, &sha256, progress)?;
        true
    } else {
        false
    };

    Ok(FlashReport {
        written,
        sha256,
        verified,
    })
}

fn write_blocks(rx: Receiver<Vec<u8>>, target: &mut File) -> Result<u64> {
    let mut written: u64 = 0;
    for block in rx {
        target
            .write_all(&block)
            .with_context(|| format!("failed to write to target at byte {written}"))?;
        written += block.len() as u64;
    }
    target.sync_all().context("failed to sync target")?;
    Ok(written)
}

fn verify_target(
    target: &mut File,
    block_size: usize,
    written: u64,
    expected: &str,
    progress: &mut dyn FnMut(Phase, u64, u64),
) -> Result<()> {
    target
        .seek(SeekFrom::Start(0))
        .context("failed to seek target for verify")?;

    let mut buf = vec![0u8; block_size];
    let mut hasher = Sha256::new();
    let mut remaining = written;
    let mut done: u64 = 0;
    progress(Phase::Verify, 0, written);
    while remaining > 0 {
        let want = remaining.min(buf.len() as u64) as usize;
        let chunk = &mut buf[..want];
        target
            .read_exact(chunk)
            .context("failed to read back target for verify")?;
        hasher.update(&*chunk);
        remaining -= want as u64;
        done += want as u64;
        progress(Phase::Verify, done, written);
    }

    if to_hex(&hasher.finalize()) != expected {
        bail!("verify failed, target does not match source");
    }
    Ok(())
}

fn read_some(src: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match src.read(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
