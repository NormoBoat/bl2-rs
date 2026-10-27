//! StaticMesh native data (plan/12-tasks.md T-035). Layout found on the installation: plan/03-evidence.md E-059.
//! Built up step by step with tests/static_mesh_scan.rs; fields after the last verified one are not read yet.

use std::io::Result;
use std::ops::Range;

use crate::package::Reader;
use crate::texture::BulkData;

#[derive(Clone, Copy, Debug, Default)]
pub struct BoxSphereBounds {
    pub origin: [f32; 3],
    pub box_extent: [f32; 3],
    pub sphere_radius: f32,
}

fn f32s<const N: usize>(r: &mut Reader) -> Result<[f32; N]> {
    let mut v = [0f32; N];
    for x in &mut v {
        *x = f32::from_bits(r.u32()?);
    }
    Ok(v)
}

/// UE3 "bulk serialized" array: element size, element count, raw elements.
fn bulk_array<'a>(r: &mut Reader<'a>) -> Result<(usize, usize, &'a [u8])> {
    let elem = r.count()?;
    let n = r.count()?;
    Ok((elem, n, r.bytes(elem * n)?))
}

/// A bulk array kept as a byte range of `Package::data`.
#[derive(Clone, Debug)]
pub struct BulkArray {
    pub element_size: usize,
    pub count: usize,
    pub data: Range<usize>,
}

impl BulkArray {
    fn read(r: &mut Reader) -> Result<Self> {
        let element_size = r.count()?;
        let count = r.count()?;
        let start = r.pos;
        r.bytes(element_size * count)?;
        Ok(Self { element_size, count, data: start..r.pos })
    }
}

#[derive(Clone, Debug)]
pub struct VertexBuffers {
    pub position_stride: u32,
    pub positions: BulkArray,
    pub num_tex_coords: u32,
    pub uv_stride: u32,
    pub full_precision_uvs: bool,
    /// Packed TangentX, TangentZ, then UVs (half floats unless full precision).
    pub tangents_uvs: BulkArray,
    pub color_stride: u32,
    pub colors: Option<BulkArray>,
    pub num_vertices: u32,
    pub indices: BulkArray,
}

#[derive(Clone, Copy, Debug)]
pub struct CollisionTriangle {
    pub indices: [u16; 3],
    pub material: u16,
}

#[derive(Clone, Debug)]
pub struct Fragment {
    pub base_index: u32,
    pub num_primitives: u32,
}

#[derive(Clone, Debug)]
pub struct Element {
    pub material: i32,
    pub enable_collision: u32,
    pub old_enable_collision: u32,
    pub enable_shadow_casting: u32,
    pub first_index: u32,
    pub num_triangles: u32,
    pub min_vertex_index: u32,
    pub max_vertex_index: u32,
    pub material_index: i32,
    pub fragments: Vec<Fragment>,
    /// One byte after the fragments; meaning not established (E-059).
    pub unknown_byte: u8,
}

#[derive(Clone, Debug)]
pub struct Lod {
    pub raw_triangles: BulkData,
    pub elements: Vec<Element>,
    pub buffers: VertexBuffers,
}

fn read_buffers(r: &mut Reader) -> Result<VertexBuffers> {
    let position_stride = r.u32()?;
    let _num_positions = r.u32()?;
    let positions = BulkArray::read(r)?;
    let num_tex_coords = r.u32()?;
    let uv_stride = r.u32()?;
    let _num_uv_vertices = r.u32()?;
    let full_precision_uvs = r.u32()? != 0;
    let tangents_uvs = BulkArray::read(r)?;
    let color_stride = r.u32()?;
    let num_colors = r.u32()?;
    let colors = if num_colors > 0 { Some(BulkArray::read(r)?) } else { None };
    let num_vertices = r.u32()?;
    let indices = BulkArray::read(r)?;
    Ok(VertexBuffers { position_stride, positions, num_tex_coords, uv_stride, full_precision_uvs, tangents_uvs, color_stride, colors, num_vertices, indices })
}

#[derive(Clone, Debug)]
pub struct StaticMeshData {
    pub bounds: BoxSphereBounds,
    pub body_setup: i32,
    pub kdop_bounds: [f32; 6],
    pub kdop_node_size: usize,
    pub kdop_node_count: usize,
    pub collision_triangles: Vec<CollisionTriangle>,
    pub internal_version: i32,
    /// 12 bytes before the LOD array; meaning not established (E-059).
    pub unknown_12: [u8; 12],
    pub lod_count: usize,
    pub lods: Vec<Lod>,
    /// i32 after the LOD array (1 in every mesh of the install); meaning not established (E-059).
    pub unknown_after_lods: i32,
    pub thumbnail_angle: [i32; 3],
    pub thumbnail_distance: f32,
    /// 9 bytes before LightingGuid; meaning not established (E-059).
    pub unknown_9: [u8; 9],
    pub lighting_guid: [u8; 16],
    pub vertex_position_version: i32,
    pub cached_streaming_texture_factors: Vec<f32>,
    /// Final i32; meaning not established (E-059).
    pub trailing: i32,
}

impl StaticMeshData {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let o = f32s::<3>(r)?;
        let e = f32s::<3>(r)?;
        let bounds = BoxSphereBounds { origin: o, box_extent: e, sphere_radius: f32::from_bits(r.u32()?) };
        let body_setup = r.i32()?;
        let kdop_bounds = f32s::<6>(r)?;
        let (kdop_node_size, kdop_node_count, _) = bulk_array(r)?;
        let (tri_size, _, tris) = bulk_array(r)?;
        if tri_size != 8 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("collision triangle size {tri_size}")));
        }
        let collision_triangles = tris
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| {
                let u = |i: usize| u16::from_le_bytes([c[i], c[i + 1]]);
                CollisionTriangle { indices: [u(0), u(2), u(4)], material: u(6) }
            })
            .collect();
        let internal_version = r.i32()?;
        let unknown_12 = r.bytes(12)?.try_into().unwrap();
        let lod_count = r.count()?;
        let lods = (0..lod_count)
            .map(|_| {
                let raw_triangles = BulkData::read(r)?;
                let elements = (0..r.count()?)
                    .map(|_| {
                        Ok(Element {
                            material: r.i32()?,
                            enable_collision: r.u32()?,
                            old_enable_collision: r.u32()?,
                            enable_shadow_casting: r.u32()?,
                            first_index: r.u32()?,
                            num_triangles: r.u32()?,
                            min_vertex_index: r.u32()?,
                            max_vertex_index: r.u32()?,
                            material_index: r.i32()?,
                            fragments: (0..r.count()?).map(|_| Ok(Fragment { base_index: r.u32()?, num_primitives: r.u32()? })).collect::<Result<_>>()?,
                            unknown_byte: r.bytes(1)?[0],
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(Lod { raw_triangles, elements, buffers: read_buffers(r)? })
            })
            .collect::<Result<_>>()?;
        let unknown_after_lods = r.i32()?;
        let thumbnail_angle = [r.i32()?, r.i32()?, r.i32()?];
        let thumbnail_distance = f32::from_bits(r.u32()?);
        let unknown_9 = r.bytes(9)?.try_into().unwrap();
        let lighting_guid = r.bytes(16)?.try_into().unwrap();
        let vertex_position_version = r.i32()?;
        let cached_streaming_texture_factors = (0..r.count()?).map(|_| Ok(f32::from_bits(r.u32()?))).collect::<Result<_>>()?;
        let trailing = r.i32()?;
        Ok(Self {
            bounds, body_setup, kdop_bounds, kdop_node_size, kdop_node_count, collision_triangles, internal_version, unknown_12,
            lod_count, lods, unknown_after_lods, thumbnail_angle, thumbnail_distance, unknown_9, lighting_guid, vertex_position_version,
            cached_streaming_texture_factors, trailing,
        })
    }
}
