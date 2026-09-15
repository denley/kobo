//! `kobo`: command-line shell over the Kobo core library.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use kobo_core::{Mapping, PcAddr, Rom, SnesAddr, config};

#[derive(Parser)]
#[command(
    name = "kobo",
    version,
    about = "Super Mario World ROM editor and build system"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect ROM images.
    Rom {
        #[command(subcommand)]
        command: RomCommand,
    },
    /// Convert between SNES addresses and ROM file offsets.
    Addr {
        /// Address to convert. `$05E000` or `05E000` is a SNES address,
        /// `0x2E000` is a file offset.
        addr: String,
        /// Use SA-1 mapping instead of LoROM.
        #[arg(long)]
        sa1: bool,
    },
}

#[derive(Subcommand)]
enum RomCommand {
    /// Print header and identification details.
    Info {
        /// ROM path. Defaults to the configured vanilla ROM.
        path: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Rom {
            command: RomCommand::Info { path },
        } => rom_info(path),
        Command::Addr { addr, sa1 } => convert_addr(&addr, sa1),
    }
}

fn resolve_rom_path(path: Option<PathBuf>) -> Result<PathBuf> {
    match path {
        Some(p) => Ok(p),
        None => Ok(config::vanilla_rom_path()?),
    }
}

fn rom_info(path: Option<PathBuf>) -> Result<()> {
    let path = resolve_rom_path(path)?;
    let rom = Rom::load(&path).with_context(|| format!("loading {}", path.display()))?;
    let h = rom.internal_header();
    let computed = rom.compute_checksum();
    println!("path:            {}", path.display());
    println!(
        "size:            {} bytes ({} KiB)",
        rom.len(),
        rom.len() / 1024
    );
    println!(
        "copier header:   {}",
        if rom.has_copier_header() { "yes" } else { "no" }
    );
    println!("mapping:         {:?}", rom.mapping());
    println!("title:           {:?}", h.title);
    println!("map mode:        ${:02X}", h.map_mode);
    println!("cartridge type:  ${:02X}", h.cartridge_type);
    println!("declared size:   {} KiB", h.rom_size() / 1024);
    println!("sram:            {} KiB", h.sram_size() / 1024);
    println!("region:          ${:02X}", h.region);
    println!("version:         1.{}", h.version);
    println!(
        "checksum:        ${:04X} (complement ${:04X}, computed ${:04X}, {})",
        h.checksum,
        h.checksum_complement,
        computed,
        if h.checksum_pair_valid() && computed == h.checksum {
            "ok"
        } else {
            "MISMATCH"
        }
    );
    println!("sha1:            {}", rom.sha1_hex());
    println!("identity:        {:?}", rom.identify());
    Ok(())
}

fn convert_addr(text: &str, sa1: bool) -> Result<()> {
    let mapping = if sa1 { Mapping::Sa1Rom } else { Mapping::LoRom };
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        let pc = PcAddr::new(u32::from_str_radix(hex, 16).context("bad file offset")?);
        let snes = mapping.pc_to_snes(pc)?;
        println!("{pc} -> {snes} ({mapping:?})");
    } else {
        let hex = text.strip_prefix('$').unwrap_or(text);
        let raw = u32::from_str_radix(hex, 16).context("bad SNES address")?;
        if raw > 0xFF_FFFF {
            bail!("SNES address {text} does not fit in 24 bits");
        }
        let snes = SnesAddr::new(raw);
        let pc = mapping.snes_to_pc(snes)?;
        println!("{snes} -> {pc} ({mapping:?})");
    }
    Ok(())
}
