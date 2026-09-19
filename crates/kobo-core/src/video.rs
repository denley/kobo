//! Video state captured from the game: what it uploaded, how a level's
//! layers are set up on entry, and the objects its sprite engine drew.
//! Fixed-screen boss arenas switch video modes during the frame, and
//! their collision tiles are not their visible artwork, so they carry a
//! scene of their own.

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
    /// The arena's window: it masks BG1 and the objects, and colour math
    /// (the back area colour added to a black backdrop) is prevented
    /// outside it, so the back area shows through the window only.
    pub window: Window,
    /// The objects of the first drawing pass, front to back in screen
    /// coordinates, including sprite-based arena walls and Bowser's floor.
    pub objects: Vec<SpriteObject>,
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
}

/// Each layer's bit in `TM`, `TS`, `TMW`, and `CGADSUB`, in the order the
/// renderer keeps its layers: BG1, BG2, BG3, objects.
pub const LAYER_BITS: [u8; 4] = [0x01, 0x02, 0x04, 0x10];

/// Window 1 over a fixed screen, as the game's HDMA drives it. Window 2
/// and the window logic registers are not modelled; SMW leaves them off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// Left and right edge (inclusive) per screen row, from the table the
    /// HDMA feeds to `WH0`/`WH1` (`$04A0`). A row with left past right is
    /// empty.
    pub rows: Vec<[u8; 2]>,
    /// `TMW`: the main-screen layers the window masks where it applies.
    pub main_mask: u8,
    /// `W12SEL`, `W34SEL`, and `WOBJSEL` mirrors (`$41`-`$43`), a nibble
    /// each for BG1 and BG2, BG3 and BG4, objects and the colour window:
    /// bit 1 enables window 1, bit 0 inverts it.
    pub select: [u8; 3],
}

impl Window {
    fn applies(&self, setting: u8, x: usize, y: usize) -> bool {
        let inside = self
            .rows
            .get(y)
            .is_some_and(|&[left, right]| (left as usize..=right as usize).contains(&x));
        setting & 2 != 0 && inside != (setting & 1 != 0)
    }

    /// Whether the window hides `layer` (an index into [`LAYER_BITS`]) on
    /// the main screen at a screen position.
    pub fn masks_main(&self, layer: usize, x: usize, y: usize) -> bool {
        let setting = [
            self.select[0],
            self.select[0] >> 4,
            self.select[1],
            self.select[2],
        ][layer];
        self.main_mask & LAYER_BITS[layer] != 0 && self.applies(setting, x, y)
    }

    /// Whether a screen position is inside the colour window, which
    /// `CGWSEL` clips and prevents colour math against.
    pub fn color(&self, x: usize, y: usize) -> bool {
        self.applies(self.select[2] >> 4, x, y)
    }
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
    /// `10` = inside, `01` = outside, `11` = always) applies to a pixel
    /// inside or outside the colour window. Without a window every pixel
    /// is outside.
    fn window_setting_applies(setting: u8, in_window: bool) -> bool {
        match setting & 3 {
            0 => false,
            1 => !in_window,
            2 => in_window,
            _ => true,
        }
    }

    /// Whether a pixel's main-screen colour is forced to black before the
    /// math.
    pub fn clips_to_black(&self, in_window: bool) -> bool {
        Self::window_setting_applies(self.math_select >> 6, in_window)
    }

    /// Whether colour math is switched off for a pixel.
    pub fn prevents_math(&self, in_window: bool) -> bool {
        Self::window_setting_applies(self.math_select >> 4, in_window)
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

impl SpriteObject {
    /// The object moved by (`dx`, `dy`): from screen to level coordinates,
    /// given the camera.
    pub fn translated(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// A level sprite entry that drew nothing, for the renderer to mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UndrawnSprite {
    /// Level tile position of the entry.
    pub x: usize,
    pub y: usize,
    /// Sprite number.
    pub id: u8,
}

/// The sprites of an ordinary level as the game draws them on their first
/// frame, front to back, plus the level sprite entries that produced no
/// graphics at all (generators, scroll commands, and the like).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpriteScene {
    pub objects: Vec<SpriteObject>,
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
    /// Entries with no graphics.
    pub undrawn: Vec<UndrawnSprite>,
    /// Objects that ride on layer 2 (the castle candle flames), positioned
    /// in layer 2 pixels. The game keeps eight bits of their position, so
    /// they repeat every 256 pixels along the layer.
    pub layer2_objects: Vec<SpriteObject>,
    /// Passes the CPU core gave up on; what they would have drawn is in
    /// `undrawn` instead.
    pub diagnostics: Vec<crate::expand::Diagnostic>,
}

/// Video memory and the registers that say how to read it, as level
/// preparation left them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct VideoMemory {
    /// VRAM: layer tiles at `$0000`, sprite tiles at `$C000`, tilemaps in
    /// between. The player's tile uploads are included, and in boss arenas
    /// the boss's.
    pub vram: Vec<u8>,
    /// Which VRAM bytes were actually written.
    pub vram_written: Vec<bool>,
    pub cgram: Vec<u8>,
    /// `BG1SC`-`BG4SC`: bits 7-2 are the tilemap's VRAM word address
    /// divided by `$400`, bit 1 selects 64 tiles tall, bit 0 selects 64
    /// tiles wide. Vanilla puts layer 1 at `$2000` and layer 2 at `$3000`,
    /// both 64x64; Lunar Magic uses `$3000` and `$3800`, 64x32.
    pub bg_sc: [u8; 4],
    /// `OBSEL`: object sizes and character base.
    pub object_select: u8,
}

impl VideoMemory {
    /// The palette as uploaded to CGRAM.
    pub fn palette(&self) -> crate::palette::Palette {
        crate::palette::Palette::from_cgram(&self.cgram)
    }
}

/// How a level is shown when it is entered.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LevelScene {
    /// Main and sub screen designation and colour math, which decide how
    /// the layers combine into the picture.
    pub screen: Screen,
    /// Layer 1 position the level was entered at (`$1A`/`$1C`) and the
    /// layer 2 position the first camera update derived for it (`$1E`/
    /// `$20`). The two differ when the level's layer 2 scroll settings
    /// offset or slow the layer (parallax); the renderer draws layer 2
    /// where this camera sees it.
    pub camera: [u16; 2],
    pub layer2_position: [u16; 2],
    /// Layer 3 position and scroll behaviour, when the level shows
    /// layer 3 on either screen in Mode 1 (every ordinary level; boss
    /// arenas draw theirs into `boss`).
    pub layer3: Option<Layer3>,
    /// The player's OAM objects at the level's entrance, in level
    /// coordinates, as the game draws him once any entrance action (pipe,
    /// cannon pipe, door) has finished. Empty for boss arenas, whose
    /// drawing pass already includes him.
    pub player: Vec<SpriteObject>,
    /// Video-mode bands installed by the ROM's boss NMI/IRQ handlers.
    pub boss: Option<BossScene>,
}

impl LevelScene {
    /// How far layer 2 content is displaced from the layer 1 grid, in
    /// pixels: a layer 2 tile at column `c` shows at level x
    /// `c * 16 + offset[0]`. Zero when both layers scroll together.
    pub fn layer2_offset(&self) -> [i32; 2] {
        std::array::from_fn(|axis| {
            self.camera[axis].wrapping_sub(self.layer2_position[axis]) as i16 as i32
        })
    }
}
