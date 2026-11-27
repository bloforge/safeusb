# Contributing to SafeUSB

Thanks for looking at SafeUSB. Bug reports, feature ideas, and pull requests are all welcome.

## Before you start

- Open an issue for anything bigger than a small fix, so we can agree on the approach first.
- For bugs, include your OS, the exact command you ran, and the full output.

## Making a change

1. Fork the repo and create a branch.
2. Build and test:

   ```
   cargo test
   cargo clippy --all-targets
   ```

3. Keep the change focused. One fix or feature per pull request.
4. Add tests for new behavior where it makes sense.
5. Note what you tested in the pull request description.

## Safety

SafeUSB writes to real disks. Any change that touches the write path, the target checks, or the system-disk guard needs extra care. Please test those on a spare drive, never a disk you can't afford to lose.

## Style

- Error messages start lowercase, have no trailing period, and avoid contractions.
- Names are short and plain, in `snake_case` for functions and variables.
- Keep comments rare and useful. Don't restate what the code does.

## License

By contributing you agree your changes are released under the GNU General Public License, version 3 or later.
