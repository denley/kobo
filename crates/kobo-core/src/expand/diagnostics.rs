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

/// One line per distinct error, naming the first pass it stopped and how
/// many others: broken per-level code fails every pass the same way.
pub fn summarize(diagnostics: &[Diagnostic]) -> Vec<String> {
    let mut reported: Vec<&CpuError> = Vec::new();
    let mut lines = Vec::new();
    for diagnostic in diagnostics {
        if reported.contains(&&diagnostic.error) {
            continue;
        }
        reported.push(&diagnostic.error);
        let passes = diagnostics
            .iter()
            .filter(|other| other.error == diagnostic.error)
            .count();
        lines.push(match passes {
            1 => diagnostic.to_string(),
            2 => format!("{diagnostic} (and 1 more pass)"),
            n => format!("{diagnostic} (and {} more passes)", n - 1),
        });
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_error_is_summarized_once() {
        let brk = CpuError::Brk {
            pb: 0x93,
            pc: 0x9D9F,
        };
        let cop = CpuError::Cop {
            pb: 0x01,
            pc: 0x8000,
        };
        let diagnostic = |pass, error: &CpuError| Diagnostic {
            pass,
            error: error.clone(),
        };
        let lines = summarize(&[
            diagnostic(Pass::Player, &brk),
            diagnostic(
                Pass::Sprite {
                    id: 0xB9,
                    x: 1,
                    y: 19,
                },
                &cop,
            ),
            diagnostic(Pass::Slotless { camera: (16, 0) }, &brk),
        ]);
        assert_eq!(
            lines,
            [
                "player entrance: BRK at $93:9D9F (and 1 more pass)",
                "sprite B9 at tile (1, 19): COP at $01:8000",
            ]
        );
        assert!(summarize(&[]).is_empty());
    }
}
