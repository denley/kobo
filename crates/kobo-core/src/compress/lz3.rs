//! LC_LZ3, which Lunar Magic can store a hack's GFX and ExGFX files in
//! instead of LC_LZ2 (it replaces the game's decompression routine to
//! match). It trades the incrementing fill for a zero fill and adds two
//! more ways to copy from the output.
//!
//! Commands (see [`super`] for the chunk headers):
//! 0 direct copy, 1 byte fill, 2 word fill, 3 zero fill (no operand),
//! 4 copy from an earlier point in the output, 5 the same with the bits
//! of each byte reversed, 6 the same reading backwards from that point.
//!
//! The copies name their source in one of two ways. With bit 7 of the
//! first byte clear it is a big-endian 15-bit offset into the output;
//! with it set the byte's low seven bits count back from the last byte
//! written (`$80` is that byte itself).

use super::{Decompressed, LzError, Reader};

/// Decompresses one LC_LZ3 stream from the start of `input`. Trailing
/// bytes after the terminator are ignored.
pub fn decompress(input: &[u8]) -> Result<Decompressed, LzError> {
    super::decompress(input, |cmd, len, start, reader, out| {
        match cmd {
            3 => out.extend(std::iter::repeat_n(0, len)),
            4..=6 => {
                let src = source(reader, out.len(), start)?;
                // A backwards copy must not run off the start; forward
                // ones may overlap what they write and repeat the pattern.
                if cmd == 6 && len > src + 1 {
                    return Err(LzError::BadBackRef {
                        src,
                        len: out.len(),
                        offset: start,
                    });
                }
                for i in 0..len {
                    let b = match cmd {
                        4 => out[src + i],
                        5 => out[src + i].reverse_bits(),
                        _ => out[src - i],
                    };
                    out.push(b);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    })
}

/// Reads a copy command's source and resolves it to an output offset.
fn source(reader: &mut Reader, produced: usize, start: usize) -> Result<usize, LzError> {
    let first = reader.byte()? as usize;
    let bad = |src| LzError::BadBackRef {
        src,
        len: produced,
        offset: start,
    };
    if first & 0x80 != 0 {
        let back = (first & 0x7F) + 1;
        produced.checked_sub(back).ok_or(bad(0))
    } else {
        let src = (first << 8) | reader.byte()? as usize;
        (src < produced).then_some(src).ok_or(bad(src))
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
    fn shares_the_first_three_commands_with_lz2() {
        assert_eq!(ok(&[0xFF]), b"");
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0xFF]), b"abc");
        assert_eq!(ok(&[0x20 | 0x03, b'A', 0xFF]), b"AAAA");
        assert_eq!(ok(&[0x40 | 0x04, b'A', b'B', 0xFF]), b"ABABA");
    }

    #[test]
    fn zero_fill_takes_no_operand() {
        assert_eq!(ok(&[0x60 | 0x02, 0x00, b'x', 0xFF]), [0, 0, 0, b'x']);
        // Long header: command 3, length 0x100 + 1.
        assert_eq!(ok(&[0xE0 | (3 << 2) | 0x01, 0x00, 0xFF]), vec![0; 257]);
    }

    #[test]
    fn copies_from_an_absolute_offset() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x01, 0x00, 0x01, 0xFF]),
            b"abcbc"
        );
        // Overlapping what it writes repeats the pattern.
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x04, 0x00, 0x01, 0xFF]),
            b"abcbcbcb"
        );
    }

    #[test]
    fn copies_from_an_offset_back_from_the_end() {
        // `$80` is the last byte written, `$81` the one before it.
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0x80, 0x80, 0xFF]), b"abcc");
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x03, 0x81, 0xFF]),
            b"abcbcbc"
        );
    }

    #[test]
    fn copies_with_bits_reversed() {
        assert_eq!(
            ok(&[0x01, 0x01, 0xF0, 0xA0 | 0x01, 0x00, 0x00, 0xFF]),
            [0x01, 0xF0, 0x80, 0x0F]
        );
    }

    #[test]
    fn copies_backwards() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0xC0 | 0x02, 0x80, 0xFF]),
            b"abccba"
        );
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0xC0 | 0x01, 0x00, 0x01, 0xFF]),
            b"abcba"
        );
    }

    #[test]
    fn errors() {
        assert_eq!(decompress(&[]), Err(LzError::Truncated(0)));
        assert_eq!(decompress(&[0x80, 0x00]), Err(LzError::Truncated(2)));
        assert_eq!(
            decompress(&[0xE0 | (7 << 2), 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 7, offset: 0 })
        );
        // An offset at or past the end, further back than the start, and
        // a backwards copy longer than what lies before its source.
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00, 0x01, 0xFF]),
            Err(LzError::BadBackRef {
                src: 1,
                len: 1,
                offset: 2
            })
        );
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x81, 0xFF]),
            Err(LzError::BadBackRef {
                src: 0,
                len: 1,
                offset: 2
            })
        );
        assert_eq!(
            decompress(&[0x01, b'a', b'b', 0xC0 | 0x02, 0x80, 0xFF]),
            Err(LzError::BadBackRef {
                src: 1,
                len: 2,
                offset: 3
            })
        );
    }
}
