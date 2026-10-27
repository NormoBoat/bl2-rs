//! Texture2D native data: mip chain with inline or texture-file-cache (TFC) payloads (plan/12-tasks.md T-034).
//! Layout found on the installation: plan/03-evidence.md E-058.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::{Package, Reader};

/// UE3 bulk data flags seen in Texture2D mips.
pub const BULK_STORE_IN_SEPARATE_FILE: u32 = 0x01;
pub const BULK_COMPRESSED_LZO: u32 = 0x10;
pub const BULK_UNUSED: u32 = 0x20;

#[derive(Clone, Debug)]
pub struct BulkData {
    pub flags: u32,
    /// Uncompressed payload size in bytes (element size is 1 for texture mips).
    pub element_count: usize,
    /// -1 for unused (cooked-out) mips.
    pub size_on_disk: i64,
    /// Offset in the package (inline) or in the texture file cache (separate file); -1 when there is none.
    pub offset_in_file: i64,
    /// Inline payload range in `Package::data` (empty for separate-file or unused data).
    pub inline: Range<usize>,
}

impl BulkData {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let flags = r.u32()?;
        let element_count = r.count()?;
        let size_on_disk = r.i32()? as i64;
        let offset_in_file = r.i32()? as i64;
        let start = r.pos;
        if flags & (BULK_STORE_IN_SEPARATE_FILE | BULK_UNUSED) == 0 {
            let n = usize::try_from(size_on_disk).map_err(|_| Error::new(ErrorKind::InvalidData, "inline bulk data with negative size"))?;
            r.bytes(n)?;
        }
        Ok(Self { flags, element_count, size_on_disk, offset_in_file, inline: start..r.pos })
    }
}

#[derive(Clone, Debug)]
pub struct Mip {
    pub data: BulkData,
    pub size_x: u32,
    pub size_y: u32,
}

/// Native part of a Texture2D after its tagged properties.
#[derive(Clone, Debug)]
pub struct Texture2DData {
    /// Meanings not established (two look like TFC identifiers); kept raw (E-058).
    pub guid_before_mips: [u8; 16],
    pub mips: Vec<Mip>,
    pub guid_after_mips: [u8; 16],
    pub guid_last: [u8; 16],
    pub trailing_count: i32,
    /// LightMapTexture2D only.
    pub lightmap_flags: Option<u32>,
}

impl Texture2DData {
    pub fn read(r: &mut Reader, is_lightmap: bool) -> Result<Self> {
        let guid_before_mips = r.bytes(16)?.try_into().unwrap();
        let mips = (0..r.count()?)
            .map(|_| Ok(Mip { data: BulkData::read(r)?, size_x: r.u32()?, size_y: r.u32()? }))
            .collect::<Result<_>>()?;
        Ok(Self {
            guid_before_mips,
            mips,
            guid_after_mips: r.bytes(16)?.try_into().unwrap(),
            guid_last: r.bytes(16)?.try_into().unwrap(),
            trailing_count: r.i32()?,
            lightmap_flags: if is_lightmap { Some(r.u32()?) } else { None },
        })
    }
}

/// Bytes of one mip for a UE3 pixel format name (`EPixelFormat` value as stored in the `Format` property).
pub fn mip_bytes(format: &str, w: u32, h: u32) -> Option<usize> {
    let (w, h) = (w as usize, h as usize);
    let blocks = w.div_ceil(4).max(1) * h.div_ceil(4).max(1);
    Some(match format {
        "PF_DXT1" => blocks * 8,
        "PF_DXT3" | "PF_DXT5" | "PF_BC5" => blocks * 16,
        "PF_A8R8G8B8" => w * h * 4,
        "PF_G8" | "PF_A8" => w * h,
        "PF_V8U8" | "PF_G16" => w * h * 2,
        "PF_A16B16G16R16" | "PF_FloatRGBA" => w * h * 8,
        "PF_A32B32G32R32F" => w * h * 16,
        _ => return None,
    })
}

/// Reads a mip payload stored in a texture file cache: LZO-compressed UE3 chunk or raw bytes.
pub fn read_tfc_mip(tfc: &[u8], d: &BulkData) -> Result<Vec<u8>> {
    let start = usize::try_from(d.offset_in_file).map_err(|_| Error::new(ErrorKind::InvalidData, "mip has no offset"))?;
    let len = usize::try_from(d.size_on_disk).map_err(|_| Error::new(ErrorKind::InvalidData, "mip has no size"))?;
    let src = tfc
        .get(start..start + len)
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "mip past end of texture file cache"))?;
    let bytes = if d.flags & BULK_COMPRESSED_LZO != 0 { crate::package::decompress_chunk(src)?.0 } else { src.to_vec() };
    if bytes.len() != d.element_count {
        return Err(Error::new(ErrorKind::InvalidData, format!("TFC mip has {} bytes, expected {}", bytes.len(), d.element_count)));
    }
    Ok(bytes)
}

impl Package {
    /// Value of an enum (`ByteProperty` with an enum) or `NameProperty` tag: the stored name.
    pub fn tag_name_value(&self, tag: &crate::properties::Tag) -> Result<String> {
        let mut r = Reader::new(&self.data, tag.value.start);
        Ok(self.name(r.name()?))
    }
}
