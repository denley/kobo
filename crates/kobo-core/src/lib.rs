//! Kobo core library.
//!
//! Everything the editor, CLI, and scripting API can do lives here. The
//! shells on top of this crate are meant to be thin.

pub mod addr;
pub mod asar;
pub mod build;
pub mod compress;
pub mod config;
pub mod cpu;
pub mod expand;
pub mod gfx;
pub mod image;
pub mod import;
pub mod level;
pub mod map16;
pub mod operation;
pub mod palette;
pub mod ram;
pub mod rats;
pub mod render;
pub mod rom;
pub mod source;
pub mod sprites;
pub mod video;

pub use addr::{MapError, Mapping, PcAddr, SnesAddr};
pub use rom::{Rom, RomError, RomIdentity};
