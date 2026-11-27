import argparse
import os
import sys


def make_image(path: str, size_mib: int, zero_mib: int) -> None:
    chunk = 1 << 20
    with open(path, "wb") as f:
        for _ in range(size_mib):
            f.write(os.urandom(chunk))
        if zero_mib:
            f.truncate(f.tell() + zero_mib * chunk)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="write a random test image")
    parser.add_argument("path", help="file to write")
    parser.add_argument("--size-mib", type=int, default=512)
    parser.add_argument("--zero-mib", type=int, default=0, help="zero-filled tail")
    args = parser.parse_args(argv)
    make_image(args.path, args.size_mib, args.zero_mib)
    print(f"wrote {args.path} ({args.size_mib + args.zero_mib} MiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
