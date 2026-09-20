//! LC_LZ2, the compression SMW uses for GFX files and other assets.
//!
//! Commands (see [`super`] for the chunk headers):
//! 0 direct copy, 1 byte fill, 2 word fill, 3 incrementing fill, 4 copy
//! from an earlier point in the output (big-endian 16-bit offset).

use super::{Decompressed, LzError};

/// Decompresses one LC_LZ2 stream from the start of `input`. Trailing
/// bytes after the terminator are ignored.
pub fn decompress(input: &[u8]) -> Result<Decompressed, LzError> {
    super::decompress(input, |cmd, len, start, reader, out| {
        match cmd {
            3 => {
                let b = reader.byte()?;
                out.extend((0..len).map(|i| b.wrapping_add(i as u8)));
            }
            4 => {
                let src = ((reader.byte()? as usize) << 8) | reader.byte()? as usize;
                if src >= out.len() {
                    return Err(LzError::BadBackRef {
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
            _ => return Ok(false),
        }
        Ok(true)
    })
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
        assert_eq!(decompress(&[]), Err(LzError::Truncated(0)));
        assert_eq!(decompress(&[0x02, b'a']), Err(LzError::Truncated(2)));
        assert_eq!(decompress(&[0x00, b'a']), Err(LzError::Truncated(2)));
        assert_eq!(
            decompress(&[0xA0, 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 5, offset: 0 })
        );
        assert_eq!(
            decompress(&[0xE0 | (7 << 2), 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 7, offset: 0 })
        );
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00, 0x05, 0xFF]),
            Err(LzError::BadBackRef {
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
        assert_eq!(decompress(&input), Err(LzError::TooLarge));
    }
}
