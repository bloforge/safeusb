use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Seek, SeekFrom};

const SECTOR: u64 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    None,
    Mbr,
    Gpt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub number: u32,
    pub start_lba: u64,
    pub sectors: u64,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub scheme: Scheme,
    pub parts: Vec<Part>,
}

pub fn read_table<R: Read + Seek>(r: &mut R) -> Result<Table> {
    let mbr = read_sector(r, 0)?;
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return Ok(Table {
            scheme: Scheme::None,
            parts: vec![],
        });
    }

    let mut parts = Vec::new();
    let mut protective = false;
    for i in 0..4usize {
        let off = 446 + 16 * i;
        let typ = field::<1>(&mbr, off + 4)?[0];
        let start = le32(&mbr, off + 8)? as u64;
        let count = le32(&mbr, off + 12)? as u64;
        if typ == 0xEE {
            protective = true;
        }
        if typ == 0 || count == 0 {
            continue;
        }
        parts.push(Part {
            number: (i + 1) as u32,
            start_lba: start,
            sectors: count,
            kind: mbr_kind(typ).to_string(),
        });
    }

    if protective {
        let hdr = read_sector(r, 1)?;
        if field::<8>(&hdr, 0)? == *b"EFI PART" {
            return read_gpt(r, &hdr);
        }
    }

    Ok(Table {
        scheme: Scheme::Mbr,
        parts,
    })
}

fn read_gpt<R: Read + Seek>(r: &mut R, hdr: &[u8; 512]) -> Result<Table> {
    let entries_lba = le64(hdr, 72)?;
    let count = le32(hdr, 80)? as u64;
    let size = le32(hdr, 84)? as u64;

    if entries_lba == 0 {
        bail!("bad gpt header entries lba");
    }
    if !(128..=4096).contains(&size) {
        bail!("bad gpt header entry size");
    }
    if count == 0 || count > 4096 {
        bail!("bad gpt header entry count");
    }

    let pos = entries_lba
        .checked_mul(SECTOR)
        .ok_or_else(|| anyhow!("bad gpt header entries lba"))?;
    r.seek(SeekFrom::Start(pos))
        .context("failed to seek to gpt entries")?;
    let mut buf = vec![0u8; (count * size) as usize];
    r.read_exact(&mut buf)
        .context("failed to read gpt entries")?;

    let mut parts = Vec::new();
    for e in buf.chunks_exact(size as usize) {
        let guid = field::<16>(e, 0)?;
        if guid == [0u8; 16] {
            continue;
        }
        let first = le64(e, 32)?;
        let last = le64(e, 40)?;
        let sectors = last
            .checked_sub(first)
            .and_then(|d| d.checked_add(1))
            .ok_or_else(|| anyhow!("bad gpt partition range"))?;
        parts.push(Part {
            number: parts.len() as u32 + 1,
            start_lba: first,
            sectors,
            kind: gpt_kind(&guid_text(&guid)).to_string(),
        });
    }

    Ok(Table {
        scheme: Scheme::Gpt,
        parts,
    })
}

fn read_sector<R: Read + Seek>(r: &mut R, lba: u64) -> Result<[u8; 512]> {
    let pos = lba
        .checked_mul(SECTOR)
        .ok_or_else(|| anyhow!("sector offset overflow"))?;
    r.seek(SeekFrom::Start(pos))
        .with_context(|| format!("failed to seek to sector {lba}"))?;
    let mut buf = [0u8; 512];
    r.read_exact(&mut buf)
        .with_context(|| format!("failed to read sector {lba}"))?;
    Ok(buf)
}

fn field<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N]> {
    let s = b
        .get(off..off + N)
        .ok_or_else(|| anyhow!("failed to read field at offset {off}"))?;
    let mut out = [0u8; N];
    out.copy_from_slice(s);
    Ok(out)
}

fn le32(b: &[u8], off: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(field::<4>(b, off)?))
}

fn le64(b: &[u8], off: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(field::<8>(b, off)?))
}

fn guid_text(b: &[u8; 16]) -> String {
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        d1, d2, d3, b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn gpt_kind(guid: &str) -> &'static str {
    match guid {
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "efi system",
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "basic data",
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => "linux fs",
        "E3C9E316-0B5C-4DB8-817D-F92DF00215AE" => "ms reserved",
        _ => "unknown",
    }
}

fn mbr_kind(typ: u8) -> &'static str {
    match typ {
        0x0B | 0x0C => "fat32",
        0x0E | 0x06 | 0x04 => "fat16",
        0x07 => "ntfs/exfat",
        0x83 => "linux",
        0xEF => "efi system",
        0xEE => "protective",
        _ => "unknown",
    }
}
