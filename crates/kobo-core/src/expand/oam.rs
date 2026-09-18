//! Reading the objects a frame drew out of the game's OAM image.

use crate::ram::{self, Ram};
use crate::video::SpriteObject;

/// Screen size the sprite engine draws within.
pub(super) const SCREEN_W: i32 = 256;
pub(super) const SCREEN_H: i32 = 224;
/// The Y position the game parks unused OAM objects at.
pub(super) const HIDDEN_Y: u8 = 0xF0;
/// Objects in OAM, and the bytes of the image: four per object, then the
/// size and X-high bits packed four objects to a byte.
pub(super) const OAM_OBJECTS: usize = 128;
pub(super) const OAM_LEN: usize = OAM_OBJECTS * 4 + OAM_OBJECTS / 4;

/// Small and large object dimensions for an `OBSEL` value.
pub fn object_sizes(object_select: u8) -> [(i32, i32); 2] {
    [
        [(8, 8), (16, 16)],
        [(8, 8), (32, 32)],
        [(8, 8), (64, 64)],
        [(16, 16), (32, 32)],
        [(16, 16), (64, 64)],
        [(32, 32), (64, 64)],
        [(16, 32), (32, 64)],
        [(16, 32), (32, 32)],
    ][(object_select >> 5) as usize]
}

/// The game's OAM image and the object it started writing at.
pub(super) fn read_oam(ram: &Ram) -> (Vec<u8>, usize) {
    (
        ram.bytes(ram::OAM, OAM_LEN),
        ram.u8(ram::OAM_ADDRESS) as usize / 2,
    )
}

/// Visible objects in an OAM image, front to back from object `first`,
/// in screen coordinates. Y `$F0` is the game's hidden marker; objects
/// entirely off the screen are dropped, and those wrapped past its bottom
/// are read as negative.
pub(super) fn screen_objects(
    oam: &[u8],
    first: usize,
    sizes: [(i32, i32); 2],
) -> Vec<SpriteObject> {
    let mut out = Vec::new();
    for offset in 0..OAM_OBJECTS {
        let object = (first + offset) % OAM_OBJECTS;
        let bytes = &oam[object * 4..object * 4 + 4];
        let high = oam[OAM_OBJECTS * 4 + object / 4] >> (2 * (object % 4));
        let large = high & 2 != 0;
        let (width, height) = sizes[large as usize];
        let x = bytes[0] as i32 - if high & 1 != 0 { 256 } else { 0 };
        let y = bytes[1];
        if y == HIDDEN_Y {
            continue;
        }
        let y = if y as i32 >= SCREEN_H {
            y as i32 - 256
        } else {
            y as i32
        };
        if x + width <= 0 || x >= SCREEN_W || y + height <= 0 || y >= SCREEN_H {
            continue;
        }
        out.push(SpriteObject {
            x,
            y,
            tile: bytes[2],
            attr: bytes[3],
            large,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oam_with(objects: &[(usize, u8, u8, u8, u8, u8)]) -> Vec<u8> {
        let mut oam = vec![0u8; OAM_LEN];
        for i in 0..OAM_OBJECTS {
            oam[i * 4 + 1] = HIDDEN_Y;
        }
        for &(i, x, y, tile, attr, high) in objects {
            oam[i * 4..i * 4 + 4].copy_from_slice(&[x, y, tile, attr]);
            oam[OAM_OBJECTS * 4 + i / 4] |= high << (2 * (i % 4));
        }
        oam
    }

    #[test]
    fn hidden_and_offscreen_objects_are_dropped() {
        let sizes = object_sizes(0x60); // 16x16 and 32x32
        let oam = oam_with(&[
            (0, 10, 20, 0x40, 0x30, 2),   // large, visible
            (1, 10, 0xF0, 0x40, 0x30, 2), // hidden marker
            (2, 0xF8, 30, 0x41, 0x00, 1), // x = -8, small: 8 px visible
            (3, 0xF0, 30, 0x41, 0x00, 1), // x = -16, small: gone
            (4, 0, 0xF8, 0x42, 0x00, 2),  // y = -8, large: visible
            (5, 0, 0xE0, 0x42, 0x00, 0),  // y = 224: below the screen
        ]);
        let got: Vec<_> = screen_objects(&oam, 0, sizes)
            .iter()
            .map(|o| (o.x, o.y, o.tile, o.attr, o.large))
            .collect();
        assert_eq!(
            got,
            [
                (10, 20, 0x40, 0x30, true),
                (-8, 30, 0x41, 0x00, false),
                (0, -8, 0x42, 0x00, true),
            ]
        );
    }

    #[test]
    fn objects_start_from_the_first_written_one() {
        let sizes = object_sizes(0x03); // 8x8 and 16x16, what SMW uses
        assert_eq!(sizes, [(8, 8), (16, 16)]);
        let oam = oam_with(&[(0, 1, 1, 1, 0, 0), (100, 2, 2, 2, 0, 0)]);
        let got: Vec<u8> = screen_objects(&oam, 100, sizes)
            .iter()
            .map(|o| o.tile)
            .collect();
        assert_eq!(got, [2, 1]);
    }
}
