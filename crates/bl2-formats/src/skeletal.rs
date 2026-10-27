//! Skeletal meshes (T-036): native data of `USkeletalMesh` for package version 832, as loaded by
//! `USkeletalMesh::Serialize` (vtable slot 13, exe 0xa5e4a0; plan/03-evidence.md E-074).

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::{NameRef, Reader};
use crate::texture::BulkData;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

fn floats<const N: usize>(r: &mut Reader) -> Result<[f32; N]> {
    let mut v = [0.0; N];
    for x in &mut v { *x = f32::from_bits(r.u32()?); }
    Ok(v)
}

fn array<T>(r: &mut Reader, mut item: impl FnMut(&mut Reader) -> Result<T>) -> Result<Vec<T>> {
    let n = r.count()?;
    if n > r.data.len().saturating_sub(r.pos) { return Err(bad(format!("array count {n} exceeds data at {}", r.pos - 4))); }
    (0..n).map(|_| item(r)).collect()
}

/// `TArray` of fixed-size elements, kept as a byte range.
fn raw_array(r: &mut Reader, element: usize) -> Result<Range<usize>> {
    let n = r.count()?;
    let start = r.pos;
    r.bytes(n.checked_mul(element).ok_or_else(|| bad("array size overflow"))?)?;
    Ok(start..r.pos)
}

/// `BulkSerialize`d array: element size, count, elements.
fn bulk(r: &mut Reader) -> Result<(usize, Range<usize>)> {
    let (size, count) = (r.count()?, r.count()?);
    let start = r.pos;
    r.bytes(size.checked_mul(count).ok_or_else(|| bad("bulk size overflow"))?)?;
    Ok((size, start..r.pos))
}

/// `FMeshBone` (0xa1bfa0): name, flags, `VJointPos` (0xa1bf30: rotation quaternion, position), children, parent,
/// a u32 (version ≥ 0x203).
#[derive(Debug, Clone)]
pub struct Bone {
    pub name: NameRef,
    pub flags: u32,
    pub rotation: [f32; 4],
    pub position: [f32; 3],
    pub num_children: i32,
    pub parent: i32,
    pub color: u32,
}

/// `FSkelMeshSection` (0xa1c030): material, chunk, first index, triangles, triangle sorting (u8).
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub material: u16,
    pub chunk: u16,
    pub base_index: u32,
    pub triangles: u32,
    pub triangle_sorting: u8,
}

/// `FSkelMeshChunk` (0xa4aa40): first vertex, rigid vertices (61 bytes: position, 3 packed normals, 4 UVs, color,
/// bone), soft vertices (68 bytes: … 4 bones, 4 weights), bone map, counts.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub base_vertex: u32,
    pub rigid_vertices: Range<usize>,
    pub soft_vertices: Range<usize>,
    pub bone_map: Vec<u16>,
    pub num_rigid: u32,
    pub num_soft: u32,
    pub max_influences: u32,
}

/// GPU skin vertex buffer (0xa562a0).
#[derive(Debug, Clone)]
pub struct VertexBuffer {
    pub num_tex_coords: u32,
    pub use_full_precision_uvs: u32,
    pub use_packed_position: u32,
    pub mesh_extension: [f32; 3],
    pub mesh_origin: [f32; 3],
    pub vertex_size: usize,
    pub vertices: Range<usize>,
}

/// One GPU skin vertex (the buffer is a raw copy of the vertex structs): packed tangent X and Z, 4 bone indices
/// (into the chunk's bone map), 4 weights (sum 255), position, UVs (f16 pairs, or f32 pairs when the vertex is
/// 28 + 8·UV bytes). Layout checked on all 9 118 703 vertices of the game (E-074), not yet against the vertex struct code.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuVertex {
    pub tangent_x: u32,
    pub tangent_z: u32,
    pub bones: [u8; 4],
    pub weights: [u8; 4],
    pub position: [f32; 3],
    pub uvs: Vec<[f32; 2]>,
}

/// IEEE half to f32.
fn half(h: u16) -> f32 {
    let (sign, exp, frac) = ((h >> 15) as u32, (h >> 10 & 0x1f) as i32, (h & 0x3ff) as u32);
    let mag = match exp {
        0 => frac as f32 * f32::powi(2.0, -24),
        31 => if frac == 0 { f32::INFINITY } else { f32::NAN },
        e => f32::from_bits(((e + 112) as u32) << 23 | frac << 13),
    };
    if sign == 1 { -mag } else { mag }
}

impl VertexBuffer {
    pub fn vertex(&self, data: &[u8], i: usize) -> Option<GpuVertex> {
        let v = data.get(self.vertices.start + i * self.vertex_size..self.vertices.start + (i + 1) * self.vertex_size)?;
        let u32_at = |o: usize| u32::from_le_bytes(v[o..o + 4].try_into().unwrap());
        let n = self.num_tex_coords as usize;
        let full = self.vertex_size == 28 + 8 * n;
        let uvs = (0..n).map(|k| if full {
            [f32::from_bits(u32_at(28 + k * 8)), f32::from_bits(u32_at(32 + k * 8))]
        } else {
            [half(u16::from_le_bytes([v[28 + k * 4], v[29 + k * 4]])), half(u16::from_le_bytes([v[30 + k * 4], v[31 + k * 4]]))]
        }).collect();
        Some(GpuVertex {
            tangent_x: u32_at(0), tangent_z: u32_at(4), bones: v[8..12].try_into().unwrap(), weights: v[12..16].try_into().unwrap(),
            position: [f32::from_bits(u32_at(16)), f32::from_bits(u32_at(20)), f32::from_bits(u32_at(24))], uvs,
        })
    }
}

/// `FStaticLODModel` (0xa5d0e0).
#[derive(Debug, Clone)]
pub struct LodModel {
    pub sections: Vec<Section>,
    pub index_cpu_access: u32,
    /// Bytes per index (2 or 4) and the index data (`FMultiSizeIndexContainer` 0xa2f210).
    pub index_size: usize,
    pub indices: Range<usize>,
    pub active_bone_indices: Vec<u16>,
    pub chunks: Vec<Chunk>,
    pub size: u32,
    pub num_vertices: u32,
    pub required_bones: Range<usize>,
    pub raw_point_indices: BulkData,
    pub num_tex_coords: u32,
    pub vertex_buffer: VertexBuffer,
    /// Vertex colours (only when the mesh has vertex colours): element size and data.
    pub colors: Option<(usize, Range<usize>)>,
    pub vertex_influences: usize,
}

#[derive(Debug, Clone)]
pub struct SkeletalMesh {
    /// Origin, box extent, sphere radius.
    pub bounds: [f32; 7],
    pub materials: Vec<i32>,
    pub origin: [f32; 3],
    pub rot_origin: [i32; 3],
    pub bones: Vec<Bone>,
    pub skeletal_depth: i32,
    pub lods: Vec<LodModel>,
    pub name_index_map: Vec<(NameRef, i32)>,
    pub bone_break_names: Vec<String>,
    pub bone_break_options: Range<usize>,
    pub clothing_assets: Vec<i32>,
    pub streaming_factors: Vec<f32>,
}

fn u16_array(r: &mut Reader) -> Result<Vec<u16>> { array(r, |r| r.u16()) }

impl SkeletalMesh {
    /// Reads the native data after the tagged properties. `has_vertex_colors` is the mesh's `bHasVertexColors`
    /// (the original tests a flag of the mesh, +0x374 bit 1, before reading a colour buffer per LOD).
    pub fn read(r: &mut Reader, end: usize, has_vertex_colors: bool) -> Result<Self> {
        let bounds = floats(r)?;
        let materials = array(r, |r| r.i32())?;
        let origin = floats(r)?;
        let rot_origin = [r.i32()?, r.i32()?, r.i32()?];
        let bones = array(r, |r| {
            let name = r.name()?;
            let flags = r.u32()?;
            let j: [f32; 7] = floats(r)?;
            Ok(Bone { name, flags, rotation: [j[0], j[1], j[2], j[3]], position: [j[4], j[5], j[6]], num_children: r.i32()?, parent: r.i32()?, color: r.u32()? })
        })?;
        let skeletal_depth = r.i32()?;
        let lods = array(r, |r| Self::lod(r, has_vertex_colors))?;
        // NameIndexMap (TMap<FName, i32>, 0x494c60 → 0x4913b0): bone name → bone index.
        let name_index_map = array(r, |r| Ok((r.name()?, r.i32()?)))?;
        // PerPolyBoneKDOPs (0xa50cd0): not decoded yet.
        let kdops = r.count()?;
        if kdops != 0 { return Err(bad(format!("{kdops} PerPolyBoneKDOPs"))); }
        let bone_break_names = array(r, |r| r.fstring())?;
        let bone_break_options = raw_array(r, 1)?;
        let clothing_assets = array(r, |r| r.i32())?;
        let streaming_factors = array(r, |r| Ok(f32::from_bits(r.u32()?)))?;
        if r.pos != end { return Err(bad(format!("{} bytes after SkeletalMesh native data", end as i64 - r.pos as i64))); }
        Ok(Self { bounds, materials, origin, rot_origin, bones, skeletal_depth, lods, name_index_map, bone_break_names, bone_break_options, clothing_assets, streaming_factors })
    }

    fn lod(r: &mut Reader, has_vertex_colors: bool) -> Result<LodModel> {
        let sections = array(r, |r| Ok(Section { material: r.u16()?, chunk: r.u16()?, base_index: r.u32()?, triangles: r.u32()?, triangle_sorting: r.bytes(1)?[0] }))?;
        let index_cpu_access = r.u32()?;
        let index_size = r.bytes(1)?[0] as usize;
        let (stride, indices) = bulk(r)?;
        if stride != index_size { return Err(bad(format!("index element size {stride} for a {index_size}-byte container"))); }
        let active_bone_indices = u16_array(r)?;
        let chunks = array(r, |r| Ok(Chunk {
            base_vertex: r.u32()?, rigid_vertices: raw_array(r, 61)?, soft_vertices: raw_array(r, 68)?, bone_map: u16_array(r)?,
            num_rigid: r.u32()?, num_soft: r.u32()?, max_influences: r.u32()?,
        }))?;
        let (size, num_vertices) = (r.u32()?, r.u32()?);
        let required_bones = raw_array(r, 1)?;
        let raw_point_indices = BulkData::read(r)?;
        let num_tex_coords = r.u32()?;
        let vb_tex = r.u32()?;
        let use_full_precision_uvs = r.u32()?;
        let use_packed_position = r.u32()?;
        let mesh_extension = floats(r)?;
        let mesh_origin = floats(r)?;
        let (vertex_size, vertices) = bulk(r)?;
        let vertex_buffer = VertexBuffer { num_tex_coords: vb_tex, use_full_precision_uvs, use_packed_position, mesh_extension, mesh_origin, vertex_size, vertices };
        let colors = if has_vertex_colors { Some(bulk(r)?) } else { None };
        // Vertex influences (0xa5c140, element 0xa5bd50): not decoded yet.
        let vertex_influences = r.count()?;
        if vertex_influences != 0 { return Err(bad(format!("{vertex_influences} vertex influence sets"))); }
        Ok(LodModel { sections, index_cpu_access, index_size, indices, active_bone_indices, chunks, size, num_vertices, required_bones, raw_point_indices, num_tex_coords, vertex_buffer, colors, vertex_influences })
    }
}

/// `UMorphTarget` native data (`Serialize` 0x8b88b0 → 0x8b7d80): per LOD the vertices (0x8adfb0 / 0x89d770, 20 bytes
/// at version ≥ 0x326: position delta 3×f32, packed tangent Z delta u32, source vertex u32) and NumBaseMeshVerts.
pub fn read_morph_target(r: &mut Reader, end: usize) -> Result<Vec<(Range<usize>, u32)>> {
    let lods = array(r, |r| Ok((raw_array(r, 20)?, r.u32()?)))?;
    if r.pos != end { return Err(bad(format!("{} bytes after MorphTarget native data", end as i64 - r.pos as i64))); }
    Ok(lods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(half(0x3c00), 1.0);
        assert_eq!(half(0xc000), -2.0);
        assert_eq!(half(0x3800), 0.5);
        assert_eq!(half(0x0001), f32::powi(2.0, -24));
        assert_eq!(half(0x7c00), f32::INFINITY);
    }
}
