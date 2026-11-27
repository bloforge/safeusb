import argparse
import os
import subprocess
import sys
import time


def run(binary: str, image: str, target: str, block_mib: int) -> float:
    if os.path.exists(target):
        os.remove(target)
    start = time.monotonic()
    subprocess.run(
        [binary, "flash", image, target, "--block-size-mib", str(block_mib)],
        input=f"{target}\n",
        text=True,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return time.monotonic() - start


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="time safeusb flash to a file target")
    parser.add_argument("binary", help="path to the safeusb binary")
    parser.add_argument("image", help="image to flash")
    parser.add_argument("target", help="file to write (never a real disk)")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--block-size-mib", type=int, default=4)
    args = parser.parse_args(argv)

    size = os.path.getsize(args.image)
    times = [run(args.binary, args.image, args.target, args.block_size_mib) for _ in range(args.runs)]
    best = min(times)
    mib = size / (1 << 20)
    print(f"image {mib:.1f} MiB, block {args.block_size_mib} MiB, {args.runs} run(s)")
    print(f"best {best:.2f}s ({mib / best:.1f} MiB/s, write + verify)")
    print(f"all runs: {', '.join(f'{t:.2f}s' for t in times)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
