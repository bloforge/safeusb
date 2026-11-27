use anyhow::{anyhow, bail, Context};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as plat;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as plat;

#[cfg(not(any(target_os = "linux", windows)))]
mod plat {
    use super::Disk;
    use std::path::Path;

    fn nope<T>() -> anyhow::Result<T> {
        anyhow::bail!("raw disk access only works on linux and windows for now")
    }
    pub fn list() -> anyhow::Result<Vec<Disk>> {
        nope()
    }
    pub fn describe(_path: &Path) -> anyhow::Result<Disk> {
        nope()
    }
    pub fn raw_size(_path: &Path) -> anyhow::Result<u64> {
        nope()
    }
    pub fn lock_volumes(_path: &Path) -> anyhow::Result<super::VolumeLock> {
        Ok(super::VolumeLock)
    }
}

#[derive(Debug, Clone)]
pub struct Disk {
    pub path: PathBuf,
    pub size: u64,
    pub is_system: bool,
    pub removable: bool,
    pub description: String,
}

pub fn list() -> anyhow::Result<Vec<Disk>> {
    plat::list()
}

#[cfg(windows)]
pub struct VolumeLock {
    handles: Vec<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl Drop for VolumeLock {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(h) };
        }
    }
}

#[cfg(not(windows))]
pub struct VolumeLock;

pub fn lock_volumes(path: &Path) -> anyhow::Result<VolumeLock> {
    plat::lock_volumes(path)
}

pub fn open_src(path: &Path) -> anyhow::Result<(File, u64)> {
    let meta = fs::metadata(path).ok();
    let raw = is_raw(path, meta.as_ref());
    let mut file = File::open(path).map_err(|e| open_err(e, path, raw))?;
    let size = if raw {
        match plat::raw_size(path) {
            Ok(n) if n > 0 => n,
            _ => file
                .seek(SeekFrom::End(0))
                .with_context(|| format!("failed to get size of {}", path.display()))?,
        }
    } else {
        file.metadata()
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len()
    };
    file.seek(SeekFrom::Start(0))
        .context("failed to rewind source")?;
    if size == 0 {
        bail!("{} has size 0, nothing to back up", path.display());
    }
    Ok((file, size))
}

pub fn check_target(path: &Path, need: u64) -> anyhow::Result<Disk> {
    check_target_force(path, need, false)
}

pub fn check_target_force(path: &Path, need: u64, force: bool) -> anyhow::Result<Disk> {
    let meta = fs::metadata(path).ok();
    if is_raw(path, meta.as_ref()) {
        let dev = raw_info(path)?;
        check_dev(&dev, need)?;
        return Ok(dev);
    }
    check_file(path, meta.as_ref(), force)?;
    Ok(Disk {
         path: path.to_path_buf(),
         size: meta.map_or(0, |m| m.len()),
         is_system: false,
         removable: false,
         description: "regular file".to_string(),
     })
}

pub fn open_target(path: &Path) -> anyhow::Result<File> {
    let meta = fs::metadata(path).ok();
    if is_raw(path, meta.as_ref()) {
        let dev = raw_info(path)?;
        if dev.is_system {
            bail!("cannot open {}: system disk", path.display());
        }
        return OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| open_err(e, path, true));
    }
    if meta.as_ref().is_some_and(|m| m.is_dir()) {
        bail!("{} is a directory", path.display());
    }
    OpenOptions::new()
        .read(true)
            .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| open_err(e, path, false))
}

fn raw_info(path: &Path) -> anyhow::Result<Disk> {
    let mut dev = plat::describe(path).with_context(|| {
         format!(
             "cannot verify {} is safe to write, refusing",
             path.display()
         )
     })?;
    dev.path = path.to_path_buf();
    Ok(dev)
}

fn check_dev(dev: &Disk, need: u64) -> anyhow::Result<()> {
    if dev.is_system {
        bail!(
             "refusing {}: system disk ({})",
             dev.path.display(),
             dev.description
         );
    }
    if dev.size < need {
        bail!(
             "refusing {}: device is {} bytes, image needs {}",
             dev.path.display(),
             dev.size,
             need
         );
    }
    Ok(())
}

fn check_file(path: &Path, meta: Option<&Metadata>, force: bool) -> anyhow::Result<()> {
    let Some(m) = meta else {
        return Ok(());
    };
    if m.is_dir() {
        bail!("refusing {}: is a directory", path.display());
    }
    if !m.is_file() {
        bail!(
             "refusing {}: not a plain file or raw device",
             path.display()
         );
    }
    if !force {
        bail!(
             "refusing {}: file exists, restore would overwrite it (use --force)",
             path.display()
         );
    }
    Ok(())
}

fn is_raw(path: &Path, meta: Option<&Metadata>) -> bool {
    if cfg!(windows) {
        return path.to_string_lossy().starts_with(r"\\.\");
    }
    path.starts_with("/dev") || meta.is_some_and(is_block)
}

#[cfg(unix)]
fn is_block(m: &Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    m.file_type().is_block_device()
}

#[cfg(not(unix))]
fn is_block(_m: &Metadata) -> bool {
    false
}

fn open_err(err: io::Error, path: &Path, raw: bool) -> anyhow::Error {
    if raw && err.kind() == io::ErrorKind::PermissionDenied {
        return anyhow!(err).context(format!(
             "failed to open {}: raw disk access needs admin (windows) or root (linux), run elevated or use sudo",
             path.display()
         ));
    }
    anyhow!(err).context(format!("failed to open {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_disk(is_system: bool, size: u64) -> Disk {
        Disk {
            path: PathBuf::from("FAKE-DISK"),
            size,
            is_system,
            removable: false,
            description: "fake test disk".to_string(),
        }
    }

    fn fake_removable(size: u64) -> Disk {
        Disk {
            path: PathBuf::from("FAKE-USB"),
            size,
            is_system: false,
            removable: true,
            description: "fake usb disk".to_string(),
        }
    }

    #[test]
    fn removable_disk_passes_check() {
        let dev = fake_removable(2 << 30);
        assert!(dev.removable);
        assert!(check_dev(&dev, 1 << 30).is_ok());
    }

    #[test]
    fn system_disk_refused() {
        let err = check_dev(&fake_disk(true, u64::MAX), 1).unwrap_err();
        assert!(err.to_string().contains("system disk"), "got: {err}");
    }

    #[test]
    fn too_small_refused() {
        let err = check_dev(&fake_disk(false, 1 << 30), 2 << 30).unwrap_err();
        assert!(err.to_string().contains("needs"), "got: {err}");
    }

    #[test]
    fn exact_size_ok() {
        assert!(check_dev(&fake_disk(false, 2 << 30), 2 << 30).is_ok());
    }
}
