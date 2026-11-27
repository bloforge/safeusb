# SafeUSB

SafeUSB is a disk backup, restore, and USB flashing tool written in Rust. It runs on Windows and Linux.

It backs up disks into compressed archives, restores them, and writes ISO and img files to USB sticks. After a flash, it reads the stick back and checks it.

## what it is

- back up a disk or image into a single archive file
- restore that archive to a disk
- verify an archive without restoring it
- flash an ISO or img (plain or compressed) to a USB stick
- inspect MBR and GPT partition tables

Archives are chunk-deduplicated, compressed with zstd, and checked with blake3 hashes.

## why not rufus

Rufus is a good tool, but it only runs on Windows. SafeUSB is meant to cover the same job on Linux too, and to be more careful about writes:

- runs on Windows and Linux
- refuses to write to the system disk
- makes you type the target path before anything is written
- reads every write back and checks it
- reads .gz, .xz, and .zst images without you unpacking them first

## commands

### list-devices

Shows the disks on the machine.

```
safeusb list-devices
```

Each row has the path, size, a SYSTEM flag, and a description. Check the SYSTEM column before you pick a target.

### backup

Reads a disk or image and writes an archive.

```
safeusb backup /dev/sdb --output ./disk.safeusb
safeusb backup \\.\PhysicalDrive2 --output D:\backups
```

If `--output` is a folder, SafeUSB names the archive after the source and adds `.safeusb`.

Options:

- `--zstd-level N`: compression level, 1 to 22. Default 9.
- `--chunk-size-mib N`: chunk size in MiB, 1 to 1024. Default 1.
- `--long-window-log N`: zstd long window size, 10 to 31. Default 31.

### restore

Writes an archive back to a disk.

```
safeusb restore ./disk.safeusb /dev/sdb
```

Before any write, SafeUSB shows the target and asks you to type its path exactly. If the text does not match, nothing is written.

`--force` skips the normal target checks. Only use it when you know why the check is wrong.

### verify

Checks an archive's hashes without writing anything.

```
safeusb verify ./disk.safeusb
```

### info

Prints what is stored in an archive: image size, chunk size, and zstd level.

```
safeusb info ./disk.safeusb
```

### flash

Writes an ISO, img, or compressed image (.gz, .xz, .zst) to a stick, then reads it back and checks the hash.

```
safeusb flash ./ubuntu.iso.xz /dev/sdb
safeusb flash ./ubuntu.iso /dev/sdb --sha256 <expected hex> --no-verify
```

- `--sha256` checks the image before anything is written.
- `--no-verify` skips the read-back check. Leave it off unless you have a reason.
- `--block-size-mib` sets the write block size (1 to 64, default 4).
- a progress bar shows the write and the verify, with speed and ETA. The write and the source read run in parallel.
- `--force` lets the target be a file that already exists.

### sum

Prints a hash of a file, or checks it against a known value.

```
safeusb sum ./ubuntu.iso
safeusb sum ./ubuntu.iso --algo blake3 --check <hex>
```

### inspect

Prints the MBR or GPT partition table of a disk or image.

```
safeusb inspect /dev/sdb
```

## safety notes

- The system disk is refused. SafeUSB will not write to it, even with `--force`.
- Every write that changes a disk asks you to type the target path exactly.
- On Windows, SafeUSB locks and dismounts every volume on the target disk before writing, so Explorer or a running program can't change the stick mid-write. If a volume is busy, close what is using it and retry.
- Raw disk access needs admin rights on Windows and root on Linux. Run the terminal as admin or use sudo. Backups from a file do not need this.
- Double-check the path from `list-devices` before you confirm. Wrong disk, wrong data.

## how it works

The archive format, in order:

- a fixed header with the image size, chunk size, and zstd level
- the data chunks, each compressed with zstd
- an index that maps each chunk to its position and hash
- a footer that points back to the index
- a blake3 hash for each chunk, so `verify` can find damage

Chunks that are all zeros are stored as a marker, and identical chunks are stored once.

## building from source

You need a recent stable Rust toolchain.

```
cargo build --release
```

The binary ends up in `target/release/` (`safeusb.exe` on Windows).

## status

Working in the code now:

- `list-devices` (with system and removable flags)
- `backup`, `restore`, `verify`, `info`
- `flash` (with read-back verify and `--sha256` pre-check)
- `sum` (sha256 and blake3, with `--check`)
- `inspect` (MBR and GPT partition table reading)

Real-USB testing is limited so far. The archive code is tested on files, but the disk paths have seen far less use. Try it on a spare stick first. Do not point it at a disk you cannot afford to lose.

## contributing

Found a bug or want a feature? Open an issue. Pull requests are welcome too. Keep changes small and say what you tested.

## license

SafeUSB is free software under the GNU General Public License, version 3 or later. See [LICENSE](LICENSE).
