//! Wwise Vorbis media (RIFF `fmt ` 0xFFFF) of this build (T-020a): header fields and packet layout.
//! Layout checked on the installation: plan/03-evidence.md E-069.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

fn u16_at(d: &[u8], at: usize) -> Result<u16> { Ok(u16::from_le_bytes(d.get(at..at + 2).ok_or_else(|| bad("truncated"))?.try_into().unwrap())) }
fn u32_at(d: &[u8], at: usize) -> Result<u32> { Ok(u32::from_le_bytes(d.get(at..at + 4).ok_or_else(|| bad("truncated"))?.try_into().unwrap())) }

/// Header of one Wwise Vorbis media: `fmt ` of 66 bytes whose last 42 bytes are the `vorb` data.
#[derive(Debug, Clone)]
pub struct WwiseVorbis {
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_second: u32,
    pub sample_count: u32,
    /// `vorb` +0x04: tells whether audio packets carry the Vorbis packet-type and mode bits.
    pub mod_signal: u32,
    /// Absolute range of the `data` chunk payload in the media (may extend past a truncated prefetch copy).
    pub data: Range<usize>,
    /// Offsets from the start of `data`.
    pub setup_packet_offset: u32,
    pub first_audio_packet_offset: u32,
    pub uid: u32,
    pub blocksize_0_pow: u8,
    pub blocksize_1_pow: u8,
    /// Raw 42 `vorb` bytes, for fields not interpreted here.
    pub vorb: Range<usize>,
}

impl WwiseVorbis {
    pub fn read(media: &[u8]) -> Result<Self> {
        if media.get(..4) != Some(b"RIFF") || media.get(8..12) != Some(b"WAVE") { return Err(bad("not RIFF/WAVE")); }
        let (mut fmt, mut data) = (None, None);
        let mut pos = 12;
        while pos + 8 <= media.len() {
            let size = u32_at(media, pos + 4)? as usize;
            let body = pos + 8..pos + 8 + size;
            match &media[pos..pos + 4] {
                b"fmt " => fmt = Some(body.clone()),
                // The data chunk may be cut short in a bank's prefetch copy; keep its declared range.
                b"data" => { data = Some(body.clone()); break; }
                _ => {}
            }
            pos = body.end + (size & 1);
        }
        let fmt = fmt.ok_or_else(|| bad("no fmt chunk"))?;
        let data = data.ok_or_else(|| bad("no data chunk"))?;
        if fmt.len() != 0x42 || u16_at(media, fmt.start)? != 0xffff { return Err(bad(format!("fmt size {} is not Wwise Vorbis 0x42", fmt.len()))); }
        let f = fmt.start;
        let v = f + 0x18;
        Ok(Self {
            channels: u16_at(media, f + 2)?,
            sample_rate: u32_at(media, f + 4)?,
            avg_bytes_per_second: u32_at(media, f + 8)?,
            sample_count: u32_at(media, v)?,
            mod_signal: u32_at(media, v + 4)?,
            data,
            setup_packet_offset: u32_at(media, v + 0x10)?,
            first_audio_packet_offset: u32_at(media, v + 0x14)?,
            uid: u32_at(media, v + 0x24)?,
            blocksize_0_pow: media[v + 0x28],
            blocksize_1_pow: media[v + 0x29],
            vorb: v..v + 0x2a,
        })
    }

    /// Packets (u16 size header, no granule) from `start` (offset in `data`) to the end of the data present.
    /// Returns the packet payload ranges and whether the last packet was cut by the end of `media`.
    pub fn packets(&self, media: &[u8], start: u32) -> Result<(Vec<Range<usize>>, bool)> {
        let end = self.data.end.min(media.len());
        let mut pos = self.data.start + start as usize;
        let mut out = Vec::new();
        while pos < end {
            if pos + 2 > end { return Ok((out, true)); }
            let size = u16_at(media, pos)? as usize;
            let p = pos + 2..pos + 2 + size;
            if p.end > end { return Ok((out, true)); }
            out.push(p.clone());
            pos = p.end;
        }
        if pos != end { return Err(bad("packets overrun data")); }
        Ok((out, false))
    }
}

/// LSB-first bit reader (Vorbis bit packing).
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for k in 0..n {
            let byte = *self.data.get(self.pos / 8).ok_or_else(|| bad("bit stream ended"))?;
            v |= ((byte >> (self.pos % 8)) as u32 & 1) << k;
            self.pos += 1;
        }
        Ok(v)
    }
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    fn write(&mut self, n: u32, v: u32) {
        for k in 0..n {
            if self.bits.is_multiple_of(8) { self.out.push(0); }
            *self.out.last_mut().unwrap() |= ((v >> k) as u8 & 1) << (self.bits % 8);
            self.bits += 1;
        }
    }
    fn copy(&mut self, r: &mut Bits, n: u32) -> Result<u32> { let v = r.read(n)?; self.write(n, v); Ok(v) }
}

fn ilog(v: u32) -> u32 { 32 - v.leading_zeros() }

/// Vorbis `lookup1_values`: the largest `r` with `r^dimensions <= entries`.
fn quantvals(entries: u32, dimensions: u32) -> u32 {
    let mut r = (entries as f64).powf(1.0 / dimensions as f64).floor() as u32;
    loop {
        let (acc, acc1) = (0..dimensions).fold((1u64, 1u64), |(a, b), _| (a * r as u64, b * (r as u64 + 1)));
        if acc <= entries as u64 && acc1 > entries as u64 { return r; }
        if acc > entries as u64 { r -= 1 } else { r += 1 }
    }
}

/// Packed codebooks of the Wwise runtime: `data[offsets[i]..offsets[i + 1]]` is codebook `i` (E-069).
pub struct CodebookLibrary<'a> {
    pub data: &'a [u8],
    pub offsets: Vec<usize>,
}

/// orig: Borderlands2.exe (SHA-256 1231B384…4C17) — 599 increasing pointers at VA 0x19dad68 bound the 598 packed
/// codebooks (the last pointer is the end of the block, E-069).
const CODEBOOK_TABLE_VA: u32 = 0x19dad68;
const CODEBOOK_BOUNDS: usize = 599;

/// File offset of a virtual address in a PE image.
fn pe_offset(exe: &[u8], va: u32) -> Result<usize> {
    let pe = u32_at(exe, 0x3c)? as usize;
    let (sections, optional) = (u16_at(exe, pe + 6)? as usize, u16_at(exe, pe + 20)? as usize);
    let base = u32_at(exe, pe + 52)?;
    for i in 0..sections {
        let o = pe + 24 + optional + i * 40;
        let (start, size, raw) = (base + u32_at(exe, o + 12)?, u32_at(exe, o + 16)?, u32_at(exe, o + 20)?);
        if va >= start && va - start < size { return Ok((raw + va - start) as usize); }
    }
    Err(bad(format!("VA {va:#x} outside the image")))
}

impl<'a> CodebookLibrary<'a> {
    /// The library inside the user's `Borderlands2.exe` (the build must be verified first, `install::Install`).
    pub fn from_exe(exe: &'a [u8]) -> Result<Self> {
        let table = pe_offset(exe, CODEBOOK_TABLE_VA)?;
        let offsets = (0..CODEBOOK_BOUNDS).map(|i| pe_offset(exe, u32_at(exe, table + 4 * i)?)).collect::<Result<Vec<_>>>()?;
        if offsets.windows(2).any(|w| w[0] >= w[1]) { return Err(bad("codebook table is not increasing")); }
        Ok(Self { data: exe, offsets })
    }
}

impl CodebookLibrary<'_> {
    /// Expands packed codebook `id` into a standard Vorbis codebook; its stored size is `bits / 8 + 1` bytes.
    fn rebuild(&self, id: u32, w: &mut BitWriter) -> Result<()> {
        let bits = self.expand(id, w)?;
        let len = self.offsets[id as usize + 1] - self.offsets[id as usize];
        if bits / 8 + 1 != len { return Err(bad(format!("codebook {id}: {bits} bits for {len} bytes"))); }
        Ok(())
    }

    /// Expands codebook `id`; returns the packed bits read.
    fn expand(&self, id: u32, w: &mut BitWriter) -> Result<usize> {
        let i = id as usize;
        let (start, end) = self.offsets.get(i).zip(self.offsets.get(i + 1)).ok_or_else(|| bad(format!("codebook {id} not in library")))?;
        let mut r = Bits { data: &self.data[*start..*end], pos: 0 };
        let (dimensions, entries) = (r.read(4)?, r.read(14)?);
        w.write(24, 0x564342);
        w.write(16, dimensions);
        w.write(24, entries);
        if w.copy(&mut r, 1)? == 1 {
            w.copy(&mut r, 5)?;
            let mut current = 0;
            while current < entries { current += w.copy(&mut r, ilog(entries - current))?; }
            if current > entries { return Err(bad("ordered codebook overrun")); }
        } else {
            let length_bits = r.read(3)?;
            if length_bits == 0 || length_bits > 5 { return Err(bad("bad codeword length width")); }
            let sparse = w.copy(&mut r, 1)?;
            for _ in 0..entries {
                if sparse == 0 || w.copy(&mut r, 1)? == 1 { w.write(5, r.read(length_bits)?); }
            }
        }
        let lookup = r.read(1)?;
        w.write(4, lookup);
        if lookup == 1 {
            w.copy(&mut r, 32)?;
            w.copy(&mut r, 32)?;
            let value_bits = w.copy(&mut r, 4)? + 1;
            w.copy(&mut r, 1)?;
            for _ in 0..quantvals(entries, dimensions) { w.copy(&mut r, value_bits)?; }
        }
        Ok(r.pos)
    }

    /// Checks that every codebook decodes to exactly its stored length.
    pub fn verify(&self) -> Result<()> {
        (0..self.offsets.len() as u32 - 1).try_for_each(|id| self.rebuild(id, &mut BitWriter::default()))
    }
}

/// Standard Vorbis stream rebuilt from a Wwise Vorbis media: identification and setup headers, mode block flags.
pub struct Headers {
    pub ident: Vec<u8>,
    pub setup: Vec<u8>,
    pub mode_blockflags: Vec<bool>,
}

fn header(kind: u8) -> BitWriter {
    let mut w = BitWriter::default();
    w.write(8, kind as u32);
    for b in b"vorbis" { w.write(8, *b as u32); }
    w
}

impl WwiseVorbis {
    pub fn headers(&self, media: &[u8], library: &CodebookLibrary) -> Result<Headers> {
        let mut id = header(1);
        for (n, v) in [(32, 0), (8, self.channels as u32), (32, self.sample_rate), (32, 0), (32, self.avg_bytes_per_second * 8), (32, 0)] { id.write(n, v); }
        id.write(4, self.blocksize_0_pow as u32);
        id.write(4, self.blocksize_1_pow as u32);
        id.write(1, 1);

        let (setup, _) = self.packets(media, self.setup_packet_offset)?;
        let mut r = Bits { data: &media[setup.first().ok_or_else(|| bad("no setup packet"))?.clone()], pos: 0 };
        let mut w = header(5);
        let codebooks = w.copy(&mut r, 8)? + 1;
        for _ in 0..codebooks { library.rebuild(r.read(10)?, &mut w)?; }
        w.write(6, 0); // time domain transform count - 1
        w.write(16, 0);
        let book = |v: u32| if v < codebooks { Ok(()) } else { Err(bad("codebook index out of range")) };
        for _ in 0..w.copy(&mut r, 6)? + 1 {
            w.write(16, 1); // floor type 1
            let partitions = w.copy(&mut r, 5)?;
            let classes: Vec<u32> = (0..partitions).map(|_| w.copy(&mut r, 4)).collect::<Result<_>>()?;
            let mut dims = Vec::new();
            for _ in 0..classes.iter().max().map_or(0, |m| m + 1) {
                dims.push(w.copy(&mut r, 3)? + 1);
                let subclasses = w.copy(&mut r, 2)?;
                if subclasses != 0 { book(w.copy(&mut r, 8)?)?; }
                for _ in 0..1 << subclasses { let b = w.copy(&mut r, 8)?; if b != 0 { book(b - 1)?; } }
            }
            w.copy(&mut r, 2)?;
            let range_bits = w.copy(&mut r, 4)?;
            for c in &classes { for _ in 0..dims[*c as usize] { w.copy(&mut r, range_bits)?; } }
        }
        for _ in 0..w.copy(&mut r, 6)? + 1 {
            let kind = r.read(2)?;
            if kind > 2 { return Err(bad("bad residue type")); }
            w.write(16, kind);
            w.copy(&mut r, 24)?;
            w.copy(&mut r, 24)?;
            w.copy(&mut r, 24)?;
            let classifications = w.copy(&mut r, 6)? + 1;
            book(w.copy(&mut r, 8)?)?;
            let cascade: Vec<u32> = (0..classifications).map(|_| {
                let low = w.copy(&mut r, 3)?;
                let high = if w.copy(&mut r, 1)? == 1 { w.copy(&mut r, 5)? } else { 0 };
                Ok(high * 8 + low)
            }).collect::<Result<_>>()?;
            for c in cascade { for j in 0..8 { if c & 1 << j != 0 { book(w.copy(&mut r, 8)?)?; } } }
        }
        for _ in 0..w.copy(&mut r, 6)? + 1 {
            w.write(16, 0); // mapping type 0
            let submaps = if w.copy(&mut r, 1)? == 1 { w.copy(&mut r, 4)? + 1 } else { 1 };
            if w.copy(&mut r, 1)? == 1 {
                let width = ilog(self.channels as u32 - 1);
                for _ in 0..w.copy(&mut r, 8)? + 1 { w.copy(&mut r, width)?; w.copy(&mut r, width)?; }
            }
            if w.copy(&mut r, 2)? != 0 { return Err(bad("mapping reserved bits")); }
            if submaps > 1 { for _ in 0..self.channels { w.copy(&mut r, 4)?; } }
            for _ in 0..submaps { w.copy(&mut r, 8)?; w.copy(&mut r, 8)?; w.copy(&mut r, 8)?; }
        }
        let mut mode_blockflags = Vec::new();
        for _ in 0..w.copy(&mut r, 6)? + 1 {
            mode_blockflags.push(w.copy(&mut r, 1)? == 1);
            w.write(16, 0);
            w.write(16, 0);
            w.copy(&mut r, 8)?;
        }
        w.write(1, 1);
        // The setup packet must be consumed to its last byte.
        if r.pos.div_ceil(8) != r.data.len() { return Err(bad(format!("setup packet: {} bits for {} bytes", r.pos, r.data.len()))); }
        Ok(Headers { ident: id.out, setup: w.out, mode_blockflags })
    }

    /// Whether audio packets lack the Vorbis packet-type bit and window flags (every signal but 0x4A/0x4B here).
    pub fn modified_packets(&self) -> bool { !matches!(self.mod_signal, 0x4a | 0x4b | 0x69 | 0x70) }

    /// Standard Vorbis audio packets.
    pub fn audio_packets(&self, media: &[u8], headers: &Headers) -> Result<Vec<Vec<u8>>> {
        let (packets, _) = self.packets(media, self.first_audio_packet_offset)?;
        if !self.modified_packets() { return Ok(packets.iter().map(|p| media[p.clone()].to_vec()).collect()); }
        let mode_bits = ilog(headers.mode_blockflags.len() as u32 - 1);
        let long = |p: &[u8]| -> Result<bool> {
            let m = Bits { data: p, pos: 0 }.read(mode_bits)? as usize;
            headers.mode_blockflags.get(m).copied().ok_or_else(|| bad("mode number out of range"))
        };
        let mut prev_long = false;
        packets.iter().enumerate().map(|(i, p)| {
            let p = &media[p.clone()];
            let mut r = Bits { data: p, pos: 0 };
            let mut w = BitWriter::default();
            w.write(1, 0);
            let is_long = long(p)?;
            w.copy(&mut r, mode_bits)?;
            if is_long {
                let next_long = match packets.get(i + 1) { Some(n) if !n.is_empty() => long(&media[n.clone()])?, _ => false };
                w.write(1, prev_long as u32);
                w.write(1, next_long as u32);
            }
            prev_long = is_long;
            for _ in mode_bits..p.len() as u32 * 8 { w.copy(&mut r, 1)?; }
            Ok(w.out)
        }).collect()
    }
}

/// Decoded media: samples per channel, and whether the media held only part of the stream (a bank's prefetch copy).
pub struct Pcm {
    pub channels: Vec<Vec<f32>>,
    pub partial: bool,
}

impl WwiseVorbis {
    /// Decodes the media to PCM (lewton decodes the rebuilt standard Vorbis packets), trimmed to `sample_count`.
    pub fn decode(&self, media: &[u8], library: &CodebookLibrary) -> Result<Pcm> {
        let lewton = |e: &dyn std::fmt::Debug| bad(format!("vorbis: {e:?}"));
        let h = self.headers(media, library)?;
        let ident = lewton::header::read_header_ident(&h.ident).map_err(|e| lewton(&e))?;
        let setup = lewton::header::read_header_setup(&h.setup, self.channels as u8, (self.blocksize_0_pow, self.blocksize_1_pow)).map_err(|e| lewton(&e))?;
        let mut pwr = lewton::audio::PreviousWindowRight::new();
        let mut channels = vec![Vec::new(); self.channels as usize];
        for p in self.audio_packets(media, &h)? {
            let block: Vec<Vec<f32>> = lewton::audio::read_audio_packet_generic(&ident, &setup, &p, &mut pwr).map_err(|e| lewton(&e))?;
            for (out, b) in channels.iter_mut().zip(block) { out.extend(b); }
        }
        let partial = self.data.end > media.len();
        if !partial {
            let n = channels[0].len();
            // The last packet may decode past the end of the stream; more than one long block short is an error.
            if n < self.sample_count as usize || n - self.sample_count as usize >= 1 << self.blocksize_1_pow {
                return Err(bad(format!("decoded {n} samples, header says {}", self.sample_count)));
            }
            for c in &mut channels { c.truncate(self.sample_count as usize); }
        }
        Ok(Pcm { channels, partial })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup1_values() {
        // Vorbis I spec 9.2.3: largest r with r^dims <= entries.
        assert_eq!(quantvals(81, 4), 3);
        assert_eq!(quantvals(80, 4), 2);
        assert_eq!(quantvals(625, 4), 5);
        assert_eq!(quantvals(1, 1), 1);
    }

    #[test]
    fn bits_round_trip() {
        let mut w = BitWriter::default();
        w.write(3, 5);
        w.write(10, 0x2ab);
        w.write(1, 1);
        let mut r = Bits { data: &w.out, pos: 0 };
        assert_eq!([r.read(3).unwrap(), r.read(10).unwrap(), r.read(1).unwrap()], [5, 0x2ab, 1]);
    }
}
