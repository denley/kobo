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
