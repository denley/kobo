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
    /// `TM`/`TS` mirrors (`$0D9D`/`$0D9E`): the layers on the main and
    /// sub screens (bit 0 layer 1, bit 1 layer 2, bit 2 layer 3, bit 4
    /// objects).
    pub main_screen: u8,
    pub sub_screen: u8,
    /// `CGADSUB` mirror (`$40`): bit 2 blends layer 3 with the subscreen
    /// (bit 7 subtracts instead of adding, bit 6 halves the result).
    pub color_math: u8,
}

impl Layer3 {
    /// Whether layer 3 pixels are blended with the subscreen.
    pub fn blends(&self) -> bool {
        self.color_math & 0x04 != 0
    }

    /// Whether layer 3 is the only background layer on the main screen,
    /// so it draws in front of the others whatever its tiles' priority.
    pub fn alone_on_main(&self) -> bool {
        self.main_screen & 0x03 == 0
    }

    /// Whether high-priority tiles go in front of everything (Mode 1's
    /// BG3 priority bit).
    pub fn high_priority_in_front(&self) -> bool {
        self.bg_mode & 0x08 != 0
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
