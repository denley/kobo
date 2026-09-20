//! Loading a level by running the ROM's own loader, phase by phase.

use super::machine::{Call, Machine};
use super::tiles::{
    GRID_LEN, LAYER2_TILEMAP_LEN, LevelTiles, SCREEN_COLS, SCREEN_LEN, SCREEN_ROWS,
};
use super::{ExpandError, LoadedLevel, boss, layer3, map16, player, routines};
use crate::level::{self, Layer2Kind, LevelMode};
use crate::palette::Color15;
use crate::ram::{self, Ram};
use crate::rom::Rom;
use crate::video::{LevelScene, Screen, VideoMemory};

/// Instruction limit for the reset code, which uploads the SPC engine.
const RESET_STEP_LIMIT: u64 = 200_000_000;

/// Runs the ROM's level loader and level preparation for `level`.
pub fn expand_level(rom: &Rom, level: u16) -> Result<LoadedLevel, ExpandError> {
    expand_level_traced(rom, level, false).map(|(t, _)| t)
}

/// A data read: (address of the reading instruction, address read).
pub type ReadTrace = Vec<(u32, u32)>;

/// Like [`expand_level`], optionally recording every data read the
/// loader made after reset.
pub fn expand_level_traced(
    rom: &Rom,
    level: u16,
    trace: bool,
) -> Result<(LoadedLevel, Option<ReadTrace>), ExpandError> {
    let header = level::read_primary_header(rom, level)?;
    let mut machine = Machine::new(rom, level);
    boot(&mut machine)?;
    if trace {
        machine.cpu.trace_data_reads = Some(Vec::new());
    }
    let expanded = load_level(&mut machine)?;
    prepare_level(&mut machine)?;
    let trace = machine.cpu.trace_data_reads.take();
    // Before any drawing pass: a Mode 7 arena's IRQ handler changes the
    // layers between the status bar and the playfield.
    let [main, sub] = machine.bus.screen_layers;

    let boss = boss::capture_boss_scene(&mut machine)?;
    let ram = &machine.bus.ram;
    let camera = [ram.u16(ram::LAYER1_X), ram.u16(ram::LAYER1_Y)];
    let layer2_position = [ram.u16(ram::LAYER2_X), ram.u16(ram::LAYER2_Y)];
    let screen = Screen {
        main,
        sub,
        color_math: ram.u8(ram::COLOR_MATH),
        math_select: ram.u8(ram::COLOR_MATH_SELECT),
        fixed_color: Color15(ram.u16(ram::BACKGROUND_COLOR)),
    };
    let mut diagnostics = Vec::new();
    let (layer3, player) = if boss.is_some() {
        // The arena's drawing pass already includes the player.
        (None, Vec::new())
    } else {
        (
            layer3::capture_layer3(&mut machine)?,
            player::capture_player(&mut machine, &mut diagnostics),
        )
    };
    let lunar_magic = rom.lunar_magic_version().is_some();
    let (bg_map16, layer2_screen_len) = match &expanded.layer2_tilemap {
        Some(planes) => map16::read_bg_map16(&mut machine, planes)?,
        None => (Vec::new(), SCREEN_LEN),
    };
    let map16 = map16::lookup_map16(&mut machine, lunar_magic)?;
    let pipe_map16 = (!lunar_magic).then(|| map16::read_pipe_map16(&mut machine.bus));
    let bus = machine.bus;
    let tiles = LevelTiles {
        level,
        header,
        level_mode: LevelMode(bus.ram.u8(ram::LEVEL_MODE)),
        object_tileset: bus.ram.u8(ram::OBJECT_TILESET),
        vertical: expanded.vertical,
        screens: expanded.screens,
        rows: expanded.rows,
        low: bus.ram.bytes(ram::TILES_LOW, GRID_LEN),
        high: bus.ram.bytes(ram::TILES_HIGH, GRID_LEN),
        layer2_tilemap: expanded.layer2_tilemap,
        layer2_screen_len,
        map16,
        pipe_map16,
        bg_map16,
    };
    let level = LoadedLevel {
        tiles,
        video: VideoMemory {
            vram: bus.vram,
            vram_written: bus.vram_written,
            cgram: bus.cgram,
            bg_sc: bus.bg_sc,
            object_select: bus.object_select,
        },
        scene: LevelScene {
            screen,
            camera,
            layer2_position,
            layer3,
            player,
            boss,
        },
        ram: bus.ram,
        diagnostics,
    };
    Ok((level, trace))
}

/// The first `len` bytes of GFX file `index` (`00` to `31`) as the ROM's
/// own code decompresses it, whatever routine a hack has put in the
/// game's place. This is what [`crate::gfx`]'s decoders are checked
/// against; a level's graphics always come this way.
pub fn decompress_gfx_file(rom: &Rom, index: u8, len: usize) -> Result<Vec<u8>, ExpandError> {
    let mut machine = Machine::new(rom, 0);
    let failed = |source| ExpandError::Gfx { index, source };
    machine
        .run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)
        .map_err(|error| match error {
            ExpandError::Cpu { source, .. } => failed(source),
            other => other,
        })?;
    machine
        .try_call(Call::jsl(routines::DECOMPRESS_GFX_FILE).index_y(index as u16))
        .map_err(failed)?;
    Ok(machine.bus.ram.bytes(ram::GFX_BUFFER, len))
}

/// Brings the machine to where the game is when a level load starts.
/// The reset code runs up to the main loop: it builds the RAM-resident
/// OAM reset routine, uploads the SPC engine (against a stub that echoes
/// the handshake), and clears memory; patches hooked into it run as well.
/// Then come the layer 3 tiles, which the game uploads once on the
/// "Nintendo Presents" screen and which survive every level load.
fn boot(machine: &mut Machine) -> Result<(), ExpandError> {
    machine.run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)?;
    machine.call(Call::jsr(routines::CLEAR_LAYER3))?;
    machine.call(Call::jsr(routines::UPLOAD_LAYER3_GFX))
}

/// What has to be read right after `LoadLevel`, before level preparation
/// overwrites it.
struct Expanded {
    screens: usize,
    vertical: bool,
    rows: usize,
    layer2_tilemap: Option<(Vec<u8>, Vec<u8>)>,
}

/// Game mode `$11`: resolves the level's header pointers, places the
/// player and the camera at the entrance, and expands the level's
/// objects into the tile grid.
fn load_level(machine: &mut Machine) -> Result<Expanded, ExpandError> {
    // Enter the level the way a screen exit on screen 0 would. The
    // overworld path cannot express every level number through `$0109`,
    // loads the "No Yoshi" entrance intro room for castle and ghost house
    // tilesets, and is rerouted by some Lunar Magic versions. The high
    // byte is given in both the vanilla form (the player's submap) and
    // Lunar Magic's exit table form.
    let [lo, hi] = machine.level.to_le_bytes();
    let ram = &mut machine.bus.ram;
    ram.set_u8(ram::SUBLEVEL_COUNT, 1);
    ram.set_u8(ram::EXIT_TABLE_LOW, lo);
    ram.set_u8(ram::EXIT_TABLE_HIGH, 0x04 | hi);
    ram.set_u8(ram::OW_PLAYER_SUBMAP, hi);
    // Run each phase with the game mode the real machine would be in.
    ram.set_u8(ram::GAME_MODE, 0x11);
    machine.call(Call::jsl(routines::LOAD_HEADER_POINTERS))?;
    // Game mode $11 seeds the camera update's previous positions from the
    // entrance, sets the player up, and places the layers for the entry
    // camera before it loads anything: `LOAD_LEVEL_DATA` ends by spawning
    // the sprites around that camera. The screen count is still the
    // maximum then, and vertical scrolling at will is on, which lets the
    // update bring the camera to the player at once where the header's
    // position would not show him (vertical level `12A` starts 192
    // pixels higher for it).
    let ram = &mut machine.bus.ram;
    for i in 0..ram::LAYER_POSITIONS_LEN {
        let value = ram.u8_at(ram::LAYER1_X, i);
        ram.set_u8_at(ram::NEXT_LAYER1_X, i, value);
    }
    machine.call(Call::jsr(routines::INIT_LEVEL_RAM))?;
    machine.bus.ram.set_u8(ram::LAST_SCREEN_HORIZ, 0x20);
    machine.call(Call::jsr(routines::INIT_LAYER2_SCROLL))?;
    machine.bus.ram.set_u8(ram::SCROLL_AT_WILL, 1);
    machine.call(Call::jsl(routines::UPDATE_CAMERA))?;
    machine.call(Call::jsl(routines::LOAD_LEVEL_DATA))?;
    let ram = &machine.bus.ram;
    let vertical = ram.u8(ram::SCREEN_MODE) & 0x01 != 0;
    Ok(Expanded {
        // Boss preparation reuses the screen-count byte (level $1C7 ends
        // with $FF). Read the length while it still describes the grid.
        screens: ram.u8(ram::SCREENS) as usize,
        vertical,
        rows: level_rows(ram, vertical),
        // Level preparation decompresses GFX files into `$7EAD00`, and
        // Lunar Magic's 4bpp files overrun the vanilla 3bpp buffer into
        // the background at `$7EB900`. The game has uploaded the tilemap
        // to VRAM by then, so it does not care; we do.
        layer2_tilemap: (LevelMode(ram.u8(ram::LEVEL_MODE)).layer2() == Layer2Kind::Background)
            .then(|| {
                (
                    ram.bytes(ram::LAYER2_TILEMAP_LOW, LAYER2_TILEMAP_LEN),
                    ram.bytes(ram::LAYER2_TILEMAP_HIGH, LAYER2_TILEMAP_LEN),
                )
            }),
    })
}

/// All of game mode `$12`: this is what draws boss arenas, sets up layer
/// 3, and uploads GFX and palettes. Then the camera update the level loop
/// starts with.
fn prepare_level(machine: &mut Machine) -> Result<(), ExpandError> {
    let ram = &mut machine.bus.ram;
    ram.fill(ram::LOADED_GFX_FILES, ram::LOADED_GFX_FILES_LEN, 0xFF);
    machine.call(Call::jsr(routines::DECOMPRESS_PLAYER_GFX))?;
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x12);
    machine.call(Call::jsr(routines::PREPARE_LEVEL))?;
    // The level loop updates the camera before anything is shown, which
    // is what settles the layer 2 position: code a hack runs during
    // preparation may have moved it (Super Hark Bros 2 level `00A` leaves
    // it at `$5D`; the update derives `$C0` from the camera again, which
    // is where the game had uploaded the background for).
    machine.call(Call::jsl(routines::UPDATE_CAMERA))
}

/// Rows per screen of the loaded level. Lunar Magic 3's expanded level
/// format stores a horizontal level's height in `$13D7`; vanilla and
/// older Lunar Magic ROMs leave it zero. Anything that does not describe
/// whole rows fitting the planes is treated as the vanilla 27.
fn level_rows(ram: &Ram, vertical: bool) -> usize {
    if vertical {
        return 16;
    }
    let height = ram.u16(ram::LEVEL_HEIGHT) as usize;
    match height / 16 {
        rows if height.is_multiple_of(16) && rows > 0 && rows * SCREEN_COLS <= GRID_LEN => rows,
        _ => SCREEN_ROWS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ram::RamMap;

    #[test]
    fn level_rows_come_from_the_reported_height() {
        let mut ram = Ram::new(RamMap::Vanilla);
        assert_eq!(level_rows(&ram, false), SCREEN_ROWS);
        assert_eq!(level_rows(&ram, true), 16);
        ram.set_u16(ram::LEVEL_HEIGHT, 0x0280);
        assert_eq!(level_rows(&ram, false), 40);
        ram.set_u16(ram::LEVEL_HEIGHT, 0x0288); // not whole rows
        assert_eq!(level_rows(&ram, false), SCREEN_ROWS);
    }
}
