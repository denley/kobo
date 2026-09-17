//! Video state used by the fixed-screen boss arenas. Ordinary levels
//! render their full object grids; these rooms switch video modes during
//! the frame and their collision tiles are not their visible artwork.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode7 {
    pub matrix: [i16; 4],
    pub center: [u16; 2],
    pub scroll: [u16; 2],
    pub control: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layer1 {
    pub mode: u8,
    pub tilemap: u8,
    /// Byte address of the character data in VRAM.
    pub character_base: u16,
    pub scroll: [u16; 2],
    pub mode7: Mode7,
}

/// First visible image row governed by a set of video registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub start: usize,
    pub layer: Layer1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BossScene {
    pub bands: Vec<Band>,
    /// The back-area colour is visible through the window; outside it
    /// the backdrop is black. This also masks BG1 and objects.
    pub backdrop_window: Vec<[u8; 2]>,
    /// Packed OAM from the first drawing pass, including sprite-based
    /// arena walls and Bowser's floor.
    pub oam: Vec<u8>,
    pub object_select: u8,
    pub first_object: usize,
}

/// Layer 3 as level preparation left it: where the game scrolled it for
/// the entry camera, and how it follows the camera from there. The
/// tilemap itself is in the captured VRAM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layer3 {
    /// `BG3HOFS`/`BG3VOFS` as the IRQ handler applies them below the
    /// status bar (`$22`/`$24`).
    pub position: [u16; 2],
    /// Layer 1 position the level was entered at (`$1A`/`$1C`).
    pub camera: [u16; 2],
    /// How far the layer moves for a 16-pixel camera move on each axis:
    /// 0 for a screen-fixed layer, 8 for half-speed parallax, 16 for a
    /// layer that stays put on the level.
    pub scroll_per_16: [i32; 2],
    /// `BG3SC`: tilemap base and size.
    pub tilemap: u8,
    /// Byte address of the 2bpp character data in VRAM.
    pub character_base: u16,
    /// `BGMODE` mirror (`$3E`): bit 3 puts high-priority layer 3 tiles
    /// in front of everything.
    pub bg_mode: u8,
}

impl Layer3 {
    /// Whether high-priority tiles go in front of everything (Mode 1's
    /// BG3 priority bit).
    pub fn high_priority_in_front(&self) -> bool {
        self.bg_mode & 0x08 != 0
    }
}

/// Screen designation and colour math as level preparation left them.
/// The level mode tables in `LoadLevel` (`LevMainScrnTbl`, `LevSubScrnTbl`,
/// `LevCGADSUBtable` at `$0581E0`-`$05823F`) choose them, and sprites such
/// as the spotlight change them afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    /// `TM` mirror (`$0D9D`): layers on the main screen (bit 0 layer 1,
    /// bit 1 layer 2, bit 2 layer 3, bit 4 objects).
    pub main: u8,
    /// `TS` mirror (`$0D9E`): layers on the subscreen.
    pub sub: u8,
    /// `CGADSUB` mirror (`$40`): which main-screen layers take part in
    /// colour math (bits as above, bit 5 the backdrop), bit 6 halves the
    /// result, bit 7 subtracts instead of adding.
    pub color_math: u8,
    /// `CGWSEL` mirror (`$44`): bit 1 uses the subscreen pixel as the
    /// operand (falling back to the fixed colour where the subscreen is
    /// transparent) instead of the fixed colour; bits 7-6 clip the main
    /// colour to black and bits 5-4 prevent colour math, each never (0),
    /// outside the colour window (1), inside it (2), or always (3).
    pub math_select: u8,
    /// `COLDATA`: the fixed colour, which the game keeps at the level's
    /// back area colour (`$0701`).
    pub fixed_color: crate::palette::Color15,
}

impl Screen {
    /// The vanilla setup of most level modes: layers 1 and 3 with objects
    /// on the main screen, layer 2 on the subscreen, added to the backdrop
    /// and layer 3 wherever they are.
    pub fn vanilla(fixed_color: crate::palette::Color15) -> Self {
        Self {
            main: 0x15,
            sub: 0x02,
            color_math: 0x24,
            math_select: 0x02,
            fixed_color,
        }
    }

    /// Whether a `CGWSEL` window-relative setting (clip or prevent, bits
    /// `10` = inside, `01` = outside, `11` = always) applies. No colour
    /// window is modelled, so "inside" never applies and "outside" always
    /// does.
    fn window_setting_applies(setting: u8) -> bool {
        matches!(setting & 3, 1 | 3)
    }

    /// Whether main-screen colours are forced to black before the math.
    pub fn clips_to_black(&self) -> bool {
        Self::window_setting_applies(self.math_select >> 6)
    }

    /// Whether colour math is switched off for every pixel.
    pub fn prevents_math(&self) -> bool {
        Self::window_setting_applies(self.math_select >> 4)
    }
}

/// One OAM object captured from the game's sprite engine, in level pixel
/// coordinates (which may be negative or extend past the level edge).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteObject {
    pub x: i32,
    pub y: i32,
    /// OAM character number.
    pub tile: u8,
    /// OAM attribute byte: `vhoopppN` (flips, priority, palette, name
    /// table).
    pub attr: u8,
    /// Uses the larger of the two `OBSEL` sizes.
    pub large: bool,
}

/// The sprites of an ordinary level as the game draws them on their first
/// frame, front to back, plus the level sprite entries that produced no
/// graphics at all (generators, scroll commands, and the like).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpriteScene {
    pub objects: Vec<SpriteObject>,
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
    /// Level tile positions and sprite numbers of entries with no graphics.
    pub undrawn: Vec<(usize, usize, u8)>,
}
