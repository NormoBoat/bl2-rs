//! Wwise data (T-020): AKPK file packages (`.pck`), sound banks inside them, and media classification.
//! Layout checked on the installation: plan/03-evidence.md E-031.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::Reader;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

/// One file in a package lookup table; `data` indexes the package bytes.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u32,
    pub language: u32,
    pub data: Range<usize>,
}

#[derive(Debug)]
pub struct FilePackage {
    pub languages: Vec<(u32, String)>,
    pub banks: Vec<Entry>,
    pub streams: Vec<Entry>,
    pub externals: Vec<Entry>,
}

#[derive(Debug)]
pub struct Bank {
    pub version: u32,
    pub id: u32,
    /// Chunk tag and absolute data range, in file order.
    pub chunks: Vec<([u8; 4], Range<usize>)>,
    /// Embedded media from DIDX/DATA: (media id, absolute range).
    pub media: Vec<(u32, Range<usize>)>,
}

fn lut(r: &mut Reader, size: usize, len: usize) -> Result<Vec<Entry>> {
    let end = r.pos + size;
    let entries = (0..r.count()?).map(|_| {
        let (id, block, size, start, language) = (r.u32()?, r.u32()?, r.u32()? as usize, r.u32()? as usize, r.u32()?);
        let offset = start.checked_mul(block as usize).ok_or_else(|| bad("entry offset overflow"))?;
        let data = offset..offset.checked_add(size).ok_or_else(|| bad("entry size overflow"))?;
        if data.end > len { return Err(bad(format!("entry {id:#x} past end of package"))); }
        Ok(Entry { id, language, data })
    }).collect::<Result<Vec<_>>>()?;
    if r.pos != end { return Err(bad("lookup table size mismatch")); }
    Ok(entries)
}

impl FilePackage {
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, 0);
        if r.bytes(4)? != b"AKPK" { return Err(bad("not an AKPK file package")); }
        let header_end = r.count()? + 8;
        if r.u32()? != 1 { return Err(bad("unsupported AKPK version")); }
        let (lang_size, bank_size, stream_size, external_size) = (r.count()?, r.count()?, r.count()?, r.count()?);
        // Language map: count, (string offset from map start, id), then NUL-terminated UTF-16 names.
        let map = r.pos;
        let languages = (0..r.count()?).map(|_| Ok((r.count()?, r.u32()?))).collect::<Result<Vec<_>>>()?
            .into_iter().map(|(offset, id)| {
                let mut s = Reader::new(&data[..map + lang_size], map + offset);
                let mut units = Vec::new();
                loop { match s.u16()? { 0 => break, u => units.push(u) } }
                Ok((id, String::from_utf16(&units).map_err(|_| bad("bad language name"))?))
            }).collect::<Result<_>>()?;
        r.pos = map + lang_size;
        let banks = lut(&mut r, bank_size, data.len())?;
        let streams = lut(&mut r, stream_size, data.len())?;
        let externals = lut(&mut r, external_size, data.len())?;
        if r.pos != header_end { return Err(bad("AKPK header size mismatch")); }
        Ok(Self { languages, banks, streams, externals })
    }
}

impl Bank {
    /// Reads a bank stored at `range` of `data`; chunks must tile the range exactly.
    pub fn read(data: &[u8], range: Range<usize>) -> Result<Self> {
        let mut r = Reader::new(data.get(..range.end).ok_or_else(|| bad("bank outside data"))?, range.start);
        let (mut version, mut id, mut chunks, mut index) = (None, 0, Vec::new(), Vec::new());
        let mut data_chunk = None;
        while r.pos < range.end {
            let tag: [u8; 4] = r.bytes(4)?.try_into().unwrap();
            let size = r.count()?;
            let start = r.pos;
            r.bytes(size)?;
            let mut c = Reader::new(&data[..r.pos], start);
            match &tag {
                b"BKHD" => { version = Some(c.u32()?); id = c.u32()?; }
                b"DIDX" => {
                    if size % 12 != 0 { return Err(bad("DIDX size")); }
                    for _ in 0..size / 12 { index.push((c.u32()?, c.count()?, c.count()?)); }
                }
                b"DATA" => data_chunk = Some(start..r.pos),
                _ => {}
            }
            chunks.push((tag, start..r.pos));
        }
        let version = version.ok_or_else(|| bad("bank without BKHD"))?;
        let media = match (data_chunk, index.is_empty()) {
            (_, true) => Vec::new(),
            (Some(d), false) => index.into_iter().map(|(id, offset, size)| {
                let m = d.start + offset..d.start + offset + size;
                if m.end > d.end { return Err(bad(format!("media {id:#x} past DATA"))); }
                Ok((id, m))
            }).collect::<Result<_>>()?,
            (None, false) => return Err(bad("DIDX without DATA")),
        };
        Ok(Self { version, id, chunks, media })
    }
}

/// Objects of a bank's HIRC chunk: (type, id, body after the id). Each object is u8 type, u32 size, u32 id, data.
pub fn hirc_objects(data: &[u8], hirc: Range<usize>) -> Result<Vec<(u8, u32, Range<usize>)>> {
    let mut r = Reader::new(data.get(..hirc.end).ok_or_else(|| bad("HIRC outside data"))?, hirc.start);
    let objects = (0..r.count()?).map(|_| {
        let kind = r.bytes(1)?[0];
        let size = r.count()?;
        let start = r.pos;
        let id = r.u32()?;
        r.bytes(size.checked_sub(4).ok_or_else(|| bad("HIRC object smaller than its id"))?)?;
        Ok((kind, id, start + 4..r.pos))
    }).collect::<Result<_>>()?;
    if r.pos != hirc.end { return Err(bad("HIRC objects do not fill the chunk")); }
    Ok(objects)
}

/// Effect object (HIRC type 18 FxShareSet / 19 FxCustom, E-077): plugin id, the plugin's parameter block, the media
/// it uses (index, media id), and the bytes after them (RTPCs).
#[derive(Debug, Clone)]
pub struct FxObject {
    pub plugin: u32,
    pub params: Range<usize>,
    pub media: Vec<(u8, u32)>,
    pub rest: Range<usize>,
}

impl FxObject {
    pub fn read(data: &[u8], body: Range<usize>) -> Result<Self> {
        let mut r = Reader::new(&data[..body.end], body.start);
        let plugin = r.u32()?;
        let n = r.count()?;
        let start = r.pos;
        r.bytes(n)?;
        let params = start..r.pos;
        let media = (0..r.bytes(1)?[0]).map(|_| Ok((r.bytes(1)?[0], r.u32()?))).collect::<Result<_>>()?;
        Ok(Self { plugin, params, media, rest: r.pos..body.end })
    }
}

/// Wwise short id of a name: 32-bit FNV-1 of the lower-case ASCII name.
pub fn short_id(name: &str) -> u32 {
    name.bytes().fold(2_166_136_261u32, |h, b| h.wrapping_mul(16_777_619) ^ b.to_ascii_lowercase() as u32)
}

/// Media codec: RIFF/WAVE `fmt ` format tag (e.g. `0xFFFF` Wwise Vorbis, `0x0002` ADPCM, `0x0001` PCM),
/// or `None` when the media is not a RIFF/WAVE file.
pub fn wave_format(media: &[u8]) -> Option<u16> {
    if media.get(..4)? != b"RIFF" || media.get(8..12)? != b"WAVE" { return None; }
    let mut pos = 12;
    while pos + 8 <= media.len() {
        let size = u32::from_le_bytes(media[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if &media[pos..pos + 4] == b"fmt " { return Some(u16::from_le_bytes(media.get(pos + 8..pos + 10)?.try_into().unwrap())); }
        pos = pos.checked_add(8 + size + (size & 1))?;
    }
    None
}

/// Cue-point labels of a RIFF/WAVE media (`LIST`/`adtl`/`labl` before `data`): (cue id, raw label bytes — text
/// labels end with NUL, binary ones do not). Labels are music markers (`Tempo: 135.0`) or FaceFX animations (E-070).
pub fn cue_labels(media: &[u8]) -> Result<Vec<(u32, &[u8])>> {
    let mut out = Vec::new();
    let mut pos = 12;
    while pos + 8 <= media.len() {
        let size = u32::from_le_bytes(media[pos + 4..pos + 8].try_into().unwrap()) as usize;
        // A bank's prefetch copy cuts the data chunk short; labels come before it.
        if &media[pos..pos + 4] == b"data" { break; }
        let body = media.get(pos + 8..pos + 8 + size).ok_or_else(|| bad("RIFF chunk past media"))?;
        match &media[pos..pos + 4] {
            b"LIST" if body.get(..4) == Some(b"adtl") => {
                let mut r = Reader::new(body, 4);
                while r.pos < body.len() {
                    let (tag, n) = (r.bytes(4)?, r.count()?);
                    let sub = r.bytes(n)?;
                    if n & 1 == 1 && r.pos < body.len() { r.bytes(1)?; }
                    if tag == b"labl" {
                        let id = u32::from_le_bytes(sub.get(..4).ok_or_else(|| bad("short labl"))?.try_into().unwrap());
                        out.push((id, &sub[4..]));
                    }
                }
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    Ok(out)
}

/// FaceFX animation carried in a cue label: `GBX` 0x01, u32 size, then that many bytes — tag `FACE` and the FaceFX
/// archive (which itself starts with `FACE`). Returns the archive.
pub fn facefx_animation(label: &[u8]) -> Option<&[u8]> {
    let rest = label.strip_prefix(b"GBX\x01")?;
    let n = u32::from_le_bytes(rest.get(..4)?.try_into().unwrap()) as usize;
    let archive = rest.get(4..4 + n)?.strip_prefix(b"FACE")?;
    archive.starts_with(b"FACE").then_some(archive)
}

/// Impulse response media of the Convolution Reverb plugin (0x007F0003), as loaded by `CAkConvolutionReverbFX::Init`
/// (exe 0x13546a0 → 0x1355ff0, E-071): 12 u32 header, u16 per-partition table padded to 16 bytes, then for each
/// channel `spectrum_len` complex values (two f32).
#[derive(Debug, Clone)]
pub struct ConvolutionIr {
    /// Header words; [0] block size, [1] FFT size, [2] sample rate, [3] channels, [7] table length,
    /// [8] complex values per channel (meanings of the others in E-071).
    pub header: [u32; 12],
    pub table: Vec<u16>,
    /// Per channel: complex values as (re, im).
    pub channels: Vec<Range<usize>>,
}

impl ConvolutionIr {
    pub fn read(media: &[u8]) -> Result<Self> {
        let mut r = Reader::new(media, 0);
        let mut header = [0; 12];
        for h in &mut header { *h = r.u32()?; }
        let (channels, table_len, spectrum) = (header[3] as usize, header[7] as usize, header[8] as usize);
        let table = (0..table_len).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
        r.pos = 48 + (table_len * 2).next_multiple_of(16);
        let channels = (0..channels).map(|_| { let s = r.pos; r.bytes(spectrum.checked_mul(8).ok_or_else(|| bad("IR size overflow"))?)?; Ok(s..r.pos) }).collect::<Result<_>>()?;
        if r.pos != media.len() { return Err(bad(format!("IR has {} trailing bytes", media.len() as i64 - r.pos as i64))); }
        Ok(Self { header, table, channels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1_short_ids() {
        assert_eq!(short_id(""), 2_166_136_261);
        assert_eq!(short_id("a"), 0x050c5d7e); // FNV-1 32-bit of "a"
        assert_eq!(short_id("A"), short_id("a"));
    }

    #[test]
    fn cue_labels_with_facefx() {
        let chunk = |tag: &[u8], body: &[u8]| [tag, &(body.len() as u32).to_le_bytes(), body, if body.len() % 2 == 1 { &[0u8][..] } else { &[][..] }].concat();
        let face = [b"FACE".as_slice(), b"FACE", &1740u32.to_le_bytes()].concat();
        let gbx = [b"GBX\x01".as_slice(), &(face.len() as u32).to_le_bytes(), &face].concat();
        let list = [b"adtl".as_slice(), &chunk(b"labl", &[&1u32.to_le_bytes(), b"Tempo: 1\0".as_slice()].concat()), &chunk(b"labl", &[&2u32.to_le_bytes(), gbx.as_slice()].concat())].concat();
        // The data chunk may declare more bytes than present (prefetch copy).
        let media = [b"RIFF\0\0\0\0WAVE".as_slice(), &chunk(b"LIST", &list), b"data\xff\xff\0\0"].concat();
        let labels = cue_labels(&media).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0], (1, b"Tempo: 1\0".as_slice()));
        assert_eq!(facefx_animation(labels[0].1), None);
        assert_eq!(facefx_animation(labels[1].1), Some(&face[4..]));
    }

    #[test]
    fn wave_format_tags() {
        let mut w = b"RIFF\0\0\0\0WAVEJUNK\x02\0\0\0abfmt \x10\0\0\0".to_vec();
        w.extend_from_slice(&0xffffu16.to_le_bytes());
        assert_eq!(wave_format(&w), Some(0xffff));
        assert_eq!(wave_format(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(wave_format(&[0, 4, 0, 0]), None);
    }

    #[test]
    fn package_and_bank() {
        let mut p = b"AKPK".to_vec();
        let body: Vec<u32> = vec![1, 20, 4 + 20, 4, 4, 1, 12, 7, 0, 0, 1, 0x1234, 1, 12, 100, 0, 0, 0];
        p.extend_from_slice(&((body.len() * 4) as u32).to_le_bytes());
        for v in &body { p.extend_from_slice(&v.to_le_bytes()); }
        // language name "sfx" occupies the language map's last 8 bytes
        let lang = 8 + 4 * 5 + 12;
        p[lang..lang + 8].copy_from_slice(&[b's', 0, b'f', 0, b'x', 0, 0, 0]);
        p.resize(112, 0);
        let pkg = FilePackage::read(&p).unwrap();
        assert_eq!(pkg.languages, [(7, "sfx".to_owned())]);
        assert_eq!(pkg.banks[0].id, 0x1234);
        assert_eq!(pkg.banks[0].data, 100..112);
        assert!(pkg.streams.is_empty() && pkg.externals.is_empty());
        p.truncate(110);
        assert!(FilePackage::read(&p).is_err()); // bank past end

        let mut b = Vec::new();
        for (tag, data) in [(b"BKHD", vec![62u32, 9]), (b"DIDX", vec![5, 0, 3]), (b"DATA", vec![0xaabbcc])] {
            b.extend_from_slice(tag);
            b.extend_from_slice(&((data.len() * 4) as u32).to_le_bytes());
            for v in data { b.extend_from_slice(&v.to_le_bytes()); }
        }
        let bank = Bank::read(&b, 0..b.len()).unwrap();
        assert_eq!((bank.version, bank.id, bank.media.len()), (62, 9, 1));
        assert_eq!(bank.media[0], (5, 44..47));
        assert!(Bank::read(&b, 0..b.len() - 1).is_err());
    }
}
