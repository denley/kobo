//! Kobo core library.
//!
//! Everything the editor, CLI, and scripting API can do lives here. The
//! shells on top of this crate are meant to be thin.

pub mod addr;
pub mod config;
pub mod rom;

pub use addr::{MapError, Mapping, PcAddr, SnesAddr};
pub use rom::{Rom, RomError, RomIdentity};
