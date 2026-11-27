use safeusb::flash::{flash, FlashOpts};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("safeusb_flash_{}_{}.bin", std::process::id(), name))
}

fn sample_bytes(len: usize) -> Vec<u8> {
    let mut state: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn open_target(path: &PathBuf) -> std::fs::File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .expect("open temp target")
}

#[test]
fn round_trip_matches_source() {
    let data = sample_bytes(3 * (1 << 20) + 12_345);
    let path = temp_path("round_trip");
    let mut target = open_target(&path);

    let mut last = (u64::MAX, u64::MAX);
    let mut src: &[u8] = &data;
    let report = flash(&mut src, &mut target, &FlashOpts::default(), &mut |_, done, total| {
        last = (done, total);
    })
    .expect("flash succeeds");

    assert_eq!(report.written, data.len() as u64);
    assert_eq!(report.sha256, hex_sha256(&data));
    assert!(report.verified);
    assert_eq!(last, (data.len() as u64, data.len() as u64));
    assert_eq!(fs::read(&path).expect("read target"), data);

    let _ = fs::remove_file(&path);
}

#[test]
fn verify_off_skips_read_back() {
    let data = sample_bytes(64 * 1024);
    let path = temp_path("verify_off");
    let mut target = open_target(&path);

    let opts = FlashOpts {
        block_size: 8192,
        verify: false,
    };
    let mut src: &[u8] = &data;
    let report = flash(&mut src, &mut target, &opts, &mut |_, _, _| {}).expect("flash succeeds");

    assert_eq!(report.written, data.len() as u64);
    assert_eq!(report.sha256, hex_sha256(&data));
    assert!(!report.verified);
    assert_eq!(fs::read(&path).expect("read target"), data);

    let _ = fs::remove_file(&path);
}

#[test]
fn block_size_not_dividing_length() {
    let data = sample_bytes(100_000);
    let path = temp_path("odd_block");
    let mut target = open_target(&path);

    let opts = FlashOpts {
        block_size: 4099,
        verify: true,
    };
    let mut src: &[u8] = &data;
    let report = flash(&mut src, &mut target, &opts, &mut |_, _, _| {}).expect("flash succeeds");

    assert_eq!(report.written, 100_000);
    assert_eq!(report.sha256, hex_sha256(&data));
    assert!(report.verified);
    assert_eq!(fs::read(&path).expect("read target"), data);

    let _ = fs::remove_file(&path);
}

#[test]
fn empty_source_is_error() {
    let path = temp_path("empty");
    let mut target = open_target(&path);

    let mut src: &[u8] = &[];
    let err = flash(&mut src, &mut target, &FlashOpts::default(), &mut |_, _, _| {})
        .expect_err("empty source must fail");

    assert_eq!(err.to_string(), "source is empty");

    let _ = fs::remove_file(&path);
}
