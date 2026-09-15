//! A 65816 interpreter for running the ROM's own routines headlessly.
//!
//! This is not a console emulator: there is no timing, no PPU, and no
//! interrupts. It exists so the level loader, object expansion, and
//! similar pure-CPU routines can run on the real ROM code, which keeps
//! Kobo faithful to patched ROMs without re-implementing every routine.

mod core65816;
pub mod smw_bus;

pub use core65816::{Bus, Cpu, CpuError, Flags};
