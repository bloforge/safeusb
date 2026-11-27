use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use safeusb::disk;
use indicatif::{ProgressBar, ProgressStyle};
use safeusb::flash::{self, FlashOpts, Phase};
use safeusb::hashing::{self, Algo};
use safeusb::img::{self, BackupOptions, Stats};
use safeusb::input;
use safeusb::part::{self, Scheme};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "safeusb", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    ListDevices,
    Backup {
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 9)]
        zstd_level: i32,
        #[arg(long, default_value_t = 1)]
        chunk_size_mib: u32,
        #[arg(long, default_value_t = 31)]
        long_window_log: u32,
    },
    Restore {
        archive: PathBuf,
        target: PathBuf,
        #[arg(long)]
        force: bool,
    },
    Verify {
        archive: PathBuf,
    },
    Info {
        archive: PathBuf,
    },
    Flash {
        image: PathBuf,
        target: PathBuf,
        #[arg(long)]
        no_verify: bool,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long, default_value_t = 4)]
        block_size_mib: usize,
    },
    Sum {
        file: PathBuf,
        #[arg(long, default_value = "sha256")]
        algo: String,
        #[arg(long)]
        check: Option<String>,
    },
    Inspect {
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Cmd::ListDevices => list_cmd(),
        Cmd::Backup {
            source,
            output,
            zstd_level,
            chunk_size_mib,
            long_window_log,
        } => backup_cmd(
            &source,
            &output,
            zstd_level,
            chunk_size_mib,
            long_window_log,
        ),
        Cmd::Restore {
            archive,
            target,
            force,
        } => restore_cmd(&archive, &target, force),
        Cmd::Verify { archive } => verify_cmd(&archive),
        Cmd::Info { archive } => info_cmd(&archive),
        Cmd::Flash {
            image,
            target,
            no_verify,
            sha256,
            force,
            block_size_mib,
        } => flash_cmd(&image, &target, no_verify, sha256, force, block_size_mib),
        Cmd::Sum { file, algo, check } => sum_cmd(&file, &algo, check),
        Cmd::Inspect { path } => inspect_cmd(&path),
    }
}

fn list_cmd() -> Result<()> {
    let disks = disk::list()?;
    if disks.is_empty() {
        println!("no disks found");
        return Ok(());
    }
    println!(
        "{:<28} {:>12}  {:<6}  {:<9}  DESCRIPTION",
        "PATH", "SIZE", "SYSTEM", "REMOVABLE"
    );
    for d in &disks {
        println!(
            "{:<28} {:>12}  {:<6}  {:<9}  {}",
            d.path.display(),
            human(d.size),
            if d.is_system { "YES" } else { "no" },
            if d.removable { "yes" } else { "no" },
            d.description
        );
    }
    Ok(())
}

fn backup_cmd(
    source: &Path,
    output: &Path,
    zstd_level: i32,
    chunk_size_mib: u32,
    long_window_log: u32,
) -> Result<()> {
    if !(1..=1024).contains(&chunk_size_mib) {
        bail!("--chunk-size-mib must be 1 to 1024");
    }
    if !(1..=22).contains(&zstd_level) {
        bail!("--zstd-level must be 1 to 22");
    }
    if !(10..=31).contains(&long_window_log) {
        bail!("--long-window-log must be 10 to 31");
    }
    let (mut src, size) = disk::open_src(source)
        .with_context(|| format!("failed to open source {}", source.display()))?;
    let archive = out_path(output, source)?;
    eprintln!(
        "backing up {} ({}) -> {}",
        source.display(),
        human(size),
        archive.display()
    );

    let opts = BackupOptions {
        chunk_size: chunk_size_mib << 20,
        zstd_level,
        long_window_log,
    };
    let mut progress = stderr_progress("backup");
    let stats = img::backup(&mut src, size, &archive, &opts, &mut progress)?;
    print_stats("backup done", &stats);
    Ok(())
}

fn restore_cmd(archive: &Path, target: &Path, force: bool) -> Result<()> {
    let info = img::info(archive)
        .with_context(|| format!("failed to read archive {}", archive.display()))?;
    let dev = disk::check_target_force(target, info.image_size, force)?;

    eprintln!("restore target:");
    eprintln!("  path:        {}", target.display());
    eprintln!("  size:        {} ({} bytes)", human(dev.size), dev.size);
    eprintln!("  description: {}", dev.description);
    eprintln!(
        "  image size:  {} ({} bytes)",
        human(info.image_size),
        info.image_size
    );
    confirm_target(target)?;

    let _lock = disk::lock_volumes(target)?;
    let mut out = disk::open_target(target)?;
    let mut progress = stderr_progress("restore");
    let stats = img::restore(archive, &mut out, &mut progress)?;
    out.sync_all()
        .with_context(|| format!("failed to flush {} to disk", target.display()))?;
    print_stats("restore done", &stats);
    Ok(())
}

fn flash_cmd(
    image: &Path,
    target: &Path,
    no_verify: bool,
    sha256: Option<String>,
    force: bool,
    block_size_mib: usize,
) -> Result<()> {
    if !(1..=64).contains(&block_size_mib) {
        bail!("--block-size-mib must be 1 to 64");
    }
    let kind = input::detect(image)?;
    let mut src = input::open(image)
        .with_context(|| format!("failed to open image {}", image.display()))?;
    let need = src.size.unwrap_or(0);
    let dev = disk::check_target_force(target, need, force)?;

    if let Some(expected) = sha256 {
        let mut pre = input::open(image)?;
        let actual = hash_stream(&mut *pre.reader)?;
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            bail!("image sha256 mismatch, expected {expected} got {actual}, nothing was written");
        }
        eprintln!("image sha256 matches");
    }

    eprintln!("flash target:");
    eprintln!("  image:       {} ({:?})", image.display(), kind);
    eprintln!("  path:        {}", target.display());
    eprintln!("  size:        {} ({} bytes)", human(dev.size), dev.size);
    eprintln!("  description: {}", dev.description);
    confirm_target(target)?;

    let _lock = disk::lock_volumes(target)?;
    let mut out = disk::open_target(target)?;
    let opts = FlashOpts {
        block_size: block_size_mib << 20,
        verify: !no_verify,
    };
    let known = src.size.is_some();
    let bar = ProgressBar::new(src.size.unwrap_or(0));
    bar.set_style(bar_style());
    bar.set_message("writing");
    let mut progress = |phase: Phase, done: u64, total: u64| match phase {
        Phase::Write => {
            if !known {
                bar.set_length(done);
            }
            bar.set_position(done);
        }
        Phase::Verify => {
            bar.set_length(total);
            bar.set_message("verifying");
            bar.set_position(done);
        }
    };
    let report = flash::flash(&mut *src.reader, &mut out, &opts, &mut progress)?;
    bar.finish_with_message("done");
    println!(
        "flash done: {} written | sha256 {} | verified {}",
        human(report.written),
        report.sha256,
        if report.verified { "yes" } else { "no" }
    );
    Ok(())
}

fn sum_cmd(file: &Path, algo: &str, check: Option<String>) -> Result<()> {
    let algo: Algo = algo.parse()?;
    match check {
        Some(expected) => {
            hashing::check_digest(file, &expected, algo)?;
            println!("ok: {}", file.display());
        }
        None => {
            let digest = match algo {
                Algo::Sha256 => hashing::sha256_file(file)?,
                Algo::Blake3 => hashing::blake3_file(file)?,
            };
            println!("{digest}  {}", file.display());
        }
    }
    Ok(())
}

fn inspect_cmd(path: &Path) -> Result<()> {
    let mut file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let table = part::read_table(&mut file)?;
    let scheme = match table.scheme {
        Scheme::None => "none",
        Scheme::Mbr => "mbr",
        Scheme::Gpt => "gpt",
    };
    println!("scheme: {scheme}");
    if table.parts.is_empty() {
        println!("no partitions found");
    }
    for p in &table.parts {
        println!(
            "  {:>2}  start {:>12}  sectors {:>12}  {}",
            p.number, p.start_lba, p.sectors, p.kind
        );
    }
    Ok(())
}

fn verify_cmd(archive: &Path) -> Result<()> {
    let mut progress = stderr_progress("verify");
    let stats = img::verify(archive, &mut progress)?;
    print_stats("verify ok", &stats);
    Ok(())
}

fn info_cmd(archive: &Path) -> Result<()> {
    let info = img::info(archive)?;
    println!(
        "image size:  {} ({} bytes)",
        human(info.image_size),
        info.image_size
    );
    println!("chunk size:  {} bytes", info.chunk_size);
    println!("zstd level:  {}", info.zstd_level);
    Ok(())
}

fn confirm_target(target: &Path) -> Result<()> {
    eprintln!("everything on the target will be overwritten, type the path exactly to confirm:");
    eprint!("> ");
    io::stderr().flush().ok();

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("failed to read answer")?;
    let typed = line.trim_end_matches(['\r', '\n']);
    let expected = target.to_string_lossy().into_owned();
    if typed != expected {
        bail!("path did not match, nothing was written");
    }
    Ok(())
}

fn hash_stream(r: &mut dyn Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf).context("failed to read image")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn out_path(output: &Path, source: &Path) -> Result<PathBuf> {
    if output.is_dir() {
        let raw = source.to_string_lossy();
        let name = raw.rsplit(['/', '\\']).next().unwrap_or("");
        let clean: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        let clean = if clean.is_empty() {
            "image".to_string()
        } else {
            clean
        };
        return Ok(output.join(format!("{clean}.safeusb")));
    }
    Ok(output.to_path_buf())
}

fn stderr_progress(label: &'static str) -> impl FnMut(u64, u64) {
    move |done: u64, total: u64| {
        if total == 0 {
            return;
        }
        let pct = (done.min(total) * 100) / total;
        eprint!("\r{label}: {pct:3}% ({} / {})", human(done), human(total));
        if done >= total {
            eprintln!();
        }
    }
}

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{msg:>10} [{wide_bar:.cyan/blue}] {percent:>3}% {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
    )
    .expect("progress template is valid")
    .progress_chars("#>-")
}

fn print_stats(label: &str, s: &Stats) {
    println!(
        "{label}: image {} | chunks {} (unique {}, zero {}) | archive {}",
        human(s.image_size),
        s.chunks_total,
        s.chunks_unique,
        s.zero_chunks,
        human(s.archive_bytes)
    );
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
