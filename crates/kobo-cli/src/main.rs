//! `kobo`: command-line shell over the Kobo core library.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use kobo_core::expand;
use kobo_core::gfx::{self, Bpp, GFX_FILE_COUNT};
use kobo_core::image::{grayscale, tile_sheet};
use kobo_core::level::{self, Layer2Data};
use kobo_core::map16;
use kobo_core::palette::{self, LevelPaletteSelect};
use kobo_core::ram::RamAddr;
use kobo_core::render::{self, LayerTiles};
use kobo_core::sprites;
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
    /// Render a level to PNG by running the ROM's own loader.
    Png {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Output PNG path.
        out: PathBuf,
        /// Leave out sprites entirely.
        #[arg(long)]
        no_sprites: bool,
        /// Leave out the player at the level's entrance.
        #[arg(long)]
        no_player: bool,
        /// Draw every sprite as an ID marker instead of running the game's
        /// sprite engine for its graphics.
        #[arg(long)]
        markers: bool,
    },
    /// List a level's sprites.
    Sprites {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
    /// Write a level's expanded tile grid planes as `level_XXX.l1lo.bin`
    /// and `.l1hi.bin` in the oracle dump layout.
    Dump {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Output directory.
        dir: PathBuf,
    },
    /// Summarise which ROM pages the loader reads, for finding tables.
    Reads {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Only report addresses at or above this SNES address (hex).
        #[arg(long, default_value = "0F8000")]
        from: String,
        /// Only report instructions that also read inside this 64 KiB bank
        /// (hex bank number), plus instructions within 256 bytes of them.
        #[arg(long)]
        near_bank: Option<String>,
    },
    /// Print foreground Map16 definitions from the loaded object grid,
    /// as `NNNN: 8 hex bytes` lines.
    Map16 {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
    /// Hex-dump a work RAM range after the level loader has run.
    Wram {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Start address, `$7E0FBE` style.
        addr: String,
        /// Byte count (decimal).
        #[arg(default_value_t = 64)]
        len: usize,
    },
    /// Print a level's expanded tile grid as hex, one screen row per line.
    Tiles {
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
    // Let a closed pipe (e.g. `| head`) end the process quietly.
    #[cfg(unix)]
    // SAFETY: resetting SIGPIPE to its default disposition has no
    // preconditions and happens before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
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
        Command::Level { command } => match command {
            LevelCommand::Info { rom, level } => level_info(&rom.load()?, &level),
            LevelCommand::Png {
                rom,
                level,
                out,
                no_sprites,
                no_player,
                markers,
            } => level_png(&rom.load()?, &level, &out, !no_sprites, !no_player, markers),
            LevelCommand::Sprites { rom, level } => level_sprites(&rom.load()?, &level),
            LevelCommand::Tiles { rom, level } => level_tiles(&rom.load()?, &level),
            LevelCommand::Dump { rom, level, dir } => level_dump(&rom.load()?, &level, &dir),
            LevelCommand::Reads {
                rom,
                level,
                from,
                near_bank,
            } => level_reads(&rom.load()?, &level, &from, near_bank.as_deref()),
            LevelCommand::Map16 { rom, level } => level_map16(&rom.load()?, &level),
            LevelCommand::Wram {
                rom,
                level,
                addr,
                len,
            } => level_wram(&rom.load()?, &level, &addr, len),
        },
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

fn level_png(
    rom: &Rom,
    level: &str,
    out: &PathBuf,
    with_sprites: bool,
    with_player: bool,
    markers: bool,
) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    // Graphics and colours come from what the game uploaded to VRAM and
    // CGRAM, so ExGFX, custom palettes, and animated tiles are covered.
    let pal = tiles.palette();
    let layer_tiles = LayerTiles::from_vram(&tiles.vram);
    let mut layers = render::level_layers(&tiles, &layer_tiles);
    let mut diagnostics = tiles.diagnostics.clone();
    let mut marked: Vec<(usize, usize, u8)> = Vec::new();
    if with_sprites && tiles.boss_scene.is_none() {
        let list = sprites::read_sprites_at(rom, tiles.sprite_data_ptr())?;
        if markers {
            marked.extend(list.sprites.iter().map(|s| {
                let (x, y) = s.tile_position(tiles.vertical);
                (x, y, s.id)
            }));
        } else {
            let scene = expand::capture_sprites(rom, &tiles, &list)?;
            diagnostics.extend(scene.diagnostics.iter().cloned());
            render::draw_sprite_scene(&mut layers, &scene, tiles.layer2_offset(), &tiles.vram);
            marked = scene.undrawn;
        }
    }
    warn(level, &diagnostics);
    // The player's OAM slots follow the sprites', so he goes behind them.
    if with_player {
        render::draw_objects(&mut layers, &tiles.player, tiles.object_select, &tiles.vram);
    }
    let mut img = render::compose_level(&tiles, &layers, &pal);
    for (x, y, id) in marked {
        render::draw_sprite_marker(&mut img, x as u32 * 16, y as u32 * 16, id, &tiles.vram);
    }
    img.write_png(out)?;
    println!(
        "level {level:03X}: {} screens, mode ${:02X}, {}x{} -> {}",
        tiles.screens,
        tiles.level_mode,
        img.width,
        img.height,
        out.display()
    );
    Ok(())
}

/// Reports the passes a capture gave up on. Broken per-level code fails
/// every pass the same way, so each distinct error is reported once.
fn warn(level: u16, diagnostics: &[expand::Diagnostic]) {
    let mut reported: Vec<&kobo_core::cpu::CpuError> = Vec::new();
    for diagnostic in diagnostics {
        if reported.contains(&&diagnostic.error) {
            continue;
        }
        reported.push(&diagnostic.error);
        let others = diagnostics
            .iter()
            .filter(|other| other.error == diagnostic.error)
            .count()
            - 1;
        match others {
            0 => eprintln!("warning: level {level:03X}: {diagnostic}"),
            n => eprintln!("warning: level {level:03X}: {diagnostic} (and {n} more passes)"),
        }
    }
}

fn level_dump(rom: &Rom, level: &str, dir: &PathBuf) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    fs::create_dir_all(dir)?;
    fs::write(dir.join(format!("level_{level:03X}.l1lo.bin")), &tiles.low)?;
    fs::write(dir.join(format!("level_{level:03X}.l1hi.bin")), &tiles.high)?;
    fs::write(dir.join(format!("level_{level:03X}.vram.bin")), &tiles.vram)?;
    fs::write(
        dir.join(format!("level_{level:03X}.cgram.bin")),
        &tiles.cgram,
    )?;
    println!("level {level:03X}: wrote planes to {}", dir.display());
    Ok(())
}

fn level_reads(rom: &Rom, level: &str, from: &str, near_bank: Option<&str>) -> Result<()> {
    let level = parse_level(level)?;
    let from = u32::from_str_radix(from.trim_start_matches('$'), 16).context("bad address")?;
    let bank = near_bank
        .map(|b| u32::from_str_radix(b.trim_start_matches('$'), 16))
        .transpose()
        .context("bad bank")?;
    let (_, trace) = expand::expand_level_traced(rom, level, true)?;
    let trace = trace.unwrap_or_default();
    // Instructions of interest: those reading in the given bank, and neighbours.
    let mut hot: Vec<u32> = Vec::new();
    if let Some(bank) = bank {
        hot = trace
            .iter()
            .filter(|(_, a)| a >> 16 == bank)
            .map(|(pc, _)| *pc)
            .collect();
        hot.sort_unstable();
        hot.dedup();
    }
    let interesting = |pc: u32| bank.is_none() || hot.iter().any(|h| pc.abs_diff(*h) <= 0x100);
    type Pages = std::collections::BTreeMap<u32, (u64, u32, u32)>;
    let mut by_pc: std::collections::BTreeMap<u32, Pages> = Default::default();
    for &(pc, a) in &trace {
        let wram = (0x7E_0000..0x80_0000).contains(&a) || (a & 0xFFFF) < 0x2000;
        if (a < from && !wram) || (wram && bank.is_none()) {
            continue;
        }
        // Skip operand reads of the instruction itself.
        if (a > pc && a - pc <= 3) || !interesting(pc) {
            continue;
        }
        let e = by_pc
            .entry(pc)
            .or_default()
            .entry(a & 0xFF_FF00)
            .or_insert((0, a, a));
        e.0 += 1;
        e.1 = e.1.min(a);
        e.2 = e.2.max(a);
    }
    println!(
        "{} data reads; ROM data reads at or above ${from:06X} by instruction:",
        trace.len()
    );
    for (pc, pages) in by_pc {
        let total: u64 = pages.values().map(|p| p.0).sum();
        println!("  instruction ${pc:06X}: {total} reads");
        for (page, (count, lo, hi)) in pages.iter().take(6) {
            println!("      ${page:06X}: {count:>6} reads, ${lo:06X}-${hi:06X}");
        }
        if pages.len() > 6 {
            println!("      ... {} more pages", pages.len() - 6);
        }
    }
    Ok(())
}

fn level_sprites(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    let start = tiles.sprite_data_ptr();
    let list = sprites::read_sprites_at(rom, start)?;
    println!(
        "level {level:03X}: sprite data at {start}, {} bytes, memory {}, buoyancy {}, new system {}",
        list.len, list.header.memory, list.header.buoyancy, list.header.new_sprite_system
    );
    println!("  id  xb screen  x  y  extension");
    for s in &list.sprites {
        let ext: Vec<String> = s.extension.iter().map(|b| format!("{b:02X}")).collect();
        println!(
            "  {:02X}  {}  {:02X}     {:X}  {:02X} {}",
            s.id,
            s.extra_bits,
            s.screen,
            s.x,
            s.y,
            ext.join(" ")
        );
    }
    Ok(())
}

fn level_map16(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    let mut numbers: Vec<_> = tiles.map16.keys().copied().collect();
    numbers.sort_unstable();
    for n in numbers {
        let b = tiles.map16[&n].to_bytes();
        let hex: Vec<String> = b.iter().map(|x| format!("{x:02X}")).collect();
        println!("{n:04X}: {}", hex.join(" "));
    }
    Ok(())
}

fn level_wram(rom: &Rom, level: &str, addr: &str, len: usize) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    let start = u32::from_str_radix(addr.trim_start_matches('$'), 16).context("bad address")?;
    let Some(first) = RamAddr::checked(start) else {
        bail!("address must be in $7E0000-$7FFFFF");
    };
    let len = len.min((0x80_0000 - start) as usize);
    for (i, chunk) in tiles.ram.bytes(first, len).chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
        println!("${:06X}: {}", start as usize + i * 16, hex.join(" "));
    }
    Ok(())
}

fn level_tiles(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let tiles = expand::expand_level(rom, level)?;
    println!(
        "level {level:03X}: {} screens, mode ${:02X}, vertical {}",
        tiles.screens, tiles.level_mode, tiles.vertical
    );
    let (w, h) = tiles.size();
    for y in 0..h {
        let row: Vec<String> = (0..w)
            .map(|x| format!("{:03X}", tiles.tile_at(x, y)))
            .collect();
        println!("{}", row.join(" "));
    }
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
    if let Some(v) = rom.lunar_magic_version() {
        println!("lunar magic:     {v}");
    }
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
