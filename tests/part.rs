use safeusb::part::{read_table, Part, Scheme};
use std::io::Cursor;

const IMAGE_SIZE: usize = 1 << 20;

fn put(img: &mut [u8], off: usize, bytes: &[u8]) {
    img[off..off + bytes.len()].copy_from_slice(bytes);
}

fn put_mbr_entry(img: &mut [u8], slot: usize, typ: u8, start: u32, count: u32) {
    let off = 446 + 16 * slot;
    img[off + 4] = typ;
    put(img, off + 8, &start.to_le_bytes());
    put(img, off + 12, &count.to_le_bytes());
}

fn mbr_signature(img: &mut [u8]) {
    img[510] = 0x55;
    img[511] = 0xAA;
}

fn guid_bytes(text: &str) -> [u8; 16] {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    let raw: Vec<u8> = (0..16)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex"))
        .collect();
    let mut out = [0u8; 16];
    for k in 0..4 {
        out[k] = raw[3 - k];
    }
    for k in 0..2 {
        out[4 + k] = raw[5 - k];
    }
    for k in 0..2 {
        out[6 + k] = raw[7 - k];
    }
    out[8..16].copy_from_slice(&raw[8..16]);
    out
}

fn put_gpt_entry(img: &mut [u8], off: usize, type_guid: &str, first: u64, last: u64) {
    put(img, off, &guid_bytes(type_guid));
    put(img, off + 32, &first.to_le_bytes());
    put(img, off + 40, &last.to_le_bytes());
}

#[test]
fn zeros_have_no_scheme() {
    let mut cur = Cursor::new(vec![0u8; IMAGE_SIZE]);
    let table = read_table(&mut cur).expect("read");
    assert_eq!(table.scheme, Scheme::None);
    assert!(table.parts.is_empty());
}

#[test]
fn mbr_two_partitions() {
    let mut img = vec![0u8; IMAGE_SIZE];
    mbr_signature(&mut img);
    put_mbr_entry(&mut img, 0, 0x0C, 2048, 4096);
    put_mbr_entry(&mut img, 1, 0x83, 8192, 2048);
    let mut cur = Cursor::new(img);
    let table = read_table(&mut cur).expect("read");
    assert_eq!(table.scheme, Scheme::Mbr);
    assert_eq!(
        table.parts,
        vec![
            Part {
                number: 1,
                start_lba: 2048,
                sectors: 4096,
                kind: "fat32".to_string(),
            },
            Part {
                number: 2,
                start_lba: 8192,
                sectors: 2048,
                kind: "linux".to_string(),
            },
        ]
    );
}

#[test]
fn gpt_efi_and_linux() {
    let mut img = vec![0u8; IMAGE_SIZE];
    mbr_signature(&mut img);
    put_mbr_entry(&mut img, 0, 0xEE, 1, 2047);

    put(&mut img, 512, b"EFI PART");
    put(&mut img, 512 + 72, &2u64.to_le_bytes());
    put(&mut img, 512 + 80, &128u32.to_le_bytes());
    put(&mut img, 512 + 84, &128u32.to_le_bytes());

    put_gpt_entry(
        &mut img,
        1024,
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B",
        2048,
        4095,
    );
    put_gpt_entry(
        &mut img,
        1024 + 128,
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
        4096,
        8191,
    );

    let mut cur = Cursor::new(img);
    let table = read_table(&mut cur).expect("read");
    assert_eq!(table.scheme, Scheme::Gpt);
    assert_eq!(
        table.parts,
        vec![
            Part {
                number: 1,
                start_lba: 2048,
                sectors: 2048,
                kind: "efi system".to_string(),
            },
            Part {
                number: 2,
                start_lba: 4096,
                sectors: 4096,
                kind: "linux fs".to_string(),
            },
        ]
    );
}

#[test]
fn bad_gpt_header_is_error() {
    let mut img = vec![0u8; IMAGE_SIZE];
    mbr_signature(&mut img);
    put_mbr_entry(&mut img, 0, 0xEE, 1, 2047);
    put(&mut img, 512, b"EFI PART");
    put(&mut img, 512 + 72, &2u64.to_le_bytes());
    put(&mut img, 512 + 80, &128u32.to_le_bytes());
    put(&mut img, 512 + 84, &0u32.to_le_bytes());

    let mut cur = Cursor::new(img);
    assert!(read_table(&mut cur).is_err());
}

#[test]
fn truncated_image_is_error_not_panic() {
    let mut img = vec![0u8; 512];
    mbr_signature(&mut img);
    put_mbr_entry(&mut img, 0, 0xEE, 1, 2047);
    let mut cur = Cursor::new(img);
    assert!(read_table(&mut cur).is_err());
}
