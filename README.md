<p align="center">
  <img src="assets/logo.svg" alt="SafeUSB" width="560">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: GPL-3.0-or-later" src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue"></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-stable-orange">
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-windows%20%7C%20linux-lightgrey">
  <img alt="Status" src="https://img.shields.io/badge/status-early-yellow">
</p>

<p align="center">
  <b>Cross-platform disk backup, restore, and USB flashing, written in Rust.</b>
</p>

SafeUSB writes ISO and disk images to USB drives, reads the drive back to verify the write, and backs up disks into compressed, checksummed archives. It runs on Windows and Linux.

> **Status:** early. The archive format and the flash path are tested. Real-hardware testing is limited, so try it on a spare drive first.

## Demo

Flashing a 512 MiB image to a USB drive, then verifying it (sample output):

```
$ safeusb flash ubuntu.iso \\.\PhysicalDrive1
flash target:
  image:       ubuntu.iso (Raw)
  path:        \\.\PhysicalDrive1
  size:        29.3 GiB (31457280000 bytes)
  description: Generic Flash Disk (PhysicalDrive1)
everything on the target will be overwritten, type the path exactly to confirm:
> \\.\PhysicalDrive1
flash done: 512.0 MiB written | sha256 c457d02b83f0acd53fe1a6dd63c422381c6939e8fe5908202eb91738fbf7ee4b | verified yes
```

## Features

- Flash ISO and disk images, including `.gz`, `.xz`, and `.zst` compressed images
- Read-back verification after every write, with an optional SHA-256 check of the image before writing
- Backup and restore of disks into zstd-compressed archives with BLAKE3 integrity checks
- Partition table inspection for MBR and GPT
- File hashing with SHA-256 and BLAKE3, including a check mode
- Refuses to write to the system disk and requires typing the target path before any write
- On Windows, locks and dismounts the volumes on the target disk before writing

## Installation

You need a stable Rust toolchain.

```
cargo build --release
```

The binary is written to `target/release/safeusb` (or `safeusb.exe` on Windows).

## Usage

```
safeusb list-devices
safeusb flash <image> <target> [--sha256 HEX] [--no-verify] [--block-size-mib N]
safeusb backup <source> --output <dir or file> [--zstd-level N] [--chunk-size-mib N] [--long-window-log N]
safeusb restore <archive> <target> [--force]
safeusb verify <archive>
safeusb info <archive>
safeusb sum <file> [--algo sha256|blake3] [--check HEX]
safeusb inspect <disk or image>
```

### Examples

List disks, then flash an image:

```
safeusb list-devices
safeusb flash ./ubuntu.iso.xz /dev/sdb
```

Check the image against a known SHA-256 before writing:

```
safeusb flash ./ubuntu.iso /dev/sdb --sha256 <expected hex>
```

Back up a disk to an archive, then restore it:

```
safeusb backup /dev/sdb --output ./backups/
safeusb restore ./backups/sdb.safeusb /dev/sdc
```

Check a download against a known hash:

```
safeusb sum ./ubuntu.iso --check <expected hex>
```

### Options

| Command | Option | Default | Description |
|---|---|---|---|
| `backup` | `--zstd-level` | 9 | Compression level, 1 to 22 |
| `backup` | `--chunk-size-mib` | 1 | Chunk size in MiB, 1 to 1024 |
| `backup` | `--long-window-log` | 31 | zstd long window size, 10 to 31 |
| `flash` | `--sha256` | none | Expected SHA-256 of the image, checked before writing |
| `flash` | `--no-verify` | off | Skip the read-back check |
| `flash` | `--block-size-mib` | 4 | Write block size, 1 to 64 |
| `flash`, `restore` | `--force` | off | Skip the normal target checks |
| `sum` | `--algo` | sha256 | `sha256` or `blake3` |
| `sum` | `--check` | none | Verify the file against this hash |

## Safety

- The system disk is refused, even with `--force`.
- Every write that changes a disk asks you to type the target path exactly. If the text does not match, nothing is written.
- On Windows, volumes on the target disk are locked and dismounted before the write. If a volume is busy, close the program using it and retry.
- Raw disk access needs administrator rights on Windows and root on Linux.
- Check the device path from `list-devices` before you confirm. A wrong path means the wrong disk is overwritten.

## Archive Format

An archive is a single file with four parts, in order:

1. A fixed header with the image size, chunk size, and zstd level
2. Data chunks, each compressed with zstd
3. An index mapping each chunk to its position and BLAKE3 hash
4. A footer that points back to the index

All-zero chunks are stored as markers, and identical chunks are stored once.

## Building and Testing

```
cargo build --release
cargo test
cargo clippy --all-targets
```

## Contributing

Bug reports and feature requests are welcome. Open an issue before starting a large change. Keep pull requests focused, and include tests where they make sense.

## License

SafeUSB is licensed under the GNU General Public License, version 3 or later. See [LICENSE](LICENSE).
