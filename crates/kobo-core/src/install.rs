//! Kobo's ROM-side code: the Asar patches that give a build the code Lunar
//! Magic's layout needs, written clean-room (docs/step-2.md, "Clean room").
//! The patches are sources in `asm/`, assembled into the ROM through
//! [`crate::asar`] at build time.
//!
//! Lunar Magic decides piece by piece whether its install is in a ROM, and
//! a save installs a missing piece over whatever is at its sites and
//! resets its tables (docs/lunar-magic-install.md). A build that writes a
//! piece's tables installs Kobo's code for that piece, with Lunar Magic's
//! check for it met. So far that is the Map16 routine and the acts-like
//! chain in bank `$06`, whose check is `$06F600`, for Map16 pages past 1;
//! the placed objects (`22`, `23`, `27`, `29`), whose check is on Lunar
//! Magic's own code, so its first save puts its own in their place; the
//! level number (`$0EF550` occupied); and backgrounds and BG Map16 (a `JML`
//! at `$0EF519`, a `JSL` at `$058DA4`); custom palettes; screen exits in
//! Lunar Magic's format; and the entrance settings, whose check is the
//! `JSL` at `$05DA17`.

use crate::asar::{Asar, AsarError, Patch, Patched};
use crate::rom::Rom;

/// The patches, by file name under `asm/lunar-magic/`, in the order they
/// apply.
pub const LUNAR_MAGIC: &[(&str, &str)] = &[
    ("map16.asm", include_str!("../asm/lunar-magic/map16.asm")),
    (
        "actslike.asm",
        include_str!("../asm/lunar-magic/actslike.asm"),
    ),
    (
        "objects.asm",
        include_str!("../asm/lunar-magic/objects.asm"),
    ),
    ("level.asm", include_str!("../asm/lunar-magic/level.asm")),
    (
        "background.asm",
        include_str!("../asm/lunar-magic/background.asm"),
    ),
    (
        "palette.asm",
        include_str!("../asm/lunar-magic/palette.asm"),
    ),
    ("exits.asm", include_str!("../asm/lunar-magic/exits.asm")),
    (
        "entrance.asm",
        include_str!("../asm/lunar-magic/entrance.asm"),
    ),
];

/// Applies the Lunar Magic layout patches to a ROM.
pub fn apply_lunar_magic(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    let mut rom = Rom::from_bytes(rom.data().to_vec())?;
    for (name, source) in LUNAR_MAGIC {
        let Patched { rom: patched, .. } = asar.patch(&rom, &Patch::source(name, *source))?;
        rom = patched;
    }
    Ok(rom)
}
