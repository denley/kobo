//! Layer 1 and layer 2 object data.
//!
//! Object data is a five-byte header (the primary header on layer 1; on
//! layer 2 the game skips it) and then objects in drawing order, ended by
//! a first byte of `$FF`. An object is `NBBYYYYY bbbbXXXX` and more:
//! `BBbbbb` is its number, `YYYYY` and `XXXX` its place on the current
//! screen, and `N` moves the current screen on by one before it is placed.
//! Standard objects have a settings byte. Object `00` is an extended
//! object whose number is the third byte: `00` is a screen exit (a fourth
//! byte, the destination), `01` a screen jump, which sets the current
//! screen. `LoadLevelData` (`$0586F1`) reads them.
//!
//! On a vertical layer the game swaps the two nibbles holding the place
//! (`CODE_0585D8`) for every object but extended objects `00` and `01`:
//! the first byte's low nibble is the column, the second's the row, and
//! bit 4 of the first byte the right half of the 32-tile-wide screen.
//!
//! Lunar Magic adds objects `22`-`28` and `2D`, some longer than three
//! bytes, a five-byte screen exit (extended `02`), a vertical part to the
//! screen jump for levels taller than one screen (bits 0-3 of its second
//! byte, in units of 32 rows), and extended `03`, the same jump with the
//! two parts the other way round, for level heights past 16 of those.
//! Layouts are from the community's documentation of the format.
//!
//! [`decode`] turns the screens into absolute tile positions and drops
//! the screen jumps; [`encode`] chooses its own, so decoding what it
//! encodes gives the same objects, though not always the same bytes.

use thiserror::Error;

/// A first byte that ends the data.
const END: u8 = 0xFF;
/// The new-screen bit of an object's first byte.
const NEW_SCREEN: u8 = 0x80;

const EXT_SCREEN_EXIT: u8 = 0x00;
const EXT_SCREEN_JUMP: u8 = 0x01;
const EXT_LONG_EXIT: u8 = 0x02;
const EXT_TALL_SCREEN_JUMP: u8 = 0x03;

/// Rows in one unit of a screen jump's vertical part.
const JUMP_ROWS: u16 = 32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObjectError {
    #[error("object data ends at byte {0} without a terminator")]
    Truncated(usize),
    #[error("object {index} is at ({x}, {y}), which a {layout:?} layer cannot place")]
    Unplaceable {
        index: usize,
        x: u16,
        y: u16,
        layout: Layout,
    },
    #[error("object {index} has number {number:02X}, which cannot be written as a {kind}")]
    BadNumber {
        index: usize,
        number: u8,
        kind: &'static str,
    },
    #[error("object {index} holds {len} bytes; a {kind} holds {expected}")]
    BadLength {
        index: usize,
        len: usize,
        kind: &'static str,
        expected: &'static str,
    },
}

/// How a layer's objects are placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Screens side by side, 16 tiles wide.
    Horizontal,
    /// Screens stacked, 16 tiles tall and 32 wide.
    Vertical,
}

/// What a screen jump may say about the vertical part of the position.
/// The game ignores it; Lunar Magic 3's expanded level heights read it.
/// Extended objects `02` and `03` are always read as Lunar Magic's: the
/// game's handlers for them are null pointers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Jumps {
    /// Screen jumps set the screen alone, as in the game.
    Vanilla,
    /// Screen jumps also set the vertical part, as Lunar Magic 3 reads
    /// them.
    Tall,
}

/// An object, placed at absolute tile coordinates where it has a place.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Object {
    /// A standard object (`01`-`21`, `2E`-`3F`) and its settings byte.
    Standard {
        number: u8,
        x: u16,
        y: u16,
        settings: u8,
    },
    /// An extended object (`04` and up; `02` and `03` are Lunar Magic's).
    Extended { number: u8, x: u16, y: u16 },
    /// A screen exit: on screen `screen`, to level or secondary exit
    /// `destination`, with flags `0000wush` (see [`ScreenExit`]).
    ScreenExit(ScreenExit),
    /// One of Lunar Magic's placed objects (`22`, `23`, `27`, `29`,
    /// `2D`), with the bytes after its second as they are stored.
    Lunar {
        number: u8,
        x: u16,
        y: u16,
        data: Vec<u8>,
    },
    /// An object with no place, stored as it is without its new-screen
    /// bit: Lunar Magic's settings objects (`24`-`26`, `28`) and its
    /// long screen exit (extended `02`).
    Unplaced(Vec<u8>),
}

/// A screen exit, extended object `00`: `000ppppp 0000wush 00000000
/// dddddddd`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenExit {
    /// The screen it applies to, 0 to 31.
    pub screen: u8,
    /// `w` (midway, or water in Lunar Magic's format), `u` (Lunar
    /// Magic's format), `s` (secondary exit), `h` (bit 8 of the
    /// destination, in Lunar Magic's format).
    pub flags: u8,
    /// The destination's low byte.
    pub destination: u8,
}

/// What a standard object's settings byte holds, from the game's handlers
/// (`CODE_0DA40F`'s table): a height and a width nibble, each one less than
/// the tiles it gives, a type in place of one of them, or a length in the
/// whole byte. The tileset-specific objects (`2E`-`3F`) differ by tileset
/// and are kept as they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Settings {
    HeightWidth,
    HeightType,
    TypeWidth,
    /// A height, and a low nibble the handler does not read.
    Height,
    /// A width, and a high nibble the handler does not read.
    Width,
    /// A length in the whole byte.
    Length,
    Raw,
}

impl Settings {
    pub fn of(number: u8) -> Self {
        match number {
            0x01..=0x0E | 0x14 | 0x16 | 0x18..=0x1B | 0x1D => Self::HeightWidth,
            0x0F | 0x12 | 0x13 | 0x15 | 0x1E => Self::HeightType,
            0x10 | 0x17 => Self::TypeWidth,
            0x11 | 0x1F => Self::Height,
            0x1C | 0x20 => Self::Width,
            0x21 => Self::Length,
            _ => Self::Raw,
        }
    }
}

impl ScreenExit {
    /// Lunar Magic's format (`u`), where `h` is bit 8 of the destination.
    pub const LUNAR_MAGIC: u8 = 0x04;
    pub const HIGH: u8 = 0x01;

    /// The exit in Lunar Magic's format, which says the destination's bit
    /// 8 itself: in the game's, it is the current level's. Lunar Magic
    /// rewrites a level's exits so when it saves the level.
    pub fn in_lunar_magic_format(self, level: u16) -> Self {
        if self.flags & Self::LUNAR_MAGIC != 0 {
            return self;
        }
        let high = if level & 0x100 != 0 { Self::HIGH } else { 0 };
        Self {
            flags: (self.flags & !Self::HIGH) | Self::LUNAR_MAGIC | high,
            ..self
        }
    }
}

/// Decoded object data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ObjectData {
    /// The five bytes before the objects.
    pub header: [u8; 5],
    pub objects: Vec<Object>,
    /// Bytes read, including the terminator.
    pub len: usize,
}

/// The length of a Lunar Magic placed object, from its first bytes.
fn lunar_len(number: u8, bytes: &[u8]) -> Option<usize> {
    match number {
        0x22 | 0x23 => Some(4),
        0x2D => Some(5),
        0x27 | 0x29 => {
            let kind = *bytes.get(3)? >> 6;
            Some(match kind {
                0 | 1 => 5,
                2 => 6,
                _ if bytes[2] & 0x80 != 0 => 8,
                _ => 7,
            })
        }
        _ => None,
    }
}

fn is_lunar_placed(number: u8) -> bool {
    matches!(number, 0x22 | 0x23 | 0x27 | 0x29 | 0x2D)
}

fn is_lunar_unplaced(number: u8) -> bool {
    matches!(number, 0x24..=0x26 | 0x28)
}

/// Decodes object data: the five-byte header, then objects up to the
/// terminator. Trailing bytes are ignored.
pub fn decode(data: &[u8], layout: Layout, jumps: Jumps) -> Result<ObjectData, ObjectError> {
    let get = |i: usize| data.get(i).copied().ok_or(ObjectError::Truncated(i));
    let header: [u8; 5] = data
        .get(..5)
        .ok_or(ObjectError::Truncated(data.len()))?
        .try_into()
        .expect("five bytes");
    let mut objects = Vec::new();
    let mut i = 5;
    // The current screen, and the vertical part a tall jump sets.
    let (mut screen, mut high) = (0u16, 0u16);
    loop {
        let a = get(i)?;
        if a == END {
            return Ok(ObjectData {
                header,
                objects,
                len: i + 1,
            });
        }
        let b = get(i + 1)?;
        let third = get(i + 2)?;
        if a & NEW_SCREEN != 0 {
            screen += 1;
        }
        let number = ((a & 0x60) >> 1) | (b >> 4);
        let (lo, hi) = (a & 0x1F, b & 0x0F);
        let (x, y) = match layout {
            Layout::Horizontal => (screen * 16 + hi as u16, high * JUMP_ROWS + lo as u16),
            Layout::Vertical => (lo as u16, screen * 16 + hi as u16),
        };
        let unplaced = |len: usize| -> Result<Object, ObjectError> {
            get(i + len - 1)?;
            let mut bytes = data[i..i + len].to_vec();
            bytes[0] &= !NEW_SCREEN;
            Ok(Object::Unplaced(bytes))
        };
        let (object, len) = if number == 0 {
            match third {
                EXT_SCREEN_EXIT => {
                    let exit = ScreenExit {
                        screen: lo,
                        flags: hi,
                        destination: get(i + 3)?,
                    };
                    (Some(Object::ScreenExit(exit)), 4)
                }
                EXT_SCREEN_JUMP => {
                    screen = lo as u16;
                    if jumps == Jumps::Tall && layout == Layout::Horizontal {
                        high = hi as u16;
                    }
                    (None, 3)
                }
                EXT_TALL_SCREEN_JUMP => {
                    screen = hi as u16;
                    high = lo as u16;
                    (None, 3)
                }
                EXT_LONG_EXIT => (Some(unplaced(5)?), 5),
                number => (Some(Object::Extended { number, x, y }), 3),
            }
        } else if is_lunar_placed(number) {
            let len = lunar_len(number, &data[i..data.len().min(i + 4)])
                .ok_or(ObjectError::Truncated(data.len()))?;
            get(i + len - 1)?;
            let data = data[i + 2..i + len].to_vec();
            (Some(Object::Lunar { number, x, y, data }), len)
        } else if is_lunar_unplaced(number) {
            (Some(unplaced(3)?), 3)
        } else {
            let settings = third;
            (
                Some(Object::Standard {
                    number,
                    x,
                    y,
                    settings,
                }),
                3,
            )
        };
        objects.extend(object);
        i += len;
    }
}

/// Encodes object data. Each placed object is put on its screen with the
/// new-screen bit when that screen follows the current one, and with a
/// screen jump before it otherwise, or when the bit would make its first
/// byte `$FF`, the terminator.
pub fn encode(
    header: [u8; 5],
    objects: &[Object],
    layout: Layout,
    jumps: Jumps,
) -> Result<Vec<u8>, ObjectError> {
    let mut out = header.to_vec();
    let (mut screen, mut high) = (0u16, 0u16);
    for (index, object) in objects.iter().enumerate() {
        let bad_number = |kind| ObjectError::BadNumber {
            index,
            number: object_number(object),
            kind,
        };
        let (number, x, y, rest): (u8, u16, u16, &[u8]) = match object {
            Object::Standard {
                number,
                x,
                y,
                settings,
            } => {
                let valid = matches!(number, 0x01..=0x3F)
                    && !is_lunar_placed(*number)
                    && !is_lunar_unplaced(*number);
                if !valid {
                    return Err(bad_number("standard object"));
                }
                (*number, *x, *y, std::slice::from_ref(settings))
            }
            Object::Extended { number, x, y } => {
                if *number <= EXT_TALL_SCREEN_JUMP {
                    return Err(bad_number("placed extended object"));
                }
                (0, *x, *y, std::slice::from_ref(number))
            }
            Object::Lunar { number, x, y, data } => {
                if !is_lunar_placed(*number) {
                    return Err(bad_number("Lunar Magic placed object"));
                }
                let mut head = vec![0, 0];
                head.extend_from_slice(data);
                if lunar_len(*number, &head) != Some(data.len() + 2) {
                    return Err(ObjectError::BadLength {
                        index,
                        len: data.len() + 2,
                        kind: "Lunar Magic placed object",
                        expected: "the length its bytes declare",
                    });
                }
                (*number, *x, *y, &data[..])
            }
            Object::ScreenExit(exit) => {
                if exit.screen > 0x1F || exit.flags > 0x0F {
                    return Err(bad_number("screen exit"));
                }
                out.extend([exit.screen, exit.flags, EXT_SCREEN_EXIT, exit.destination]);
                continue;
            }
            Object::Unplaced(bytes) => {
                let expected = match bytes.as_slice() {
                    [a, b, EXT_LONG_EXIT, ..] if a & 0x60 == 0 && b >> 4 == 0 => 5,
                    [a, b, ..] if is_lunar_unplaced(((a & 0x60) >> 1) | (b >> 4)) => 3,
                    _ => return Err(bad_number("unplaced object")),
                };
                if bytes.len() != expected || bytes[0] & NEW_SCREEN != 0 {
                    return Err(ObjectError::BadLength {
                        index,
                        len: bytes.len(),
                        kind: "unplaced object",
                        expected: "3 bytes, or 5 for a long screen exit, with no new-screen bit",
                    });
                }
                out.extend_from_slice(bytes);
                continue;
            }
        };
        let unplaceable = ObjectError::Unplaceable {
            index,
            x,
            y,
            layout,
        };
        let (to_screen, to_high, lo, hi) = match layout {
            Layout::Horizontal => (x / 16, y / JUMP_ROWS, y % JUMP_ROWS, x % 16),
            Layout::Vertical if x < 32 => (y / 16, 0, x, y % 16),
            Layout::Vertical => return Err(unplaceable),
        };
        if to_high > 0 && jumps == Jumps::Vanilla {
            return Err(unplaceable);
        }
        let a = ((number & 0x30) << 1) | lo as u8;
        let b = ((number & 0x0F) << 4) | hi as u8;
        let next = to_high == high && to_screen == screen + 1 && a | NEW_SCREEN != END;
        let a = if (to_screen, to_high) == (screen, high) {
            a
        } else if next {
            a | NEW_SCREEN
        } else {
            let jump = if to_screen <= 0x1F && to_high <= 0x0F {
                [to_screen as u8, to_high as u8, EXT_SCREEN_JUMP]
            } else if jumps == Jumps::Tall && to_screen <= 0x0F && to_high <= 0x1F {
                [to_high as u8, to_screen as u8, EXT_TALL_SCREEN_JUMP]
            } else {
                return Err(unplaceable);
            };
            out.extend(jump);
            a
        };
        (screen, high) = (to_screen, to_high);
        out.extend([a, b]);
        out.extend_from_slice(rest);
    }
    out.push(END);
    Ok(out)
}

/// The object number an object is written with, for errors.
fn object_number(object: &Object) -> u8 {
    match object {
        Object::Standard { number, .. } | Object::Lunar { number, .. } => *number,
        Object::Extended { .. } | Object::ScreenExit(_) => 0,
        Object::Unplaced(bytes) => match bytes.as_slice() {
            [a, b, ..] => ((a & 0x60) >> 1) | (b >> 4),
            _ => 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: [u8; 5] = [1, 2, 3, 4, 5];

    fn data(objects: &[u8]) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(objects);
        bytes.push(END);
        bytes
    }

    fn round_trip(objects: &[Object], layout: Layout, jumps: Jumps) -> Vec<u8> {
        let bytes = encode(HEADER, objects, layout, jumps).unwrap();
        let decoded = decode(&bytes, layout, jumps).unwrap();
        assert_eq!(decoded.objects, objects);
        assert_eq!(decoded.len, bytes.len());
        bytes
    }

    #[test]
    fn horizontal_objects_are_placed_on_their_screens() {
        // Coins (05) on screen 0, a new-screen cement block (0D), and a
        // screen jump to 3 before an extended object.
        let bytes = data(&[
            0x0A, 0x53, 0x12, // coins at (3, 10)
            0x98, 0xD2, 0x00, // new screen: cement at (16 + 2, 24)
            0x03, 0x00, 0x01, // jump to screen 3
            0x05, 0x04, 0x2D, // extended object 2D at (48 + 4, 5)
        ]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects,
            [
                Object::Standard {
                    number: 0x05,
                    x: 3,
                    y: 10,
                    settings: 0x12
                },
                Object::Standard {
                    number: 0x0D,
                    x: 18,
                    y: 24,
                    settings: 0
                },
                Object::Extended {
                    number: 0x2D,
                    x: 52,
                    y: 5
                },
            ]
        );
        assert_eq!(decoded.len, bytes.len());
        assert_eq!(
            encode(HEADER, &decoded.objects, Layout::Horizontal, Jumps::Vanilla).unwrap(),
            bytes
        );
    }

    #[test]
    fn vertical_layers_swap_the_place() {
        // Row in the second byte, column and half in the first.
        let bytes = data(&[0x80 | 0x13, 0x57, 0x20]);
        let decoded = decode(&bytes, Layout::Vertical, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects,
            [Object::Standard {
                number: 0x05,
                x: 19,
                y: 16 + 7,
                settings: 0x20
            }]
        );
        let bytes = round_trip(&decoded.objects, Layout::Vertical, Jumps::Vanilla);
        assert_eq!(bytes[5..8], [0x93, 0x57, 0x20]);
        assert!(matches!(
            encode(
                HEADER,
                &[Object::Extended {
                    number: 0x40,
                    x: 32,
                    y: 0
                }],
                Layout::Vertical,
                Jumps::Vanilla
            ),
            Err(ObjectError::Unplaceable { .. })
        ));
    }

    #[test]
    fn screen_exits_keep_their_own_screen() {
        let bytes = data(&[0x80 | 0x04, 0x03, 0x00, 0x21, 0x05, 0x14, 0x40]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects[0],
            Object::ScreenExit(ScreenExit {
                screen: 4,
                flags: 3,
                destination: 0x21
            })
        );
        // The exit's new-screen bit still moved the screen on.
        assert_eq!(
            decoded.objects[1],
            Object::Standard {
                number: 0x01,
                x: 20,
                y: 5,
                settings: 0x40
            }
        );
        round_trip(&decoded.objects, Layout::Horizontal, Jumps::Vanilla);
    }

    #[test]
    fn exits_in_lunar_magic_format() {
        let exit = ScreenExit {
            screen: 7,
            flags: 0x02,
            destination: 0xCB,
        };
        assert_eq!(exit.in_lunar_magic_format(0x105).flags, 0x07);
        assert_eq!(exit.in_lunar_magic_format(0x005).flags, 0x06);
        let lunar = ScreenExit {
            flags: 0x04,
            ..exit
        };
        assert_eq!(lunar.in_lunar_magic_format(0x105), lunar);
    }

    #[test]
    fn a_first_byte_of_ff_becomes_a_jump() {
        // Object 3x at row 31 of the next screen would start with $FF.
        let objects = [
            Object::Standard {
                number: 0x01,
                x: 0,
                y: 0,
                settings: 0,
            },
            Object::Standard {
                number: 0x3F,
                x: 16,
                y: 31,
                settings: 0,
            },
        ];
        let bytes = round_trip(&objects, Layout::Horizontal, Jumps::Vanilla);
        assert_eq!(bytes[8..], [0x01, 0x00, 0x01, 0x7F, 0xF0, 0x00, END]);
    }

    #[test]
    fn tall_levels_jump_vertically() {
        let objects = [
            Object::Standard {
                number: 0x01,
                x: 20,
                y: 70,
                settings: 0,
            },
            Object::Standard {
                number: 0x01,
                x: 3,
                y: 32 * 20 + 1,
                settings: 0,
            },
        ];
        let bytes = round_trip(&objects, Layout::Horizontal, Jumps::Tall);
        // Screen 1, part 2; then extended 03 for part 20 of screen 0.
        assert_eq!(bytes[5..8], [0x01, 0x02, 0x01]);
        assert_eq!(bytes[11..14], [20, 0x00, 0x03]);
        assert!(matches!(
            encode(HEADER, &objects, Layout::Horizontal, Jumps::Vanilla),
            Err(ObjectError::Unplaceable { index: 0, .. })
        ));
    }

    #[test]
    fn lunar_magic_objects_have_their_own_lengths() {
        let bytes = data(&[
            0x40 | 0x02,
            0x23,
            0x11,
            0x55, // 22: direct Map16, 4 bytes
            0x40 | 0x03,
            0x74,
            0x00,
            0x01,
            0x23, // 27 single tile, 5 bytes
            0x40,
            0x75,
            0x11,
            0x41,
            0x23, // 27 tiles unstretched, 5 bytes
            0x40,
            0x75,
            0x00,
            0x81,
            0x23,
            0x11, // 27 stretched tiles, 6 bytes
            0x40,
            0x76,
            0x05,
            0xC0,
            0x00,
            0x11,
            0x02, // 27 multi-screen, 7
            0x40,
            0x77,
            0x85,
            0xC0,
            0x00,
            0x11,
            0x02,
            0x81, // 27 conditional, 8
            0x40,
            0xD8,
            0x01,
            0x02,
            0x03, // 2D, 5 bytes
            0x80 | 0x40 | 0x05,
            0x60,
            0x21, // 26, the music bypass: no place
            0x01,
            0x00,
            0x02,
            0x10,
            0x3E, // extended 02, a long exit
        ]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Tall).unwrap();
        let lengths: Vec<_> = decoded
            .objects
            .iter()
            .map(|o| match o {
                Object::Lunar { data, .. } => data.len() + 2,
                Object::Unplaced(bytes) => bytes.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(lengths, [4, 5, 5, 6, 7, 8, 5, 3, 5]);
        assert_eq!(decoded.objects[7], Object::Unplaced(vec![0x45, 0x60, 0x21]));
        round_trip(&decoded.objects, Layout::Horizontal, Jumps::Tall);
    }

    #[test]
    fn malformed_data_is_an_error() {
        assert_eq!(
            decode(&[1, 2, 3], Layout::Horizontal, Jumps::Vanilla),
            Err(ObjectError::Truncated(3))
        );
        let mut bytes = data(&[0x40, 0x77, 0x85, 0xC0]);
        bytes.pop();
        assert!(matches!(
            decode(&bytes, Layout::Horizontal, Jumps::Tall),
            Err(ObjectError::Truncated(_))
        ));
        let bad = |object| encode(HEADER, &[object], Layout::Horizontal, Jumps::Vanilla);
        assert!(matches!(
            bad(Object::Standard {
                number: 0x22,
                x: 0,
                y: 0,
                settings: 0
            }),
            Err(ObjectError::BadNumber { .. })
        ));
        assert!(matches!(
            bad(Object::Extended {
                number: 0x01,
                x: 0,
                y: 0
            }),
            Err(ObjectError::BadNumber { .. })
        ));
        assert!(matches!(
            bad(Object::Lunar {
                number: 0x27,
                x: 0,
                y: 0,
                data: vec![0x00, 0xC0, 0x00]
            }),
            Err(ObjectError::BadLength { .. })
        ));
        assert!(matches!(
            bad(Object::Standard {
                number: 0x01,
                x: 16 * 40,
                y: 0,
                settings: 0
            }),
            Err(ObjectError::Unplaceable { .. })
        ));
    }
}
