//! `kobo`: command-line shell over the Kobo core library.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use kobo_core::gfx::{self, Bpp, GFX_FILE_COUNT};
use kobo_core::image::{grayscale, tile_sheet};
use kobo_core::level::{self, Layer2Data};
use kobo_core::map16;
use kobo_core::palette::{self, LevelPaletteSelect};
use kobo_core::render::{self, LayerTiles};
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
    /// Work with GFX files (8x8 tile graphics).
    Gfx {
        #[command(subcommand)]
        command: GfxCommand,
    },
    /// Inspect levels.
    Level {
        #[command(subcommand)]
        command: LevelCommand,
    },
    /// Render palettes.
    Palette {
        #[command(subcommand)]
        command: PaletteCommand,
    },
    /// Render Map16 tiles.
    Map16 {
        #[command(subcommand)]
        command: Map16Command,
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
        #[command(flatten)]
        rom: RomArg,
    },
}

#[derive(Subcommand)]
enum GfxCommand {
    /// List the GFX files in a ROM.
    List {
        #[command(flatten)]
        rom: RomArg,
    },
    /// Write GFX00 to GFX31 as .bin files in Lunar Magic's export layout.
    Export {
        #[command(flatten)]
        rom: RomArg,
        /// Output directory. Created if missing.
        dir: PathBuf,
    },
    /// Render a GFX file as a grayscale tile sheet PNG.
    Png {
        #[command(flatten)]
        rom: RomArg,
        /// GFX file index in hex, for example `00` or `1A`.
        index: String,
        /// Output PNG path.
        out: PathBuf,
        /// Tiles per row.
        #[arg(long, default_value_t = 16)]
        columns: u32,
        /// Reinterpret the stored bytes at this bit depth (2, 3, or 4)
        /// instead of the inferred one. Useful for checking unknown files.
        #[arg(long)]
        bpp: Option<u8>,
    },
}

#[derive(Subcommand)]
enum LevelCommand {
    /// Print a level's header and data pointers.
    Info {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
}

#[derive(Subcommand)]
enum PaletteCommand {
    /// Render the level palette as a 16x16 swatch grid.
    Png {
        #[command(flatten)]
        rom: RomArg,
        #[command(flatten)]
        sel: PaletteArgs,
        /// Output PNG path.
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum Map16Command {
    /// Render all 0x400 Map16 tiles of a tileset as a 16-column sheet.
    Png {
        #[command(flatten)]
        rom: RomArg,
        #[command(flatten)]
        sel: PaletteArgs,
        /// Object tileset (0 to 14). Overrides the level's tileset.
        #[arg(long)]
        tileset: Option<u8>,
        /// Output PNG path.
        out: PathBuf,
    },
}

/// Palette selection, either from a level header or explicit fields.
#[derive(Args)]
struct PaletteArgs {
    /// Take palette and tileset settings from this level's header (hex).
    #[arg(long)]
    level: Option<String>,
    #[arg(long, default_value_t = 0)]
    fg: u8,
    #[arg(long, default_value_t = 0)]
    bg: u8,
    #[arg(long, default_value_t = 0)]
    sprite: u8,
    #[arg(long, default_value_t = 0)]
    back_area: u8,
}

impl PaletteArgs {
    /// Resolves to (palette selection, object tileset if a level was given).
    fn resolve(&self, rom: &Rom) -> Result<(LevelPaletteSelect, Option<u8>)> {
        if let Some(level) = &self.level {
            let level = parse_level(level)?;
            let h = level::read_primary_header(rom, level)?;
            Ok((h.palette_select(), Some(h.object_tileset)))
        } else {
            Ok((
                LevelPaletteSelect {
                    fg: self.fg,
                    bg: self.bg,
                    sprite: self.sprite,
                    back_area: self.back_area,
                },
                None,
            ))
        }
    }
}

fn parse_level(text: &str) -> Result<u16> {
    u16::from_str_radix(text, 16).context("level must be hex, e.g. 105")
}

#[derive(Args)]
struct RomArg {
    /// ROM path. Defaults to the configured vanilla ROM.
    #[arg(long, short = 'r')]
    rom: Option<PathBuf>,
}

impl RomArg {
    fn load(&self) -> Result<Rom> {
        let path = match &self.rom {
            Some(p) => p.clone(),
            None => config::vanilla_rom_path()?,
        };
        Rom::load(&path).with_context(|| format!("loading {}", path.display()))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Rom {
            command: RomCommand::Info { rom },
        } => rom_info(&rom.load()?),
        Command::Gfx { command } => match command {
            GfxCommand::List { rom } => gfx_list(&rom.load()?),
            GfxCommand::Export { rom, dir } => gfx_export(&rom.load()?, &dir),
            GfxCommand::Png {
                rom,
                index,
                out,
                columns,
                bpp,
            } => gfx_png(&rom.load()?, &index, &out, columns, bpp),
        },
        Command::Level {
            command: LevelCommand::Info { rom, level },
        } => level_info(&rom.load()?, &level),
        Command::Palette {
            command: PaletteCommand::Png { rom, sel, out },
        } => palette_png(&rom.load()?, &sel, &out),
        Command::Map16 {
            command:
                Map16Command::Png {
                    rom,
                    sel,
                    tileset,
                    out,
                },
        } => map16_png(&rom.load()?, &sel, tileset, &out),
        Command::Addr { addr, sa1 } => convert_addr(&addr, sa1),
    }
}

fn level_info(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let h = level::read_primary_header(rom, level)?;
    println!("level:            {level:03X}");
    println!("layer 1 data:     {}", level::layer1_ptr(rom, level)?);
    match level::layer2_ptr(rom, level)? {
        Layer2Data::Objects(a) => println!("layer 2 data:     {a} (objects)"),
        Layer2Data::Tilemap(a) => println!("layer 2 data:     {a} (background tilemap)"),
    }
    println!("sprite data:      {}", level::sprite_ptr(rom, level)?);
    println!("screens:          {}", h.screens);
    println!("level mode:       ${:02X}", h.level_mode);
    println!(
        "object tileset:   {} ({:?})",
        h.object_tileset,
        gfx::object_tileset_files(rom, h.object_tileset)
            .map(|f| f.map(|x| format!("{x:02X}")))
            .unwrap_or_default()
    );
    println!(
        "sprite tileset:   {} ({:?})",
        h.sprite_tileset,
        gfx::sprite_tileset_files(rom, h.sprite_tileset)
            .map(|f| f.map(|x| format!("{x:02X}")))
            .unwrap_or_default()
    );
    println!("fg palette:       {}", h.fg_palette);
    println!("bg palette:       {}", h.bg_palette);
    println!("sprite palette:   {}", h.sprite_palette);
    println!("back area colour: {}", h.back_area);
    println!("music:            {}", h.music);
    println!("time:             {}", h.time);
    println!("layer 3 priority: {}", h.layer3_priority);
    println!("item memory:      {}", h.item_memory);
    println!("vertical scroll:  {}", h.vertical_scroll);
    Ok(())
}

fn palette_png(rom: &Rom, sel: &PaletteArgs, out: &PathBuf) -> Result<()> {
    let (sel, _) = sel.resolve(rom)?;
    let pal = palette::vanilla_level_palette(rom, sel)?;
    let img = render::palette_swatch(&pal, 16);
    img.write_png(out)?;
    println!("palette {sel:?} -> {}", out.display());
    Ok(())
}

fn map16_png(rom: &Rom, sel: &PaletteArgs, tileset: Option<u8>, out: &PathBuf) -> Result<()> {
    let (sel, level_tileset) = sel.resolve(rom)?;
    let tileset = tileset.or(level_tileset).unwrap_or(0);
    let pal = palette::vanilla_level_palette(rom, sel)?;
    let back = palette::vanilla_back_area_color(rom, sel.back_area)?.to_rgb8();
    let table = map16::vanilla_map16(rom, tileset, true)?;
    let tiles = LayerTiles::for_object_tileset(rom, tileset)?;
    let img = render::map16_sheet(&table, &tiles, &pal, back, 16);
    img.write_png(out)?;
    println!(
        "tileset {tileset}, palette {sel:?}: {} tiles, {}x{} -> {}",
        table.tiles.len(),
        img.width,
        img.height,
        out.display()
    );
    Ok(())
}

fn rom_info(rom: &Rom) -> Result<()> {
    let h = rom.internal_header();
    let computed = rom.compute_checksum();
    if let Some(path) = rom.source() {
        println!("path:            {}", path.display());
    }
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

fn gfx_list(rom: &Rom) -> Result<()> {
    println!("file   addr     format  tiles  stored  compressed");
    for index in 0..GFX_FILE_COUNT {
        match gfx::read_gfx_file(rom, index) {
            Ok(f) => println!(
                "GFX{:02X}  {}  {:<6}  {:>5}  {:>6}  {:>10}",
                index,
                f.addr,
                f.bpp()
                    .map_or("raw".to_string(), |b| format!("{}bpp", b.bits())),
                f.tile_count(),
                f.data.len(),
                f.compressed_len
            ),
            Err(e) => println!("GFX{index:02X}  error: {e}"),
        }
    }
    Ok(())
}

fn gfx_export(rom: &Rom, dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for index in 0..GFX_FILE_COUNT {
        let f = gfx::read_gfx_file(rom, index)?;
        let path = dir.join(format!("GFX{index:02X}.bin"));
        fs::write(&path, f.to_lm_export())
            .with_context(|| format!("writing {}", path.display()))?;
    }
    println!("wrote {GFX_FILE_COUNT} files to {}", dir.display());
    Ok(())
}

fn gfx_png(rom: &Rom, index: &str, out: &PathBuf, columns: u32, bpp: Option<u8>) -> Result<()> {
    let index = u8::from_str_radix(index, 16).context("GFX index must be hex, e.g. 1A")?;
    let f = gfx::read_gfx_file(rom, index)?;
    let bpp = match bpp {
        None => f.bpp().ok_or_else(|| {
            anyhow::anyhow!("GFX{index:02X} is not planar tile data; pass --bpp to force a depth")
        })?,
        Some(2) => Bpp::Two,
        Some(3) => Bpp::Three,
        Some(4) => Bpp::Four,
        Some(other) => bail!("unsupported bit depth {other}; use 2, 3, or 4"),
    };
    let tiles = gfx::decode_tiles(bpp, &f.data);
    let img = tile_sheet(&tiles, columns, &grayscale(bpp.colors()));
    img.write_png(out)?;
    println!(
        "GFX{index:02X}: {} tiles, {}bpp, {}x{} -> {}",
        tiles.len(),
        bpp.bits(),
        img.width,
        img.height,
        out.display()
    );
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
