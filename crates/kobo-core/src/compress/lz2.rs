//! LC_LZ2, the compression SMW uses for GFX files and other assets.
//!
//! A stream is a sequence of chunks, each starting with a header byte:
//! the top three bits are the command and the low five bits are the
//! length minus one. A command of 7 marks a long header: the command moves
//! to bits 2 to 4 and the length becomes ten bits spanning the low two bits
//! and the following byte. A header byte of `$FF` ends the stream.
//!
//! Commands:
//! 0 direct copy, 1 byte fill, 2 word fill, 3 incrementing fill, 4 copy
//! from an earlier point in the output (big-endian 16-bit offset).

use thiserror::Error;

/// Output offsets are 16-bit, so no stream can address more than this.
pub const MAX_OUTPUT: usize = 0x1_0000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Lz2Error {
    #[error("compressed data ends at offset {0} without a terminator")]
    Truncated(usize),
    #[error("unknown command {cmd} at offset {offset}")]
    BadCommand { cmd: u8, offset: usize },
    #[error(
        "back-reference to output offset {src} at input offset {offset} points past the {len} bytes produced so far"
    )]
    BadBackRef {
        src: usize,
        len: usize,
        offset: usize,
    },
    #[error("decompressed output exceeds {MAX_OUTPUT} bytes")]
    TooLarge,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decompressed {
    pub data: Vec<u8>,
    /// Number of input bytes consumed, including the terminator.
    pub consumed: usize,
}

/// Decompresses one LC_LZ2 stream from the start of `input`. Trailing
/// bytes after the terminator are ignored.
pub fn decompress(input: &[u8]) -> Result<Decompressed, Lz2Error> {
    let mut out = Vec::new();
    let mut pos = 0;
    let byte = |at: usize| input.get(at).copied().ok_or(Lz2Error::Truncated(at));
    loop {
        let start = pos;
        let header = byte(pos)?;
        pos += 1;
        if header == 0xFF {
            return Ok(Decompressed {
                data: out,
                consumed: pos,
            });
        }
        let (cmd, len) = if header >> 5 == 7 {
            let low = byte(pos)?;
            pos += 1;
            let len = (((header & 0x03) as usize) << 8) | low as usize;
            ((header >> 2) & 0x07, len + 1)
        } else {
            (header >> 5, (header & 0x1F) as usize + 1)
        };
        if out.len() + len > MAX_OUTPUT {
            return Err(Lz2Error::TooLarge);
        }
        match cmd {
            0 => {
                let src = input
                    .get(pos..pos + len)
                    .ok_or(Lz2Error::Truncated(input.len()))?;
                out.extend_from_slice(src);
                pos += len;
            }
            1 => {
                let b = byte(pos)?;
                pos += 1;
                out.extend(std::iter::repeat_n(b, len));
            }
            2 => {
                let pair = [byte(pos)?, byte(pos + 1)?];
                pos += 2;
                out.extend((0..len).map(|i| pair[i & 1]));
            }
            3 => {
                let b = byte(pos)?;
                pos += 1;
                out.extend((0..len).map(|i| b.wrapping_add(i as u8)));
            }
            4 => {
                let src = ((byte(pos)? as usize) << 8) | byte(pos + 1)? as usize;
                pos += 2;
                if src >= out.len() {
                    return Err(Lz2Error::BadBackRef {
                        src,
                        len: out.len(),
                        offset: start,
                    });
                }
                // Overlapping copies are allowed and repeat the pattern.
                for i in 0..len {
                    let b = out[src + i];
                    out.push(b);
                }
            }
            _ => return Err(Lz2Error::BadCommand { cmd, offset: start }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &[u8]) -> Vec<u8> {
        let d = decompress(input).unwrap();
        assert_eq!(d.consumed, input.len());
        d.data
    }

    #[test]
    fn empty_stream() {
        assert_eq!(ok(&[0xFF]), b"");
    }

    #[test]
    fn direct_copy() {
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0xFF]), b"abc");
    }

    #[test]
    fn byte_fill() {
        assert_eq!(ok(&[0x20 | 0x03, b'A', 0xFF]), b"AAAA");
    }

    #[test]
    fn word_fill() {
        assert_eq!(ok(&[0x40 | 0x04, b'A', b'B', 0xFF]), b"ABABA");
    }

    #[test]
    fn incrementing_fill() {
        assert_eq!(ok(&[0x60 | 0x03, 0x10, 0xFF]), [0x10, 0x11, 0x12, 0x13]);
        // Wraps around at 256.
        assert_eq!(ok(&[0x60 | 0x02, 0xFE, 0xFF]), [0xFE, 0xFF, 0x00]);
    }

    #[test]
    fn back_reference() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x01, 0x00, 0x00, 0xFF]),
            b"abcab"
        );
    }

    #[test]
    fn overlapping_back_reference_repeats() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x04, 0x00, 0x01, 0xFF]),
            b"abcbcbcb"
        );
    }

    #[test]
    fn long_direct_copy() {
        let payload: Vec<u8> = (0..=255).chain(0..1).collect();
        let mut input = vec![0xE0 | 0x01, 0x00]; // long header, command 0
        input.extend(&payload);
        input.push(0xFF);
        assert_eq!(ok(&input), payload);
    }

    #[test]
    fn long_byte_fill() {
        // Long header for command 1 with length 0x3FF + 1 = 1024.
        assert_eq!(
            ok(&[0xE0 | (1 << 2) | 0x03, 0xFF, 0x7E, 0xFF]),
            vec![0x7E; 1024]
        );
    }

    #[test]
    fn consumed_ignores_trailing_bytes() {
        let d = decompress(&[0x00, b'x', 0xFF, 0xAA, 0xBB]).unwrap();
        assert_eq!(d.data, b"x");
        assert_eq!(d.consumed, 3);
    }

    #[test]
    fn errors() {
        assert_eq!(decompress(&[]), Err(Lz2Error::Truncated(0)));
        assert_eq!(decompress(&[0x02, b'a']), Err(Lz2Error::Truncated(2)));
        assert_eq!(decompress(&[0x00, b'a']), Err(Lz2Error::Truncated(2)));
        assert_eq!(
            decompress(&[0xA0, 0x00, 0xFF]),
            Err(Lz2Error::BadCommand { cmd: 5, offset: 0 })
        );
        assert_eq!(
            decompress(&[0xE0 | (7 << 2), 0x00, 0xFF]),
            Err(Lz2Error::BadCommand { cmd: 7, offset: 0 })
        );
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00, 0x05, 0xFF]),
            Err(Lz2Error::BadBackRef {
                src: 5,
                len: 1,
                offset: 2
            })
        );
    }

    #[test]
    fn output_limit() {
        // 65 fills of 1024 bytes exceed 64 KiB.
        let mut input = Vec::new();
        for _ in 0..65 {
            input.extend([0xE0 | (1 << 2) | 0x03, 0xFF, 0x00]);
        }
        input.push(0xFF);
        assert_eq!(decompress(&input), Err(Lz2Error::TooLarge));
    }
}
