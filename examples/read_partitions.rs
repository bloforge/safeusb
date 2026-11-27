use safeusb::part::{read_table, Scheme};
use std::env;
use std::fs::File;

fn main() -> anyhow::Result<()> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: read_partitions <image or disk>"))?;
    let mut file = File::open(&path)?;
    let table = read_table(&mut file)?;

    let scheme = match table.scheme {
        Scheme::None => "none",
        Scheme::Mbr => "mbr",
        Scheme::Gpt => "gpt",
    };
    println!("{path}: {scheme}, {} partition(s)", table.parts.len());
    for p in &table.parts {
        println!("  {}: start {} sectors {} ({})", p.number, p.start_lba, p.sectors, p.kind);
    }
    Ok(())
}
