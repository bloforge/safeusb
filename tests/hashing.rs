use safeusb::hashing::{blake3_file, check_digest, sha256_file, Algo};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "safeusb_hashing_{}_{}",
        std::process::id(),
        name
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[test]
fn sha256_of_abc() {
    let path = temp_file("sha_abc", b"abc");
    let digest = sha256_file(&path).unwrap();
    remove(&path);
    assert_eq!(digest, ABC_SHA256);
}

#[test]
fn blake3_of_abc() {
    let path = temp_file("blake_abc", b"abc");
    let digest = blake3_file(&path).unwrap();
    remove(&path);
    assert_eq!(digest, blake3::hash(b"abc").to_hex().to_string());
}

#[test]
fn digests_match_whole_file_across_buffers() {
    let data: Vec<u8> = (0..(3 * (1 << 20) + 12345)).map(|i| (i % 251) as u8).collect();
    let path = temp_file("multi_buf", &data);
    let sha = sha256_file(&path).unwrap();
    let blake = blake3_file(&path).unwrap();
    remove(&path);

    use sha2::Digest;
    let expected_sha: String = sha2::Sha256::digest(&data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(sha, expected_sha);
    assert_eq!(blake, blake3::hash(&data).to_hex().to_string());
}

#[test]
fn empty_file_has_known_sha256() {
    let path = temp_file("empty", b"");
    let digest = sha256_file(&path).unwrap();
    remove(&path);
    assert_eq!(
        digest,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn missing_file_reports_open_error() {
    let path = std::env::temp_dir().join(format!(
        "safeusb_hashing_{}_does_not_exist",
        std::process::id()
    ));
    let err = sha256_file(&path).unwrap_err();
    assert!(format!("{err:#}").contains("failed to open"));
    let err = blake3_file(&path).unwrap_err();
    assert!(format!("{err:#}").contains("failed to open"));
}

#[test]
fn check_digest_accepts_uppercase_and_whitespace() {
    let path = temp_file("check_ok", b"abc");
    let upper = ABC_SHA256.to_uppercase();
    let padded = format!("  {upper}\n");
    let result = check_digest(&path, &padded, Algo::Sha256);
    remove(&path);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn check_digest_rejects_wrong_digest() {
    let path = temp_file("check_bad", b"abc");
    let wrong = "0".repeat(64);
    let err = check_digest(&path, &wrong, Algo::Sha256).unwrap_err();
    remove(&path);
    let msg = format!("{err:#}");
    assert!(msg.starts_with("digest mismatch, expected"), "{msg}");
    assert!(msg.contains(ABC_SHA256), "{msg}");
}

#[test]
fn check_digest_blake3() {
    let path = temp_file("check_blake", b"abc");
    let good = blake3::hash(b"abc").to_hex().to_string().to_uppercase();
    let ok = check_digest(&path, &good, Algo::Blake3);
    let bad = check_digest(&path, ABC_SHA256, Algo::Blake3);
    remove(&path);
    assert!(ok.is_ok());
    assert!(bad.is_err());
}

#[test]
fn algo_from_str() {
    assert_eq!(Algo::from_str("sha256").unwrap(), Algo::Sha256);
    assert_eq!(Algo::from_str("blake3").unwrap(), Algo::Blake3);
    let err = Algo::from_str("md5").unwrap_err();
    assert_eq!(err.to_string(), "unknown algo md5, use sha256 or blake3");
}
