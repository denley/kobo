//! The player as the game draws him at the level's entrance.

use super::diagnostics::{Diagnostic, Pass};
use super::load_flags::LoadFlags;
use super::machine::{Call, Machine};
use super::oam::{self, HIDDEN_Y, OAM_OBJECTS};
use super::routines;
use crate::cpu::CpuError;
use crate::ram;
use crate::video::SpriteObject;

/// Most frames the player pass runs waiting for an entrance action to end.
const PLAYER_FRAMES: usize = 128;
/// The OAM slots `DrawMarioAndYoshi` (`CODE_01EA70`) fills, at `$0300`-
/// `$031F`: the player and, when he rides, Yoshi. Everything else the game
/// draws in a frame (cluster sprites such as the castle candle flames and
/// the ghost house Boo ceilings the loader spawned) uses other slots.
const PLAYER_OAM_SLOTS: std::ops::Range<usize> = 64..72;

/// The player at the entrance: one level frame after another from the
/// prepared state, with every sprite slot cleared and every level sprite
/// marked as already loaded so nothing else spawns, until his entrance
/// action (`$71`) has finished. The NMI's player tile and palette upload
/// then runs so the objects' graphics are in VRAM and CGRAM (the bus keeps
/// those; RAM is restored). Only his own OAM slots are read: the cluster
/// sprites the loader spawned are still drawn in this pass, and the
/// sprite passes already capture them. The objects come back in level
/// coordinates from the camera the pass ended with.
///
/// The level loop runs the hack's per-level code, which can be broken
/// (Invictus level 136 returns with `RTS` from a `JSL`). The level itself
/// has loaded by then; it goes without a player, and says so in
/// `diagnostics`.
pub(super) fn capture_player(
    machine: &mut Machine,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SpriteObject> {
    let saved = machine.bus.ram.clone();
    let objects = match enter(machine) {
        Ok(objects) => objects,
        Err(error) => {
            diagnostics.push(Diagnostic {
                pass: Pass::Player,
                error,
            });
            Vec::new()
        }
    };
    machine.bus.ram = saved;
    objects
}

fn enter(machine: &mut Machine) -> Result<Vec<SpriteObject>, CpuError> {
    let load_flags = LoadFlags::detect(&mut machine.bus);
    let ram = &mut machine.bus.ram;
    ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
    load_flags.fill(ram, 1);
    ram.set_u8(ram::SPRITE_GENERATOR, 0);
    ram.set_u8(ram::GAME_MODE, 0x14);
    for _ in 0..PLAYER_FRAMES {
        machine.try_call(Call::jsr(routines::DRAW_LEVEL_FRAME))?;
        if machine.bus.ram.u8(ram::PLAYER_ANIMATION) == 0 {
            break;
        }
    }
    machine.try_call(Call::jsr(routines::UPLOAD_PLAYER_TILES))?;
    let ram = &machine.bus.ram;
    let (mut image, first) = oam::read_oam(ram);
    for slot in (0..OAM_OBJECTS).filter(|slot| !PLAYER_OAM_SLOTS.contains(slot)) {
        image[slot * 4 + 1] = HIDDEN_Y;
    }
    let camera = (
        ram.u16(ram::LAYER1_X) as i16 as i32,
        ram.u16(ram::LAYER1_Y) as i16 as i32,
    );
    let sizes = oam::object_sizes(machine.bus.object_select);
    Ok(oam::screen_objects(&image, first, sizes)
        .into_iter()
        .map(|object| object.translated(camera.0, camera.1))
        .collect())
}
