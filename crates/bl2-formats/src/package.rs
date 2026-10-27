//! UE3 package (`.upk`) reader for Borderlands 2: file version 832, licensee 46.
//! Layout evidence: plan/03-evidence.md E-005, E-040, E-041, E-056.

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use crate::lzo;

pub const TAG: u32 = 0x9E2A_83C1;
pub const FILE_VERSION: u16 = 832;
pub const LICENSEE_VERSION: u16 = 46;
const COMPRESS_LZO: u32 = 2;

fn bad(msg: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, msg.into())
}

/// Bounds-checked little-endian reader over a byte slice.
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| bad("read offset overflow"))?;
        let s = self.data.get(self.pos..end).ok_or_else(|| bad(format!("read of {n} bytes at {} past end {}", self.pos, self.data.len())))?;
        self.pos += n;
        Ok(s)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    /// Non-negative i32 used as a count or offset.
    pub fn count(&mut self) -> Result<usize> {
        let v = self.i32()?;
        usize::try_from(v).map_err(|_| bad(format!("negative count/offset {v} at {}", self.pos - 4)))
    }
    /// UE3 FString: positive length = Latin-1 with NUL, negative = UTF-16 with NUL.
    pub fn fstring(&mut self) -> Result<String> {
        let n = self.i32()?;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let s = self.bytes(n as usize)?;
            if s.last() != Some(&0) {
                return Err(bad("FString lacks NUL terminator"));
            }
            Ok(s[..s.len() - 1].iter().map(|&b| b as char).collect())
        } else {
            let units = n.unsigned_abs() as usize;
            let s = self.bytes(units * 2)?;
            let w: Vec<u16> = s.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            if w.last() != Some(&0) {
                return Err(bad("UTF-16 FString lacks NUL terminator"));
            }
            String::from_utf16(&w[..units - 1]).map_err(|e| bad(format!("bad UTF-16 FString: {e}")))
        }
    }
    pub fn name(&mut self) -> Result<NameRef> {
        Ok(NameRef { index: self.count()?, number: self.i32()? })
    }
}

/// Reference into the name table; `number` > 0 means the UE3 instance suffix `_{number-1}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameRef {
    pub index: usize,
    pub number: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Generation {
    pub export_count: i32,
    pub name_count: i32,
    pub net_object_count: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ChunkInfo {
    pub uncompressed_offset: usize,
    pub uncompressed_size: usize,
    pub compressed_offset: usize,
    pub compressed_size: usize,
}

/// `FPackageFileSummary` fields up to the compressed-chunk table.
/// Names after `depends_offset` follow UE3 v832 conventions; only their consistency is verified (E-041).
#[derive(Clone, Debug)]
pub struct Summary {
    pub total_header_size: usize,
    pub folder_name: String,
    pub package_flags: u32,
    pub name_count: usize,
    pub name_offset: usize,
    pub export_count: usize,
    pub export_offset: usize,
    pub import_count: usize,
    pub import_offset: usize,
    pub depends_offset: usize,
    pub import_export_guids_offset: i32,
    pub import_guids_count: i32,
    pub export_guids_count: i32,
    pub thumbnail_table_offset: i32,
    pub guid: [u8; 16],
    pub generations: Vec<Generation>,
    /// Not the engine version (log says 8639, E-046); meaning unknown (E-UNK-12).
    pub engine_version_field: i32,
    pub cooker_version: i32,
    pub compression_flags: u32,
    pub chunks: Vec<ChunkInfo>,
}

impl Summary {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let tag = r.u32()?;
        if tag != TAG {
            return Err(bad(format!("not a UE3 package (tag {tag:#x})")));
        }
        let (ver, lic) = (r.u16()?, r.u16()?);
        if (ver, lic) != (FILE_VERSION, LICENSEE_VERSION) {
            return Err(bad(format!("unsupported package version {ver}/{lic}")));
        }
        let total_header_size = r.count()?;
        let folder_name = r.fstring()?;
        let package_flags = r.u32()?;
        let (name_count, name_offset) = (r.count()?, r.count()?);
        let (export_count, export_offset) = (r.count()?, r.count()?);
        let (import_count, import_offset) = (r.count()?, r.count()?);
        let depends_offset = r.count()?;
        let (import_export_guids_offset, import_guids_count, export_guids_count, thumbnail_table_offset) =
            (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
        let guid = r.bytes(16)?.try_into().unwrap();
        let generations = (0..r.count()?)
            .map(|_| Ok(Generation { export_count: r.i32()?, name_count: r.i32()?, net_object_count: r.i32()? }))
            .collect::<Result<_>>()?;
        let engine_version_field = r.i32()?;
        let cooker_version = r.i32()?;
        let compression_flags = r.u32()?;
        let chunks = (0..r.count()?)
            .map(|_| {
                Ok(ChunkInfo {
                    uncompressed_offset: r.count()?,
                    uncompressed_size: r.count()?,
                    compressed_offset: r.count()?,
                    compressed_size: r.count()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            total_header_size, folder_name, package_flags, name_count, name_offset, export_count, export_offset,
            import_count, import_offset, depends_offset, import_export_guids_offset, import_guids_count,
            export_guids_count, thumbnail_table_offset, guid, generations, engine_version_field, cooker_version,
            compression_flags, chunks,
        })
    }
}

/// Decompresses one UE3 compressed chunk (header, block sizes, LZO1X blocks) starting at `data[0]`.
/// Returns the bytes and the compressed length consumed.
pub fn decompress_chunk(data: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut r = Reader::new(data, 0);
    if r.u32()? != TAG {
        return Err(bad("bad compressed chunk tag"));
    }
    let block_size = r.count()?;
    let (_compressed, uncompressed) = (r.count()?, r.count()?);
    if block_size == 0 {
        return Err(bad("zero block size"));
    }
    let blocks: Vec<(usize, usize)> = (0..uncompressed.div_ceil(block_size)).map(|_| Ok((r.count()?, r.count()?))).collect::<Result<_>>()?;
    let mut out = Vec::with_capacity(uncompressed);
    for (c, u) in blocks {
        let (block, used) = lzo::decompress(r.bytes(c)?, u)?;
        if used != c {
            return Err(bad(format!("LZO block used {used} of {c} bytes")));
        }
        out.extend_from_slice(&block);
    }
    if out.len() != uncompressed {
        return Err(bad("chunk size mismatch"));
    }
    Ok((out, r.pos))
}

#[derive(Clone, Debug)]
pub struct Import {
    pub class_package: NameRef,
    pub class_name: NameRef,
    /// UE3 package index: > 0 export `i-1`, < 0 import `-i-1`, 0 none.
    pub outer: i32,
    pub object_name: NameRef,
}

#[derive(Clone, Debug)]
pub struct Export {
    pub class: i32,
    pub super_: i32,
    pub outer: i32,
    pub object_name: NameRef,
    pub archetype: i32,
    pub object_flags: u64,
    pub serial_size: usize,
    pub serial_offset: usize,
    pub export_flags: u32,
    pub generation_net_object_counts: Vec<i32>,
    pub package_guid: [u8; 16],
    pub package_flags: u32,
}

#[derive(Debug, Default)]
pub struct CrossLevelGuids {
    pub imports: Vec<(String, Vec<[u8; 16]>)>,
    pub exports: Vec<([u8; 16], i32)>,
}

impl CrossLevelGuids {
    /// Level package and object GUID named by a cross-level reference `0xF0LLNNNN`; `None` for other values
    /// or indices outside the tables. The object is the export with that GUID in the level's own `exports`.
    pub fn import_guid(&self, raw: u32) -> Option<(&str, [u8; 16])> {
        if raw >> 28 != 0xf { return None; }
        let (level, guids) = self.imports.get(((raw >> 16) & 0xfff) as usize)?;
        Some((level, *guids.get((raw & 0xffff) as usize)?))
    }
}


pub struct Package {
    pub summary: Summary,
    /// Whole package as it would be on disk without compression.
    pub data: Vec<u8>,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl Package {
    /// Opens a package file; fully compressed files are recognised by their `.uncompressed_size` sidecar.
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::read(path)?;
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(".uncompressed_size");
        let data = match std::fs::read_to_string(&sidecar) {
            Ok(s) => {
                let expected: usize = s.trim().parse().map_err(|_| bad("bad .uncompressed_size"))?;
                let mut out = Vec::with_capacity(expected);
                let mut pos = 0;
                while pos < file.len() {
                    let (chunk, used) = decompress_chunk(&file[pos..])?;
                    out.extend_from_slice(&chunk);
                    pos += used;
                }
                if out.len() != expected {
                    return Err(bad(format!("fully compressed package: {} bytes, sidecar says {expected}", out.len())));
                }
                out
            }
            Err(e) if e.kind() == ErrorKind::NotFound => file,
            Err(e) => return Err(e),
        };
        Self::parse(data)
    }

    /// Parses package bytes, expanding chunk-compressed bodies first.
    pub fn parse(mut data: Vec<u8>) -> Result<Self> {
        let summary = Summary::read(&mut Reader::new(&data, 0))?;
        if !summary.chunks.is_empty() {
            if summary.compression_flags != COMPRESS_LZO {
                return Err(bad(format!("unsupported compression flags {:#x}", summary.compression_flags)));
            }
            data = expand_chunks(&data, &summary.chunks)?;
        }
        let names = {
            let mut r = Reader::new(&data, summary.name_offset);
            (0..summary.name_count)
                .map(|_| {
                    let s = r.fstring()?;
                    r.u64()?; // object flags of the name entry
                    Ok(s)
                })
                .collect::<Result<Vec<_>>>()?
        };
        let mut r = Reader::new(&data, summary.import_offset);
        let imports = (0..summary.import_count)
            .map(|_| Ok(Import { class_package: r.name()?, class_name: r.name()?, outer: r.i32()?, object_name: r.name()? }))
            .collect::<Result<Vec<_>>>()?;
        let mut r = Reader::new(&data, summary.export_offset);
        let exports = (0..summary.export_count)
            .map(|_| {
                Ok(Export {
                    class: r.i32()?,
                    super_: r.i32()?,
                    outer: r.i32()?,
                    object_name: r.name()?,
                    archetype: r.i32()?,
                    object_flags: r.u64()?,
                    serial_size: r.count()?,
                    serial_offset: r.count()?,
                    export_flags: r.u32()?,
                    generation_net_object_counts: (0..r.count()?).map(|_| r.i32()).collect::<Result<_>>()?,
                    package_guid: r.bytes(16)?.try_into().unwrap(),
                    package_flags: r.u32()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let export_table_end = r.pos;
        let package = Self { summary, data, names, imports, exports };
        package.validate(export_table_end)?;
        Ok(package)
    }

    /// Structural checks that would catch a wrong table layout (plan/12-tasks.md T-031).
    fn validate(&self, export_table_end: usize) -> Result<()> {
        let s = &self.summary;
        if export_table_end != s.depends_offset {
            return Err(bad(format!("export table ends at {export_table_end}, depends table starts at {}", s.depends_offset)));
        }
        let name_ok = |n: &NameRef| n.index < self.names.len();
        let index_ok = |i: i32| (i >= 0 && (i as usize) <= self.exports.len()) || (i < 0 && (i.unsigned_abs() as usize) <= self.imports.len());
        for (i, imp) in self.imports.iter().enumerate() {
            if !(name_ok(&imp.class_package) && name_ok(&imp.class_name) && name_ok(&imp.object_name) && index_ok(imp.outer)) {
                return Err(bad(format!("import {i} has an out-of-range reference")));
            }
        }
        for (i, exp) in self.exports.iter().enumerate() {
            if !(name_ok(&exp.object_name) && index_ok(exp.class) && index_ok(exp.super_) && index_ok(exp.outer) && index_ok(exp.archetype)) {
                return Err(bad(format!("export {i} has an out-of-range reference")));
            }
            if exp.serial_size > 0 && exp.serial_offset + exp.serial_size > self.data.len() {
                return Err(bad(format!("export {i} body [{}+{}] past end {}", exp.serial_offset, exp.serial_size, self.data.len())));
            }
        }
        Ok(())
    }

    /// Cross-level GUID tables at `ImportExportGuidsOffset` (E-067): import GUIDs per referenced level package
    /// (`FLevelGuids`: level name, GUIDs), then this package's export GUID -> export index (1-based) map.
    /// A cross-level object reference `0xF0LLNNNN` names GUID `NNNN` of import level `LL` (exe 0x498380/0x497d50).
    pub fn level_guids(&self) -> Result<CrossLevelGuids> {
        let s = &self.summary;
        if s.import_guids_count == 0 && s.export_guids_count == 0 { return Ok(CrossLevelGuids::default()); }
        let mut r = Reader::new(&self.data, usize::try_from(s.import_export_guids_offset).map_err(|_| bad("negative guid table offset"))?);
        let guid = |r: &mut Reader| -> Result<[u8; 16]> { Ok(r.bytes(16)?.try_into().unwrap()) };
        let imports = (0..s.import_guids_count)
            .map(|_| {
                let level = r.fstring()?;
                let n = r.count()?;
                if n > (self.data.len() - r.pos) / 16 { return Err(bad("level GUID count exceeds data")); }
                Ok((level, (0..n).map(|_| guid(&mut r)).collect::<Result<_>>()?))
            })
            .collect::<Result<_>>()?;
        let exports = (0..s.export_guids_count).map(|_| Ok((guid(&mut r)?, r.i32()?))).collect::<Result<_>>()?;
        Ok(CrossLevelGuids { imports, exports })
    }

    /// Name-table index of `None` (every cooked package has it).
    pub fn none_name(&self) -> u32 {
        self.names.iter().position(|n| n == "None").expect("package without None name") as u32
    }

    /// Object name for a package index (0 is `None`).
    pub fn object_name(&self, i: i32) -> String {
        match i {
            0 => "None".into(),
            i if i > 0 => self.name(self.exports[i as usize - 1].object_name),
            i => self.name(self.imports[(-i - 1) as usize].object_name),
        }
    }

    /// Name text with the UE3 instance-number suffix.
    pub fn name(&self, n: NameRef) -> String {
        match n.number {
            0 => self.names[n.index].clone(),
            k => format!("{}_{}", self.names[n.index], k - 1),
        }
    }
}

/// Rebuilds the uncompressed package: header bytes before the first chunk plus each decompressed chunk.
fn expand_chunks(file: &[u8], chunks: &[ChunkInfo]) -> Result<Vec<u8>> {
    let first = chunks.iter().map(|c| c.uncompressed_offset).min().unwrap();
    let total = chunks.iter().map(|c| c.uncompressed_offset + c.uncompressed_size).max().unwrap();
    let mut out = vec![0u8; total];
    out[..first].copy_from_slice(file.get(..first).ok_or_else(|| bad("header shorter than first chunk offset"))?);
    for c in chunks {
        let src = file.get(c.compressed_offset..c.compressed_offset + c.compressed_size).ok_or_else(|| bad("chunk past end of file"))?;
        let (bytes, used) = decompress_chunk(src)?;
        if bytes.len() != c.uncompressed_size || used != c.compressed_size {
            return Err(bad("chunk table sizes disagree with chunk contents"));
        }
        out[c.uncompressed_offset..c.uncompressed_offset + c.uncompressed_size].copy_from_slice(&bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_level_reference_fields() {
        let g = CrossLevelGuids { imports: vec![("A".into(), vec![[1; 16]]), ("B".into(), vec![[2; 16], [3; 16]])], exports: vec![] };
        assert_eq!(g.import_guid(0xf000_0000), Some(("A", [1; 16])));
        assert_eq!(g.import_guid(0xf001_0001), Some(("B", [3; 16])));
        assert_eq!(g.import_guid(0xf001_0002), None);
        assert_eq!(g.import_guid(0xf002_0000), None);
        assert_eq!(g.import_guid(0x0000_0005), None);
    }
}
