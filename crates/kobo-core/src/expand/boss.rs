//! The video-mode bands and object artwork of a Mode 7 boss arena.

use super::ExpandError;
use super::machine::{Call, Machine};
use super::oam::{self, OAM_LEN, SCREEN_H};
use super::routines;
use crate::cpu::smw_bus::SmwBus;
use crate::ram;
use crate::video::{Band, BossScene, Layer1};

/// Instruction limit for a stretch of an interrupt handler.
const HANDLER_STEP_LIMIT: u64 = 100_000;
/// What the boss IRQ handler expects in A: `TIMEUP` with its IRQ flag set.
const IRQ_PENDING: u16 = 0x81;

fn layer1(bus: &SmwBus) -> Layer1 {
    Layer1 {
        mode: bus.bg_mode,
        tilemap: bus.bg_sc[0],
        character_base: bus.bg_character_base[0],
        scroll: bus.bg_scroll[0],
        mode7: bus.mode7,
    }
}

/// Run just the video-register portions of the boss interrupt handlers.
/// The ROM chooses its tilemap, graphics base, Mode 7 transform, and IRQ
/// scanlines. One drawing pass populates the sprite-based arena artwork;
/// its RAM changes are isolated from the captured collision grid. `None`
/// for a level that is not an arena.
pub(super) fn capture_boss_scene(machine: &mut Machine) -> Result<Option<BossScene>, ExpandError> {
    let command = machine.bus.ram.u8(ram::IRQ_NMI_COMMAND);
    if command & 0x80 == 0 {
        return Ok(None);
    }
    let saved = machine.bus.ram.clone();
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x14);
    machine.call(Call::jsr(routines::DRAW_LEVEL_FRAME))?;
    let (oam, first_object) = oam::read_oam(&machine.bus.ram);
    debug_assert_eq!(oam.len(), OAM_LEN);
    machine.call(Call::jsr(routines::UPLOAD_PLAYER_TILES))?;
    if command & 0x40 != 0 {
        machine.call(Call::jsr(routines::UPLOAD_BOSS_TILES))?;
    }
    let single_band = command & 1 != 0;
    let stop = if single_band {
        routines::EXIT_IRQ
    } else {
        routines::SET_STATUS_BAR_IRQ
    };
    machine.run_until(routines::MODE7_NMI_REGISTERS, stop, HANDLER_STEP_LIMIT)?;
    let mut bands = vec![Band {
        start: 0,
        layer: layer1(&machine.bus),
    }];
    if !single_band {
        // At the first stop Y holds the status-bar/ceiling IRQ line.
        let first_line = machine.cpu.y as usize;
        machine.bus.ram.set_u8(ram::IRQ_TYPE, 0);
        let mut next_band = |machine: &mut Machine, start: usize| {
            machine.cpu.a = IRQ_PENDING;
            machine.resume_until(routines::BOSS_IRQ, routines::EXIT_IRQ, HANDLER_STEP_LIMIT)?;
            bands.push(Band {
                start,
                layer: layer1(&machine.bus),
            });
            Ok::<_, ExpandError>(())
        };
        next_band(machine, first_line)?;
        if machine.bus.interrupt_enable & 0x20 != 0 {
            let floor_line = machine.bus.irq_scanline as usize;
            next_band(machine, floor_line)?;
        }
    }
    let backdrop_window = machine
        .bus
        .ram
        .bytes(ram::WINDOW_TABLE, SCREEN_H as usize * 2)
        .as_chunks::<2>()
        .0
        .to_vec();
    machine.bus.ram = saved;
    Ok(Some(BossScene {
        bands,
        backdrop_window,
        oam,
        object_select: machine.bus.object_select,
        first_object,
    }))
}
