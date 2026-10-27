//! Scaleform GFx movies (`SwfMovie.RawData`, T-021): SWF container with GFx extension tags.
//! Checked on every movie of the installation: plan/03-evidence.md E-032.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

pub struct Movie {
    /// `GFX` (uncompressed) or `CFX` (zlib after the 8-byte header), as stored.
    pub signature: [u8; 3],
    pub version: u8,
    /// The whole uncompressed file, header included, so ranges index it like an uncompressed movie.
    pub data: Vec<u8>,
    pub frame_rate: u16,
    pub frame_count: u16,
    /// Top-level tags, in file order.
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub code: u16,
    pub data: Range<usize>,
}

impl Movie {
    pub fn read(raw: &[u8]) -> Result<Self> {
        let header = raw.get(..8).ok_or_else(|| bad("movie shorter than its header"))?;
        let signature: [u8; 3] = header[..3].try_into().unwrap();
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut data = header.to_vec();
        match &signature {
            b"GFX" | b"FWS" => data.extend_from_slice(&raw[8..]),
            b"CFX" | b"CWS" => data.extend(miniz_oxide::inflate::decompress_to_vec_zlib(&raw[8..])
                .map_err(|e| bad(format!("zlib: {e:?}")))?),
            _ => return Err(bad("not a SWF/GFx movie")),
        }
        if data.len() != length { return Err(bad(format!("movie is {} bytes, header says {length}", data.len()))); }
        // RECT: 5-bit field width, then four signed fields, padded to a byte.
        let bits = data.get(8).ok_or_else(|| bad("missing frame rect"))? >> 3;
        let pos = 8 + (5 + 4 * bits as usize).div_ceil(8);
        let word = |i: usize| data.get(i..i + 2).map(|w| u16::from_le_bytes(w.try_into().unwrap())).ok_or_else(|| bad("truncated header"));
        let (frame_rate, frame_count) = (word(pos)?, word(pos + 2)?);
        let tags = tags(&data, pos + 4..data.len())?;
        Ok(Self { signature, version: header[3], data, frame_rate, frame_count, tags })
    }
}

/// Tags of a tag list ending with End (code 0) exactly at `range.end`; End itself is not returned.
pub fn tags(data: &[u8], range: Range<usize>) -> Result<Vec<Tag>> {
    let mut out = Vec::new();
    let mut pos = range.start;
    loop {
        let header = data.get(pos..pos + 2).filter(|_| pos + 2 <= range.end).ok_or_else(|| bad("tag list without End"))?;
        let header = u16::from_le_bytes(header.try_into().unwrap());
        let (code, mut len) = (header >> 6, (header & 0x3f) as usize);
        pos += 2;
        if len == 0x3f {
            len = data.get(pos..pos + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()) as usize).ok_or_else(|| bad("truncated long tag"))?;
            pos += 4;
        }
        let end = pos.checked_add(len).filter(|&e| e <= range.end).ok_or_else(|| bad(format!("tag {code} past its container")))?;
        if code == 0 {
            if end != range.end { return Err(bad(format!("{} bytes after End", range.end - end))); }
            return Ok(out);
        }
        out.push(Tag { code, data: pos..end });
        pos = end;
    }
}

/// Opcode and payload range.
pub type Action = (u8, Range<usize>);

/// AS2 action records up to ActionEnd: (opcode, payload). Function bodies are inline, so a linear walk sees them.
/// `range` may extend past ActionEnd (clip/button action lists); the returned end is the byte after ActionEnd.
pub fn actions(data: &[u8], range: Range<usize>) -> Result<(Vec<Action>, usize)> {
    let mut out = Vec::new();
    let mut pos = range.start;
    while pos < range.end {
        let code = data[pos];
        pos += 1;
        if code == 0 { return Ok((out, pos)); }
        let mut payload = pos..pos;
        if code >= 0x80 {
            let len = data.get(pos..pos + 2).filter(|_| pos + 2 <= range.end).map(|w| u16::from_le_bytes(w.try_into().unwrap()) as usize)
                .ok_or_else(|| bad("truncated action length"))?;
            payload = pos + 2..pos + 2 + len;
            if payload.end > range.end { return Err(bad(format!("action {code:#x} past its container"))); }
            pos = payload.end;
        }
        out.push((code, payload));
    }
    Err(bad("action list without ActionEnd"))
}

/// MSB-first bit reader for SWF records (RECT, MATRIX, CXFORM).
struct Bits<'a> { data: &'a [u8], bit: usize }

impl Bits<'_> {
    fn read(&mut self, n: usize) -> Result<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.data.get(self.bit / 8).ok_or_else(|| bad("bit field past tag"))?;
            v = v << 1 | (byte >> (7 - self.bit % 8) & 1) as u32;
            self.bit += 1;
        }
        Ok(v)
    }
    fn end(&self) -> usize { self.bit.div_ceil(8) }
}

/// Clip action lists of a PlaceObject2 (code 26) or PlaceObject3 (code 70) tag: one action range per event record.
pub fn clip_actions(data: &[u8], tag: &Tag) -> Result<Vec<Range<usize>>> {
    let d = &data[tag.data.clone()];
    let flags = *d.first().ok_or_else(|| bad("empty PlaceObject"))?;
    if flags & 0x80 == 0 { return Ok(Vec::new()); }
    let flags2 = if tag.code == 70 { *d.get(1).ok_or_else(|| bad("short PlaceObject3"))? } else { 0 };
    let mut pos = if tag.code == 70 { 2 } else { 1 } + 2; // flags, depth
    let string = |pos: &mut usize| -> Result<()> {
        let n = d.get(*pos..).and_then(|s| s.iter().position(|&b| b == 0)).ok_or_else(|| bad("unterminated string"))?;
        *pos += n + 1;
        Ok(())
    };
    if tag.code == 70 && (flags2 & 0x08 != 0 || (flags2 & 0x10 != 0 && flags & 0x02 != 0)) { string(&mut pos)?; } // class name
    if flags & 0x02 != 0 { pos += 2; } // character id
    if flags & 0x04 != 0 { // MATRIX
        let mut b = Bits { data: d.get(pos..).ok_or_else(|| bad("short tag"))?, bit: 0 };
        for _ in 0..2 { if b.read(1)? == 1 { let n = b.read(5)? as usize; b.read(2 * n)?; } } // scale, rotate
        let n = b.read(5)? as usize;
        b.read(2 * n)?;
        pos += b.end();
    }
    if flags & 0x08 != 0 { // CXFORMWITHALPHA
        let mut b = Bits { data: d.get(pos..).ok_or_else(|| bad("short tag"))?, bit: 0 };
        let (add, mult, n) = (b.read(1)?, b.read(1)?, b.read(4)? as usize);
        b.read(4 * n * (add + mult) as usize)?;
        pos += b.end();
    }
    if flags & 0x10 != 0 { pos += 2; } // ratio
    if flags & 0x20 != 0 { string(&mut pos)?; } // name
    if flags & 0x40 != 0 { pos += 2; } // clip depth
    if tag.code == 70 {
        if flags2 & 0x01 != 0 { return Err(bad("PlaceObject3 filter lists are not supported")); }
        if flags2 & 0x02 != 0 { pos += 1; } // blend mode
        if flags2 & 0x04 != 0 { pos += 1; } // cache as bitmap
        if flags2 & 0x20 != 0 { pos += 1; } // visible (SWF 11)
        if flags2 & 0x40 != 0 { pos += 4; } // opaque background RGBA (SWF 11)
    }
    // Clip actions (SWF 6+): reserved u16, all event flags u32, records until zero event flags.
    let base = tag.data.start;
    pos += 6;
    let mut out = Vec::new();
    loop {
        let word = |at: usize| d.get(at..at + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).ok_or_else(|| bad("truncated clip action"));
        let events = word(pos)?;
        pos += 4;
        if events == 0 {
            if pos != d.len() { return Err(bad("bytes after clip actions")); }
            return Ok(out);
        }
        let size = word(pos)? as usize;
        pos += 4;
        let end = pos.checked_add(size).filter(|&e| e <= d.len()).ok_or_else(|| bad("clip action past tag"))?;
        if events & 0x0002_0000 != 0 { pos += 1; } // KeyPress key code
        out.push(base + pos..base + end);
        pos = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(signature: &[u8; 3], body: &[u8]) -> Vec<u8> {
        let mut m = signature.to_vec();
        m.push(8);
        m.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
        m.extend_from_slice(body);
        m
    }

    #[test]
    fn tags_and_actions() {
        // RECT with 0-bit fields (1 byte), frame rate, 1 frame, DoAction [Stop, Play-with-payload?], End
        let body = [0x00, 0x00, 0x18, 0x01, 0x00, (12 << 6 | 5) as u8, 0x03, 0x07, 0x96, 0x00, 0x00, 0x00, 0x00, 0x00];
        let m = Movie::read(&movie(b"GFX", &body)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((m.frame_rate, m.frame_count, m.tags.len(), m.tags[0].code), (0x1800, 1, 1, 12));
        let (acts, end) = actions(&m.data, m.tags[0].data.clone()).unwrap();
        assert_eq!(acts.iter().map(|a| a.0).collect::<Vec<_>>(), [0x07, 0x96]);
        assert_eq!(end, m.tags[0].data.end);
        assert!(tags(&[0x3f, 0x03, 9, 0, 0, 0], 0..6).is_err()); // long tag past container
        assert!(actions(&[0x96, 5, 0], 0..3).is_err());
        let compressed = {
            let mut c = movie(b"CFX", &[]);
            let inner = [0u8, 0, 0x18, 1, 0, 0, 0];
            c[4..8].copy_from_slice(&((8 + inner.len()) as u32).to_le_bytes());
            c.extend(miniz_oxide::deflate::compress_to_vec_zlib(&inner, 6));
            c
        };
        // PlaceObject2: has clip actions + name "a", depth 1; one EnterFrame record with [Stop, End]
        let place = [0xa0u8, 1, 0, b'a', 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 0x07, 0, 0, 0, 0, 0];
        let mut data = vec![0u8; 3];
        data.extend_from_slice(&place);
        let ranges = clip_actions(&data, &Tag { code: 26, data: 3..data.len() }).unwrap();
        assert_eq!(ranges, vec![22..24]);
        assert_eq!(actions(&data, ranges[0].clone()).unwrap().1, 24);
        assert!(clip_actions(&data[..data.len() - 1], &Tag { code: 26, data: 3..data.len() - 1 }).is_err());
        assert_eq!(Movie::read(&compressed).unwrap_or_else(|e| panic!("{e}")).frame_count, 1);
    }
}
