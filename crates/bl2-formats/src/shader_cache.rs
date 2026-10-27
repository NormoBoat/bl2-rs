//! Shader caches of the cooked game (T-019): `GlobalShaderCache-PC-D3D-SM3.bin` and the `ShaderCache` object of
//! `RefShaderCache-PC-D3D-SM3.upk`. Layout found on the installation (E-030); every record and material map ends
//! exactly at its stored end offset. Shader parameter bindings and uniform expressions are kept as byte ranges.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::{Package, Reader};

const GLOBAL_TAG: u32 = 0x4753_4D42; // "BMSG"
/// Version triple stored in the global cache header and in every material shader map.
const VERSION: (u32, u32, u32) = (832, 46, 26);

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

pub type Guid = [u8; 16];

#[derive(Debug)]
pub struct Shader {
    pub type_name: String,
    pub id: Guid,
    /// UE3 shader platform (0 = PC D3D SM3) and frequency (0 vertex, 3 pixel).
    pub platform: u8,
    pub frequency: u8,
    /// D3D9 bytecode.
    pub code: Range<usize>,
    /// Shader-type-specific parameter bindings after the code, not decoded.
    pub parameters: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub struct ShaderRef {
    pub type_name: String,
    pub id: Guid,
}

#[derive(Debug)]
pub struct MeshShaderMap {
    pub vertex_factory: String,
    pub shaders: Vec<ShaderRef>,
}

#[derive(Debug)]
pub struct MaterialShaderMap {
    /// Serialized `FStaticParameterSet` (base material id + static switch/mask/normal/terrain parameters).
    pub static_parameters: Range<usize>,
    pub material_id: Guid,
    pub friendly_name: String,
    pub shaders: Vec<ShaderRef>,
    pub mesh: Vec<MeshShaderMap>,
    /// Uniform expression set and remaining fields, not decoded (needed by T-038).
    pub rest: Range<usize>,
}

#[derive(Debug, Default)]
pub struct ShaderCache {
    pub platform: u8,
    pub type_crcs: Vec<(String, u32)>,
    pub shaders: Vec<Shader>,
    pub vertex_factory_crcs: Vec<(String, u32)>,
    pub materials: Vec<MaterialShaderMap>,
    /// Global cache only: shader type -> shader.
    pub global: Vec<ShaderRef>,
}

type NameFn<'n> = &'n dyn Fn(&mut Reader) -> Result<String>;

fn guid(r: &mut Reader) -> Result<Guid> { Ok(r.bytes(16)?.try_into().unwrap()) }

fn crcs(r: &mut Reader, name: NameFn) -> Result<Vec<(String, u32)>> {
    (0..r.count()?).map(|_| Ok((name(r)?, r.u32()?))).collect()
}

/// Absolute end offset of a record, which must lie after the reader and inside the data.
fn end_offset(r: &mut Reader) -> Result<usize> {
    let end = r.count()?;
    if end < r.pos || end > r.data.len() { return Err(bad(format!("end offset {end} outside record at {}", r.pos))); }
    Ok(end)
}

fn shaders(r: &mut Reader, name: NameFn) -> Result<Vec<Shader>> {
    (0..r.count()?).map(|_| {
        let type_name = name(r)?;
        let id = guid(r)?;
        let end = end_offset(r)?;
        let (platform, frequency) = (r.bytes(1)?[0], r.bytes(1)?[0]);
        let n = r.count()?;
        let start = r.pos;
        r.bytes(n)?;
        if r.pos > end { return Err(bad(format!("{type_name}: code past record end"))); }
        let shader = Shader { type_name, id, platform, frequency, code: start..r.pos, parameters: r.pos..end };
        r.pos = end;
        Ok(shader)
    }).collect()
}

/// TMap<FShaderType*, FShaderRef>: (type, shader id, type again).
fn refs(r: &mut Reader, name: NameFn) -> Result<Vec<ShaderRef>> {
    (0..r.count()?).map(|_| {
        let (type_name, id) = (name(r)?, guid(r)?);
        if name(r)? != type_name { return Err(bad(format!("shader map entry type mismatch: {type_name}"))); }
        Ok(ShaderRef { type_name, id })
    }).collect()
}

/// `FStaticParameterSet`; element sizes are fixed (checked by the end of every key on the installation).
fn static_parameter_set(r: &mut Reader) -> Result<Range<usize>> {
    let start = r.pos;
    r.bytes(16)?; // base material id
    // Switch {Name, Value, bOverride, ExpressionGUID}, ComponentMask {Name, R, G, B, A, bOverride, GUID},
    // Normal {Name, CompressionSettings byte, bOverride, GUID}, TerrainLayerWeight {Name, WeightmapIndex, bOverride, GUID}.
    for size in [32, 44, 29, 32] {
        let n = r.count()?;
        r.bytes(n.checked_mul(size).ok_or_else(|| bad("parameter count overflow"))?)?;
    }
    Ok(start..r.pos)
}

fn version(r: &mut Reader) -> Result<()> {
    let v = (r.u32()?, r.u32()?, r.u32()?);
    if v != VERSION { return Err(bad(format!("shader map version {v:?}"))); }
    Ok(())
}

fn materials(r: &mut Reader, name: NameFn) -> Result<Vec<MaterialShaderMap>> {
    (0..r.count()?).map(|_| {
        let static_parameters = static_parameter_set(r)?;
        version(r)?;
        let end = end_offset(r)?;
        let shaders = refs(r, name)?;
        let mesh = (0..r.count()?).map(|_| {
            let shaders = refs(r, name)?;
            Ok(MeshShaderMap { vertex_factory: name(r)?, shaders })
        }).collect::<Result<_>>()?;
        let material_id = guid(r)?;
        let friendly_name = r.fstring()?;
        let copy = static_parameter_set(r)?;
        if r.data[copy] != r.data[static_parameters.clone()] {
            return Err(bad(format!("{friendly_name}: static parameters differ from the map key")));
        }
        if r.pos > end { return Err(bad(format!("{friendly_name}: map past its end offset"))); }
        let rest = r.pos..end;
        r.pos = end;
        Ok(MaterialShaderMap { static_parameters, material_id, friendly_name, shaders, mesh, rest })
    }).collect()
}

impl ShaderCache {
    /// `GlobalShaderCache-*.bin`: offsets in the result index `data`.
    pub fn read_global(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, 0);
        if r.u32()? != GLOBAL_TAG { return Err(bad("not a global shader cache")); }
        // Unlike material maps, the header stores the licensee version and the third value as u16.
        let v = (r.u32()?, r.u16()? as u32, r.u16()? as u32);
        if v != VERSION { return Err(bad(format!("global shader cache version {v:?}"))); }
        let name: NameFn = &|r: &mut Reader| r.fstring();
        let platform = r.bytes(1)?[0];
        let type_crcs = crcs(&mut r, name)?;
        let shaders = shaders(&mut r, name)?;
        let global = refs(&mut r, name)?;
        if r.pos != data.len() { return Err(bad(format!("{} trailing bytes in global shader cache", data.len() - r.pos))); }
        Ok(Self { platform, type_crcs, shaders, global, ..Default::default() })
    }

    /// The single `ShaderCache` export of `RefShaderCache-*.upk`; offsets index `package.data`.
    pub fn read_package(package: &Package) -> Result<Self> {
        let e = package.exports.iter().find(|e| package.object_name(e.class) == "ShaderCache")
            .ok_or_else(|| bad("no ShaderCache export"))?;
        let end = e.serial_offset + e.serial_size;
        let mut r = Reader::new(&package.data[..end], e.serial_offset);
        r.i32()?; // NetIndex
        if !package.read_tags(&mut r)?.is_empty() { return Err(bad("ShaderCache has tagged properties")); }
        let name: NameFn = &|r: &mut Reader| {
            let n = r.name()?;
            if n.index >= package.names.len() || n.number < 0 { return Err(bad("invalid name reference")); }
            Ok(package.name(n))
        };
        // Meaning unknown; 0 in the only ShaderCache of the installation (E-030).
        if r.u32()? != 0 { return Err(bad("unexpected ShaderCache prefix")); }
        let platform = r.bytes(1)?[0];
        let type_crcs = crcs(&mut r, name)?;
        let shaders = shaders(&mut r, name)?;
        let vertex_factory_crcs = crcs(&mut r, name)?;
        let materials = materials(&mut r, name)?;
        if r.pos != end { return Err(bad(format!("{} trailing bytes in ShaderCache", end - r.pos))); }
        Ok(Self { platform, type_crcs, shaders, vertex_factory_crcs, materials, global: Vec::new() })
    }
}

/// D3D9 bytecode version token: (`0xFFFE` vertex / `0xFFFF` pixel, major, minor), if the code ends with END.
pub fn d3d9_version(code: &[u8]) -> Option<(u16, u8, u8)> {
    let word = |i: usize| code.get(i..i + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
    let token = word(0)?;
    (word(code.len().checked_sub(4)?)? == 0x0000_FFFF).then_some(((token >> 16) as u16, (token >> 8) as u8, token as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d3d9_version_tokens() {
        assert_eq!(d3d9_version(&[0, 3, 0xff, 0xff, 0xff, 0xff, 0, 0]), Some((0xffff, 3, 0)));
        assert_eq!(d3d9_version(&[0, 3, 0xfe, 0xff, 0xff, 0xff, 0, 0]), Some((0xfffe, 3, 0)));
        assert_eq!(d3d9_version(&[0, 3, 0xfe, 0xff, 0, 0, 0, 0]), None); // no END token
        assert_eq!(d3d9_version(&[1]), None);
    }

    #[test]
    fn rejects_bad_global_headers() {
        assert!(ShaderCache::read_global(b"XXXX").is_err());
        let mut h = GLOBAL_TAG.to_le_bytes().to_vec();
        h.extend_from_slice(&832u32.to_le_bytes());
        h.extend_from_slice(&[46, 0, 25, 0]);
        assert!(ShaderCache::read_global(&h).unwrap_err().to_string().contains("version"));
    }
}
