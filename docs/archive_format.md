# Archive format

A `.safeusb` archive stores a disk image as deduplicated, zstd-compressed chunks. All integers are little-endian.

## Layout

| Part | Size | Contents |
|---|---|---|
| Header | 30 bytes | Fixed header (below) |
| Blobs | variable | For each unique non-zero chunk: a 32-byte BLAKE3 hash, then the zstd-compressed chunk data |
| Index | 13 bytes per chunk | One entry per chunk, in image order |
| Footer | 80 bytes | Points back to the index and stores the hashes |

## Header (30 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | Magic `SAFEUSB\0` |
| 8 | 2 | Version (currently 1) |
| 10 | 8 | Image size in bytes |
| 18 | 4 | Chunk size in bytes |
| 22 | 4 | zstd level (signed) |
| 26 | 4 | zstd long window log (10 to 31) |

A reader rejects the archive if the magic is wrong, the version is unknown, the chunk size is zero, or the window log is out of range.

## Index entry (13 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Kind: `0` is an all-zero chunk, `1` is a stored blob |
| 1 | 8 | Blob offset in the file (points at the blob's hash), or `0` for zero chunks |
| 9 | 4 | Compressed length of the blob data, or `0` for zero chunks |

Identical chunks share one blob. Several index entries can point at the same offset.

## Footer (80 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | Index offset in the file |
| 8 | 32 | BLAKE3 hash of the header |
| 40 | 32 | BLAKE3 hash of the index |
| 72 | 8 | Magic `SAFEUSB\0` |

## Checks when reading

1. The file must be at least header plus footer long.
2. The footer magic and the header hash must match.
3. The index must sit exactly between the index offset and the footer.
4. The index hash must match.
5. Each blob must lie inside the blob area, decompress to the expected chunk length, and match its stored BLAKE3 hash before any bytes are written out.

Any failure stops the restore or verify with an error. The restore never writes a chunk whose hash does not match.
