//! Remaster textures of the Mancana DLC (T-034, plan/03-evidence.md E-006, E-075): `Mancana_Textures_Meta.upk` is an
//! index (FString data file name, u32 count, per texture FString object path, u64 offset, u64 size into the
//! data file, then three u32); each record of `Mancana_Textures_Data.upk` is an i32 NetIndex and a property stream whose tag names, types, struct and enum
//! names are FStrings instead of name-table references, ending with `None`, then the usual `Texture2D` native data.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::Reader;
use crate::texture::Texture2DData;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: String,
    pub data: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Index {
    pub data_file: String,
    pub entries: Vec<IndexEntry>,
    pub trailer: [u32; 3],
}

impl Index {
    pub fn read(meta: &[u8]) -> Result<Self> {
        let mut r = Reader::new(meta, 0);
        let data_file = r.fstring()?;
        let entries = (0..r.count()?).map(|_| {
            let path = r.fstring()?;
            let (offset, size) = (usize::try_from(r.u64()?).map_err(|_| bad("offset"))?, usize::try_from(r.u64()?).map_err(|_| bad("size"))?);
            Ok(IndexEntry { path, data: offset..offset + size })
        }).collect::<Result<_>>()?;
        // Three u32 after the entries (1, 0, 0 in the game's file; meaning unknown).
        let trailer = [r.u32()?, r.u32()?, r.u32()?];
        if r.pos != meta.len() { return Err(bad(format!("{} bytes after the Mancana index", meta.len() - r.pos))); }
        Ok(Self { data_file, entries, trailer })
    }
}

/// One tag of a record: name, type, array index, the struct/enum name of struct/byte tags, the bool value, and the
/// value bytes.
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub type_name: String,
    pub array_index: i32,
    pub inner: Option<String>,
    pub value: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Record {
    /// The object's NetIndex (−1 or a small index), as in an export's object header.
    pub net_index: i32,
    pub tags: Vec<Tag>,
    pub texture: Texture2DData,
}

impl Record {
    pub fn read(data: &[u8], range: Range<usize>) -> Result<Self> {
        let end = range.end;
        let mut r = Reader::new(data.get(..end).ok_or_else(|| bad("record past the data file"))?, range.start);
        let net_index = r.i32()?;
        let mut tags = Vec::new();
        loop {
            let name = r.fstring()?;
            if name == "None" { break; }
            let type_name = r.fstring()?;
            let (size, array_index) = (r.count()?, r.i32()?);
            let inner = match type_name.as_str() {
                "StructProperty" | "ByteProperty" => Some(r.fstring()?),
                "BoolProperty" => Some(r.bytes(1)?[0].to_string()),
                _ => None,
            };
            let start = r.pos;
            r.bytes(size)?;
            tags.push(Tag { name, type_name, array_index, inner, value: start..r.pos });
        }
        let texture = Texture2DData::read(&mut r, false)?;
        if r.pos != end { return Err(bad(format!("{} bytes after a Mancana texture record", end as i64 - r.pos as i64))); }
        Ok(Self { net_index, tags, texture })
    }

    /// Value of an FString-valued tag (`NameProperty`, `StrProperty`, or the enum value of a `ByteProperty`).
    pub fn string(&self, data: &[u8], name: &str) -> Option<String> {
        let t = self.tags.iter().find(|t| t.name == name)?;
        Reader::new(&data[..t.value.end], t.value.start).fstring().ok()
    }

    pub fn int(&self, data: &[u8], name: &str) -> Option<i32> {
        let t = self.tags.iter().find(|t| t.name == name && t.type_name == "IntProperty")?;
        Some(i32::from_le_bytes(data.get(t.value.clone())?.try_into().ok()?))
    }
}
