//! LZO1X decompression (packages and saves use it: plan/03-evidence.md E-040, E-047).
//! Logic follows lzokay; research reference: tools/research/lzo1x.mjs.

use std::io::{Error, ErrorKind, Result};

fn bad(msg: &str) -> Error {
    Error::new(ErrorKind::InvalidData, format!("lzo1x: {msg}"))
}

/// Decompresses one LZO1X stream into exactly `out_len` bytes; returns them and the input bytes consumed.
pub fn decompress(src: &[u8], out_len: usize) -> Result<(Vec<u8>, usize)> {
    let mut dst = Vec::with_capacity(out_len);
    let mut ip = 0usize;
    let byte = |ip: &mut usize| -> Result<usize> {
        let b = *src.get(*ip).ok_or_else(|| bad("input truncated"))?;
        *ip += 1;
        Ok(b as usize)
    };
    let literals = |dst: &mut Vec<u8>, ip: &mut usize, n: usize| -> Result<()> {
        let run = src.get(*ip..*ip + n).ok_or_else(|| bad("literal run past input"))?;
        if dst.len() + n > out_len {
            return Err(bad("output overrun"));
        }
        dst.extend_from_slice(run);
        *ip += n;
        Ok(())
    };
    // Zero bytes extend a length by 255 each; the following byte is added by the caller.
    let zeros = |ip: &mut usize| -> Result<usize> {
        let mut extra = 0;
        while byte(ip)? == 0 {
            extra += 255;
        }
        *ip -= 1;
        Ok(extra)
    };

    let mut state = 0usize;
    let first = *src.first().ok_or_else(|| bad("empty input"))? as usize;
    if first >= 22 {
        ip = 1;
        literals(&mut dst, &mut ip, first - 17)?;
        state = 4;
    } else if first >= 18 {
        ip = 1;
        state = first - 17;
        literals(&mut dst, &mut ip, state)?;
    }
    loop {
        let inst = byte(&mut ip)?;
        let (len, dist, next_state);
        if inst & 0xc0 != 0 {
            dist = (byte(&mut ip)? << 3) + ((inst >> 2) & 7) + 1;
            len = (inst >> 5) + 1;
            next_state = inst & 3;
        } else if inst & 0x20 != 0 {
            let mut l = (inst & 0x1f) + 2;
            if l == 2 {
                l += zeros(&mut ip)? + 31 + byte(&mut ip)?;
            }
            let ds = byte(&mut ip)? | (byte(&mut ip)? << 8);
            len = l;
            dist = (ds >> 2) + 1;
            next_state = ds & 3;
        } else if inst & 0x10 != 0 {
            let mut l = (inst & 7) + 2;
            if l == 2 {
                l += zeros(&mut ip)? + 7 + byte(&mut ip)?;
            }
            let ds = byte(&mut ip)? | (byte(&mut ip)? << 8);
            let d = ((inst & 8) << 11) + (ds >> 2);
            if d == 0 {
                break; // end-of-stream marker
            }
            len = l;
            dist = d + 16384;
            next_state = ds & 3;
        } else if state == 0 {
            let mut l = inst + 3;
            if l == 3 {
                l += zeros(&mut ip)? + 15 + byte(&mut ip)?;
            }
            literals(&mut dst, &mut ip, l)?;
            state = 4;
            continue;
        } else if state != 4 {
            dist = (inst >> 2) + (byte(&mut ip)? << 2) + 1;
            len = 2;
            next_state = inst & 3;
        } else {
            dist = (inst >> 2) + (byte(&mut ip)? << 2) + 2049;
            len = 3;
            next_state = inst & 3;
        }
        if dist > dst.len() || dst.len() + len > out_len {
            return Err(bad("back-reference out of range"));
        }
        let from = dst.len() - dist;
        for i in 0..len {
            dst.push(dst[from + i]); // overlapping copy is intended
        }
        state = next_state;
        literals(&mut dst, &mut ip, next_state)?;
    }
    if dst.len() != out_len {
        return Err(bad(&format!("produced {} bytes, expected {out_len}", dst.len())));
    }
    Ok((dst, ip))
}

#[cfg(test)]
mod tests {
    use super::decompress;

    #[test]
    fn literals_then_overlapping_match_then_end() {
        // 3 literals "abc", M2 match len 6 dist 3, end marker.
        let src = [20, b'a', b'b', b'c', 0xa8, 0x00, 0x11, 0x00, 0x00];
        let (out, used) = decompress(&src, 9).unwrap();
        assert_eq!(out, b"abcabcabc");
        assert_eq!(used, src.len());
    }

    #[test]
    fn rejects_wrong_size_and_truncation() {
        let src = [20, b'a', b'b', b'c', 0xa8, 0x00, 0x11, 0x00, 0x00];
        assert!(decompress(&src, 8).is_err());
        assert!(decompress(&src[..5], 9).is_err());
    }
}
