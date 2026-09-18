//! What a capture gave up on without failing the level.

use std::fmt;

use crate::cpu::CpuError;

/// The pass of a capture that the CPU core could not finish. A crashing
/// custom sprite, or per-level code that is broken whatever runs it,
/// costs that pass its picture and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pass {
    /// The player's entrance frames; the level goes without a player.
    Player,
    /// The ROM's sprite loader, called with the camera here. The
    /// column's sprites become markers.
    SpriteLoader { camera: (i32, i32) },
    /// The drawing pass of sprite `id`, from the entry at this tile.
    Sprite { id: u8, x: i32, y: i32 },
    /// The shared pass of the entries that took no sprite slot, from the
    /// loader's camera.
    Slotless { camera: (i32, i32) },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub pass: Pass,
    pub error: CpuError,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pass {
            Pass::Player => write!(f, "player entrance")?,
            Pass::SpriteLoader { camera: (x, y) } => {
                write!(f, "sprite loader with the camera at ({x}, {y})")?
            }
            Pass::Sprite { id, x, y } => write!(f, "sprite {id:02X} at tile ({x}, {y})")?,
            Pass::Slotless { camera: (x, y) } => {
                write!(f, "slotless sprites with the camera at ({x}, {y})")?
            }
        }
        write!(f, ": {}", self.error)
    }
}
