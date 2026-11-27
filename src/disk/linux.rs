use super::Disk;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const SYSFS: &str = "/sys";

pub fn list() -> Result<Vec<Disk>> {
    let sys_disks = system_disks().unwrap_or_else(|e| {
         eprintln!("warning: failed to find the system disk ({e:#}), SYSTEM column may be wrong");
         BTreeSet::new()
     });
    let block_dir = Path::new(SYSFS).join("block");
    let mut names: Vec<String> = fs::read_dir(&block_dir)
        .with_context(|| format!("failed to read {}", block_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !skip_name(n))
        .collect();
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let Some(sectors) = read_number(&block_dir.join(&name).join("size")) else {
            continue;
        };
        out.push(Disk {
             path: PathBuf::from("/dev").join(&name),
             size: sectors * 512,
             is_system: sys_disks.contains(&name),
             removable: is_removable(&name),
             description: disk_label(&name),
         });
    }
    Ok(out)
}

pub fn lock_volumes(_path: &Path) -> Result<super::VolumeLock> {
    Ok(super::VolumeLock)
}

pub fn raw_size(path: &Path) -> Result<u64> {
    let real = resolve(path)?;
    read_sysfs_size(block_device_name(&real)?)
}

pub fn describe(path: &Path) -> Result<Disk> {
    let real = resolve(path)?;
    let name = block_device_name(&real)?;
    let disks = disks_of(Path::new(SYSFS), name)?;
    let sys = system_disks()?;
    let is_system = disks.iter().any(|d| sys.contains(d));
    let disk = disks
        .iter()
        .next()
        .cloned()
        .unwrap_or_else(|| name.to_string());
    Ok(Disk {
         path: path.to_path_buf(),
         size: read_sysfs_size(name)?,
         is_system,
         removable: is_removable(&disk),
         description: disk_label(&disk),
     })
}

fn is_removable(disk: &str) -> bool {
    let base = Path::new(SYSFS).join("block").join(disk);
    if read_str(&base.join("removable")).as_deref() == Some("1") {
        return true;
    }
    fs::canonicalize(base.join("device"))
        .map(|p| p.to_string_lossy().contains("/usb"))
        .unwrap_or(false)
}

pub fn system_disks() -> Result<BTreeSet<String>> {
    let root_dev = fs::metadata("/").context("failed to stat /")?.dev();
    sys_disks_in(Path::new(SYSFS), root_dev)
}

fn sys_disks_in(sys: &Path, dev: u64) -> Result<BTreeSet<String>> {
    let link = sys
        .join("dev")
        .join("block")
        .join(format!("{}:{}", major(dev), minor(dev)));
    let canon = fs::canonicalize(&link).with_context(|| {
         format!(
             "no sysfs entry for the root fs device ({})",
             link.display()
         )
     })?;
    let mut disks = BTreeSet::new();
    collect(&canon, &mut disks, 0)?;
    if disks.is_empty() {
        bail!("failed to map root fs to a disk");
    }
    Ok(disks)
}

fn disks_of(sys: &Path, name: &str) -> Result<BTreeSet<String>> {
    let canon = fs::canonicalize(sys.join("class").join("block").join(name))
        .with_context(|| format!("{name} is not a block device known to sysfs"))?;
    let mut disks = BTreeSet::new();
    collect(&canon, &mut disks, 0)?;
    Ok(disks)
}

fn collect(p: &Path, out: &mut BTreeSet<String>, depth: u32) -> Result<()> {
    if depth > 8 {
        bail!("device stack too deep at {}", p.display());
    }
    if p.join("partition").exists() {
        let parent = p
            .parent()
            .and_then(|x| x.file_name())
            .and_then(|n| n.to_str())
            .context("unexpected sysfs layout for partition")?;
        out.insert(parent.to_string());
        return Ok(());
    }
    let slaves = p.join("slaves");
    if slaves.is_dir() {
        let mut any = false;
        for entry in fs::read_dir(&slaves)? {
            let entry = entry?;
            any = true;
            let canon = fs::canonicalize(entry.path())?;
            collect(&canon, out, depth + 1)?;
        }
        if any {
            return Ok(());
        }
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .context("unexpected sysfs layout")?;
    out.insert(name.to_string());
    Ok(())
}

fn resolve(path: &Path) -> Result<PathBuf> {
    let real = fs::canonicalize(path)
        .with_context(|| format!("failed to open {}: no such device", path.display()))?;
    if !real.starts_with("/dev") {
        bail!("{} is not a device under /dev", path.display());
    }
    Ok(real)
}

fn block_device_name(real: &Path) -> Result<&str> {
    real.file_name()
        .and_then(|n| n.to_str())
        .context("bad device path")
}

fn read_sysfs_size(name: &str) -> Result<u64> {
    let sectors = read_number(
         &Path::new(SYSFS)
             .join("class")
             .join("block")
             .join(name)
             .join("size"),
     )
    .with_context(|| format!("no sysfs size for {name}"))?;
    Ok(sectors * 512)
}

fn disk_label(disk: &str) -> String {
    let base = Path::new(SYSFS).join("block").join(disk).join("device");
    let vendor = read_str(&base.join("vendor"));
    let model = read_str(&base.join("model"));
    let parts: Vec<String> = [vendor, model]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        format!("block device {disk}")
    } else {
        format!("{} ({disk})", parts.join(" "))
    }
}

fn skip_name(name: &str) -> bool {
    ["loop", "ram", "zram", "dm-", "sr"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}

fn minor(dev: u64) -> u64 {
    (dev & 0xff) | ((dev >> 12) & !0xff)
}

fn read_str(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_number(p: &Path) -> Option<u64> {
    read_str(p)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn partition_maps_to_parent() {
        let root = std::env::temp_dir().join(format!("safeusb-sysfs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sda2 = root.join("devices/pci0/block/sda/sda2");
        let sdb = root.join("devices/pci0/block/sdb");
        fs::create_dir_all(&sda2).unwrap();
        fs::create_dir_all(&sdb).unwrap();
        fs::write(sda2.join("partition"), "2\n").unwrap();
        fs::create_dir_all(root.join("dev/block")).unwrap();
        fs::create_dir_all(root.join("class/block")).unwrap();
        symlink(&sda2, root.join("dev/block/8:2")).unwrap();
        symlink(&sda2, root.join("class/block/sda2")).unwrap();
        symlink(&sdb, root.join("class/block/sdb")).unwrap();

        let system = sys_disks_in(&root, (8 << 8) | 2).unwrap();
        assert_eq!(system, BTreeSet::from(["sda".to_string()]));
        assert_eq!(
             disks_of(&root, "sda2").unwrap(),
             BTreeSet::from(["sda".to_string()])
         );
        assert_eq!(
             disks_of(&root, "sdb").unwrap(),
             BTreeSet::from(["sdb".to_string()])
         );

        let _ = fs::remove_dir_all(&root);
    }
}
