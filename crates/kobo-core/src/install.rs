//! Kobo's ROM-side code: the Asar patches that give a build the code Lunar
//! Magic's layout needs, written clean-room (docs/step-2.md, "Clean room").
//! The patches are sources in `asm/`, assembled into the ROM through
//! [`crate::asar`] at build time.
//!
//! Lunar Magic installs its one-time set only into a ROM whose gate
//! `$06F600` is `$FF`, and wipes its tables when it does; a build that
//! writes anything in Lunar Magic's layout sets the gate and provides the
//! whole set itself (docs/lunar-magic-install.md). The patches here are
//! that set, piece by piece; until it is complete, no build uses them.

use crate::asar::{Asar, AsarError, Patch, Patched};
use crate::rom::Rom;

/// The patches, by file name under `asm/lunar-magic/`, in the order they
/// apply.
pub const LUNAR_MAGIC: &[(&str, &str)] =
    &[("map16.asm", include_str!("../asm/lunar-magic/map16.asm"))];

/// Applies the Lunar Magic layout patches to a ROM.
pub fn apply_lunar_magic(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    let mut rom = Rom::from_bytes(rom.data().to_vec())?;
    for (name, source) in LUNAR_MAGIC {
        let Patched { rom: patched, .. } = asar.patch(&rom, &Patch::source(name, *source))?;
        rom = patched;
    }
    Ok(rom)
}
