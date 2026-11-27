use safeusb::disk::{check_target, check_target_force};
use std::fs;
use std::path::{Path, PathBuf};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("safeusb-guard-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn new_file_ok() {
    let dir = tmp_dir("new");
    let target = dir.join("restored.img");
    let info = check_target(&target, 4 << 20).expect("a new path should be fine");
    assert!(!info.is_system);
    assert!(!target.exists(), "check_target should not create the file");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn existing_file_refused_without_force() {
    let dir = tmp_dir("existing");
    let target = dir.join("keep.img");
    fs::write(&target, b"keep me").unwrap();
    let err = check_target(&target, 1).unwrap_err();
    assert!(err.to_string().contains("--force"), "got: {err}");
    assert_eq!(fs::read(&target).unwrap(), b"keep me");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn existing_file_ok_with_force() {
    let dir = tmp_dir("force");
    let target = dir.join("overwrite.img");
    fs::write(&target, b"old").unwrap();
    assert!(check_target_force(&target, 1, true).is_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dir_refused_even_with_force() {
    let dir = tmp_dir("dir");
    assert!(check_target_force(&dir, 1, true).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_raw_refused() {
    let path: &Path = if cfg!(windows) {
        Path::new(r"\\.\PhysicalDrive999")
    } else {
        Path::new("/dev/safeusb-no-such-device")
    };
    assert!(check_target(path, 1).is_err());
}
