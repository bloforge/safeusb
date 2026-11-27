use super::Disk;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, VOLUME_DISK_EXTENTS};

struct Row {
    number: u32,
    size: u64,
    boot: bool,
    system_flag: bool,
    usb: bool,
    name: String,
}

struct Table {
    rows: Vec<Row>,
    sys_nums: Option<BTreeSet<u32>>,
}

pub fn list() -> Result<Vec<Disk>> {
    let t = run_query()?;
    if t.sys_nums.is_none() {
        eprintln!("warning: failed to map the system drive to a disk, SYSTEM column may be incomplete");
    }
    Ok(t.rows
         .iter()
         .map(|r| to_disk(r, t.sys_nums.as_ref()))
         .collect())
}

pub fn describe(path: &Path) -> Result<Disk> {
    let n = drive_num(path).with_context(|| {
         format!(
             "unsupported device path {}, use \\\\.\\PhysicalDriveN",
             path.display()
         )
     })?;
    let t = run_query()?;
    let row = t
        .rows
        .iter()
        .find(|r| r.number == n)
        .with_context(|| format!("{} is not in the Get-Disk output", path.display()))?;
    let sys = t
        .sys_nums
        .as_ref()
        .context("failed to map the system drive (Get-Partition failed), refusing to continue")?;
    let mut info = to_disk(row, Some(sys));
    info.path = path.to_path_buf();
    Ok(info)
}

pub fn raw_size(path: &Path) -> Result<u64> {
    let n = drive_num(path)
        .with_context(|| format!("unsupported device path {}", path.display()))?;
    let t = run_query()?;
    t.rows
        .iter()
        .find(|r| r.number == n)
        .map(|r| r.size)
        .with_context(|| format!("{} is not in the Get-Disk output", path.display()))
}

fn to_disk(row: &Row, sys: Option<&BTreeSet<u32>>) -> Disk {
    let is_system = row.boot || row.system_flag || sys.is_some_and(|s| s.contains(&row.number));
    Disk {
        path: PathBuf::from(format!(r"\\.\PhysicalDrive{}", row.number)),
        size: row.size,
        is_system,
        removable: row.usb,
        description: format!("{} (PhysicalDrive{})", row.name, row.number),
    }
}

fn drive_num(path: &Path) -> Option<u32> {
    let s = path.to_string_lossy().to_ascii_lowercase();
    s.strip_prefix(r"\\.\physicaldrive")?.parse().ok()
}

fn sys_letter() -> char {
    std::env::var("SystemDrive")
        .ok()
        .and_then(|s| s.chars().next())
        .filter(|c| c.is_ascii_alphabetic())
        .unwrap_or('C')
        .to_ascii_uppercase()
}

fn run_query() -> Result<Table> {
    let letter = sys_letter();
    let script = format!(
         "$ErrorActionPreference = 'Stop'; \
         Get-Disk | ForEach-Object {{ '{{0}}|{{1}}|{{2}}|{{3}}|{{4}}|{{5}}' -f $_.Number, $_.Size, [int][bool]$_.IsBoot, [int][bool]$_.IsSystem, $_.BusType, $_.FriendlyName }}; \
         try {{ Get-Partition -DriveLetter {letter} -ErrorAction Stop | ForEach-Object {{ 'SYS|{{0}}' -f $_.DiskNumber }} }} catch {{ 'SYSERR' }}"
     );
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script.as_str()])
        .output()
        .context("failed to run powershell.exe")?;
    if !out.status.success() {
        bail!(
             "Get-Disk failed: {}, this needs an elevated prompt",
             String::from_utf8_lossy(&out.stderr).trim()
         );
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

fn parse(text: &str) -> Result<Table> {
    let mut rows = Vec::new();
    let mut sys = BTreeSet::new();
    let mut sys_error = false;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line == "SYSERR" {
            sys_error = true;
        } else if let Some(n) = line.strip_prefix("SYS|") {
            sys.insert(
                 n.trim()
                     .parse::<u32>()
                     .with_context(|| format!("bad line: {line}"))?,
             );
        } else {
            let f: Vec<&str> = line.splitn(6, '|').collect();
            if f.len() != 6 {
                bail!("unexpected Get-Disk line: {line}");
            }
            rows.push(Row {
                 number: f[0]
                     .parse::<u32>()
                     .with_context(|| format!("bad line: {line}"))?,
                 size: f[1]
                     .parse::<u64>()
                     .with_context(|| format!("bad line: {line}"))?,
                 boot: f[2] == "1",
                 system_flag: f[3] == "1",
                 usb: f[4].trim().eq_ignore_ascii_case("usb"),
                 name: f[5].trim().to_string(),
             });
        }
    }
    let sys_nums = if sys_error || sys.is_empty() {
        None
    } else {
        Some(sys)
    };
    Ok(Table { rows, sys_nums })
}

pub fn lock_volumes(path: &Path) -> Result<super::VolumeLock> {
    let Some(n) = drive_num(path) else {
        return Ok(super::VolumeLock { handles: Vec::new() });
    };
    let mut lock = super::VolumeLock { handles: Vec::new() };
    for letter in 'A'..='Z' {
        let vol = format!(r"\\.\{letter}:");
        if volume_disk(&vol) != Some(n) {
            continue;
        }
        let h = open_handle(&vol, true)
            .with_context(|| format!("failed to open volume {letter}:"))?;
        lock.handles.push(h);
        lock_and_dismount(h, letter)?;
    }
    Ok(lock)
}

fn open_handle(path: &str, write: bool) -> Result<HANDLE> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let access = if write { GENERIC_READ | GENERIC_WRITE } else { 0 };
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        bail!("failed to open {path}: {}", std::io::Error::last_os_error());
    }
    Ok(h)
}

fn volume_disk(path: &str) -> Option<u32> {
    let h = open_handle(path, false).ok()?;
    let mut ext: VOLUME_DISK_EXTENTS = unsafe { std::mem::zeroed() };
    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            std::ptr::null(),
            0,
            &mut ext as *mut VOLUME_DISK_EXTENTS as *mut std::ffi::c_void,
            std::mem::size_of::<VOLUME_DISK_EXTENTS>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(h) };
    if ok == 0 || ext.NumberOfDiskExtents == 0 {
        None
    } else {
        Some(ext.Extents[0].DiskNumber)
    }
}

fn lock_and_dismount(h: HANDLE, letter: char) -> Result<()> {
    let mut returned: u32 = 0;
    let locked = unsafe {
        DeviceIoControl(
            h,
            FSCTL_LOCK_VOLUME,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if locked == 0 {
        bail!(
            "failed to lock volume {letter}: close anything using it (explorer, antivirus) and retry"
        );
    }
    let dismounted = unsafe {
        DeviceIoControl(
            h,
            FSCTL_DISMOUNT_VOLUME,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if dismounted == 0 {
        bail!("failed to dismount volume {letter}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_paths() {
        assert_eq!(drive_num(Path::new(r"\\.\PhysicalDrive1")), Some(1));
        assert_eq!(drive_num(Path::new(r"\\.\physicaldrive12")), Some(12));
        assert_eq!(drive_num(Path::new(r"\\.\C:")), None);
        assert_eq!(drive_num(Path::new(r"C:\images\x.img")), None);
    }

    #[test]
    fn get_disk_parse_flags_system() {
        let t = parse(
             "0|500107862016|1|1|NVMe|Samsung SSD\r\n1|32017047552|0|0|USB|SanDisk Cruzer\r\nSYS|0\r\n",
         )
        .unwrap();
        assert_eq!(t.rows.len(), 2);
        let info = to_disk(&t.rows[0], t.sys_nums.as_ref());
        assert!(info.is_system);
        let usb = to_disk(&t.rows[1], t.sys_nums.as_ref());
        assert!(!usb.is_system);
        assert_eq!(usb.size, 32017047552);
        assert_eq!(usb.path, PathBuf::from(r"\\.\PhysicalDrive1"));
    }

    #[test]
    fn usb_bus_is_removable() {
        let t = parse("0|500107862016|1|1|NVMe|Samsung SSD\r\n1|32017047552|0|0|USB|SanDisk Cruzer\r\n")
            .unwrap();
        assert!(!to_disk(&t.rows[0], None).removable);
        assert!(to_disk(&t.rows[1], None).removable);
    }

    #[test]
    fn name_keeps_pipes() {
        let t = parse("3|1000|0|0|USB|Weird|Name\n").unwrap();
        assert_eq!(t.rows[0].name, "Weird|Name");
        assert!(t.rows[0].usb);
    }

    #[test]
    fn no_sys_drive_is_none() {
        let t = parse("1|100|0|0|SATA|Disk\nSYSERR\n").unwrap();
        assert!(t.sys_nums.is_none());
    }
}
