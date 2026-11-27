# Architecture

SafeUSB is one Rust crate with a library (`src/lib.rs`) and a command-line binary (`src/main.rs`). The library holds all the logic, so the tests and examples can use it directly.

## Modules

| Module | Purpose |
|---|---|
| `disk` | Lists disks and checks targets. Has one backend per OS: `linux.rs` (sysfs) and `windows.rs` (PowerShell `Get-Disk`, Win32 volume locking). |
| `img` | Backup and restore of `.safeusb` archives. `wire.rs` holds the on-disk byte layout. See [archive-format.md](archive-format.md). |
| `input` | Detects and opens images. Recognizes raw, ISO 9660, gzip, xz, and zstd input, and streams compressed input without unpacking it to disk. |
| `flash` | Writes a stream to a target and reads it back. Source reads and target writes run on separate threads. |
| `hashing` | SHA-256 and BLAKE3 for files, with check mode. |
| `part` | Reads MBR and GPT partition tables, read-only. |

## Write path

1. `input::open` detects the image kind and returns a reader. Compressed images have no known size.
2. `disk::check_target_force` refuses the system disk and targets that are too small.
3. `disk::lock_volumes` locks and dismounts the volumes on the target disk (Windows only).
4. `flash::flash` streams the image to the target. The source is hashed as it is read.
5. The target is read back and hashed. The write counts as good only if both hashes match.

## Safety checks

- The system disk is refused even with `--force`.
- Every write asks the user to type the target path exactly.
- Raw disk access needs admin on Windows and root on Linux.

## Tests

- Unit tests live next to the code they test.
- `tests/` holds integration tests for each module, using temporary files only. They never open real devices.
