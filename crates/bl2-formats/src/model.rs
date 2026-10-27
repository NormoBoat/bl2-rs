//! `Model` (BSP) native data after its tagged properties (T-037). Layout checked on every model of the installation:
//! plan/03-evidence.md E-067. Bulk arrays keep their byte ranges; their element sizes are fixed and checked.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::Reader;
use crate::texture::BulkData;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

/// `FBspSurf`.
#[derive(Debug, Clone, PartialEq)]
pub struct Surf {
    pub material: i32,
    pub poly_flags: u32,
    pub base: i32,
    pub normal: i32,
    pub texture_u: i32,
    pub texture_v: i32,
    pub brush_poly: i32,
    pub actor: i32,
    pub plane: [f32; 4],
    pub shadow_map_scale: f32,
    pub lighting_channels: u32,
    pub lightmass_index: i32,
}

#[derive(Debug)]
pub struct Model {
    /// FBoxSphereBounds: origin, box extent, sphere radius.
    pub bounds: [f32; 7],
    pub vectors: Range<usize>,
    pub points: Range<usize>,
    pub nodes: Range<usize>,
    pub surfs: Vec<Surf>,
    pub verts: Range<usize>,
    pub num_shared_sides: i32,
    /// Raw `FZoneProperties` (zone actor, connectivity, visibility, last render time).
    pub zones: Vec<Range<usize>>,
    pub polys: i32,
    pub leaf_hulls: Range<usize>,
    pub leaves: Range<usize>,
    pub root_outside: i32,
    pub linked: i32,
    pub portal_nodes: Range<usize>,
    pub num_vertices: i32,
    pub vertex_buffer: Range<usize>,
    pub lighting_guid: [u8; 16],
    pub lightmass_settings: Vec<Range<usize>>,
}

/// UE3 `BulkSerialize`d TArray whose element size must be `size` (empty arrays may store any size).
fn bulk(r: &mut Reader, size: usize) -> Result<Range<usize>> {
    let (stored, count) = (r.count()?, r.count()?);
    if stored != size && count != 0 { return Err(bad(format!("bulk element size {stored}, expected {size}"))); }
    let start = r.pos;
    r.bytes(stored.checked_mul(count).ok_or_else(|| bad("bulk size overflow"))?)?;
    Ok(start..r.pos)
}

fn floats<const N: usize>(r: &mut Reader) -> Result<[f32; N]> {
    let mut v = [0.0; N];
    for x in &mut v { *x = f32::from_bits(r.u32()?); }
    Ok(v)
}

fn raw(r: &mut Reader, n: usize) -> Result<Range<usize>> { let s = r.pos; r.bytes(n)?; Ok(s..r.pos) }

impl Model {
    /// `r` starts after the model's tagged properties; the model must end exactly at `end`.
    /// `self_index` is the model's own export index (the `TTransArray` owner of its surfaces).
    pub fn read(r: &mut Reader, end: usize, self_index: i32) -> Result<Self> {
        let bounds = floats(r)?;
        let vectors = bulk(r, 12)?;
        let points = bulk(r, 12)?;
        let nodes = bulk(r, 64)?;
        if r.i32()? != self_index { return Err(bad("Surfs owner is not the model")); }
        let n = r.count()?;
        if n > (end - r.pos) / 60 { return Err(bad("surface count exceeds data")); }
        let surfs = (0..n).map(|_| Ok(Surf {
            material: r.i32()?, poly_flags: r.u32()?, base: r.i32()?, normal: r.i32()?, texture_u: r.i32()?, texture_v: r.i32()?,
            brush_poly: r.i32()?, actor: r.i32()?, plane: floats(r)?, shadow_map_scale: f32::from_bits(r.u32()?),
            lighting_channels: r.u32()?, lightmass_index: r.i32()?,
        })).collect::<Result<_>>()?;
        let verts = bulk(r, 24)?;
        let num_shared_sides = r.i32()?;
        let zones = (0..r.count()?).map(|_| raw(r, 24)).collect::<Result<_>>()?;
        let polys = r.i32()?;
        let leaf_hulls = bulk(r, 4)?;
        let leaves = bulk(r, 4)?;
        let (root_outside, linked) = (r.i32()?, r.i32()?);
        let portal_nodes = bulk(r, 4)?;
        let num_vertices = r.i32()?;
        let vertex_buffer = bulk(r, 36)?;
        let lighting_guid = r.bytes(16)?.try_into().unwrap();
        let n = r.count()?;
        if n > (end - r.pos) / 40 { return Err(bad("lightmass settings count exceeds data")); }
        let lightmass_settings = (0..n).map(|_| raw(r, 40)).collect::<Result<_>>()?;
        if r.pos != end { return Err(bad(format!("model has {} trailing bytes", end as i64 - r.pos as i64))); }
        Ok(Self {
            bounds, vectors, points, nodes, surfs, verts, num_shared_sides, zones, polys, leaf_hulls, leaves, root_outside,
            linked, portal_nodes, num_vertices, vertex_buffer, lighting_guid, lightmass_settings,
        })
    }
}

/// `FPoly` of a `Polys` object (editor polygons kept in cooked brushes).
#[derive(Debug, Clone)]
pub struct Poly {
    pub base: [f32; 3],
    pub normal: [f32; 3],
    pub texture_u: [f32; 3],
    pub texture_v: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub poly_flags: u32,
    pub actor: i32,
    pub item_name: crate::package::NameRef,
    pub material: i32,
    pub link: i32,
    pub brush_poly: i32,
    pub shadow_map_scale: f32,
    pub lighting_channels: u32,
    pub lightmass_settings: Range<usize>,
    pub ruleset_variation: crate::package::NameRef,
}

/// `Polys` native data: element count, capacity, owner (the polys object), then the polygons.
pub fn read_polys(r: &mut Reader, end: usize, self_index: i32) -> Result<Vec<Poly>> {
    let (num, _max) = (r.count()?, r.count()?);
    if r.i32()? != self_index { return Err(bad("Polys owner is not the polys object")); }
    if num > (end - r.pos) / 136 { return Err(bad("polygon count exceeds data")); }
    let polys = (0..num).map(|_| {
        let (base, normal, texture_u, texture_v) = (floats(r)?, floats(r)?, floats(r)?, floats(r)?);
        let n = r.count()?;
        if n > (end - r.pos) / 12 { return Err(bad("vertex count exceeds data")); }
        Ok(Poly {
            base, normal, texture_u, texture_v, vertices: (0..n).map(|_| floats(r)).collect::<Result<_>>()?,
            poly_flags: r.u32()?, actor: r.i32()?, item_name: r.name()?, material: r.i32()?, link: r.i32()?, brush_poly: r.i32()?,
            shadow_map_scale: f32::from_bits(r.u32()?), lighting_channels: r.u32()?, lightmass_settings: raw(r, 40)?, ruleset_variation: r.name()?,
        })
    }).collect::<Result<_>>()?;
    if r.pos != end { return Err(bad(format!("polys have {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok(polys)
}

/// `FLightMapRef` (E-067): 0 none, 1 `FLightMap1D` (vertex samples), 2 `FLightMap2D` (textures).
#[derive(Debug, Clone)]
pub enum LightMap {
    None,
    /// Owner, directional samples, three scale vectors, simple samples: `FUntypedBulkData`, inline in cooked levels.
    Vertex { light_guids: Vec<[u8; 16]>, owner: i32, directional: BulkData, scales: [[f32; 3]; 3], simple: BulkData },
    /// Three (texture, scale) coefficients, coordinate scale and bias.
    Texture { light_guids: Vec<[u8; 16]>, textures: [(i32, [f32; 3]); 3], coordinate_scale: [f32; 2], coordinate_bias: [f32; 2] },
}

fn guids(r: &mut Reader, end: usize) -> Result<Vec<[u8; 16]>> {
    let n = r.count()?;
    if n > (end - r.pos) / 16 { return Err(bad("GUID count exceeds data")); }
    (0..n).map(|_| Ok(r.bytes(16)?.try_into().unwrap())).collect()
}

pub fn read_light_map(r: &mut Reader, end: usize) -> Result<LightMap> {
    Ok(match r.u32()? {
        0 => LightMap::None,
        1 => {
            let light_guids = guids(r, end)?;
            let owner = r.i32()?;
            let directional = BulkData::read(r)?;
            let scales = [floats(r)?, floats(r)?, floats(r)?];
            LightMap::Vertex { light_guids, owner, directional, scales, simple: BulkData::read(r)? }
        }
        2 => {
            let light_guids = guids(r, end)?;
            let mut textures = [(0, [0.0; 3]); 3];
            for t in &mut textures { *t = (r.i32()?, floats(r)?); }
            LightMap::Texture { light_guids, textures, coordinate_scale: floats(r)?, coordinate_bias: floats(r)? }
        }
        t => return Err(bad(format!("unknown light map type {t}"))),
    })
}

/// `FModelElement` of a `ModelComponent`.
#[derive(Debug, Clone)]
pub struct ModelElement {
    pub light_map: LightMap,
    pub component: i32,
    pub material: i32,
    pub nodes: Vec<u16>,
    pub shadow_maps: Vec<i32>,
    pub irrelevant_lights: Vec<[u8; 16]>,
}

#[derive(Debug)]
pub struct ModelComponent {
    pub model: i32,
    pub zone_index: i32,
    pub elements: Vec<ModelElement>,
    pub component_index: u16,
    pub nodes: Vec<u16>,
}

fn words(r: &mut Reader, end: usize) -> Result<Vec<u16>> {
    let n = r.count()?;
    if n > (end - r.pos) / 2 { return Err(bad("u16 count exceeds data")); }
    (0..n).map(|_| r.u16()).collect()
}

pub fn read_model_component(r: &mut Reader, end: usize) -> Result<ModelComponent> {
    let (model, zone_index) = (r.i32()?, r.i32()?);
    let n = r.count()?;
    if n > end - r.pos { return Err(bad("element count exceeds data")); }
    let elements = (0..n).map(|_| Ok(ModelElement {
        light_map: read_light_map(r, end)?, component: r.i32()?, material: r.i32()?, nodes: words(r, end)?,
        shadow_maps: { let n = r.count()?; if n > (end - r.pos) / 4 { return Err(bad("shadow map count exceeds data")); } (0..n).map(|_| r.i32()).collect::<Result<_>>()? },
        irrelevant_lights: guids(r, end)?,
    })).collect::<Result<_>>()?;
    let component_index = r.u16()?;
    let nodes = words(r, end)?;
    if r.pos != end { return Err(bad(format!("model component has {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok(ModelComponent { model, zone_index, elements, component_index, nodes })
}

/// `BrushComponent` native data: cached PhysX convex elements (bulk bytes each); empty for templates (E-067).
pub fn read_brush_component(r: &mut Reader, end: usize) -> Result<Vec<Range<usize>>> {
    if r.pos == end { return Ok(Vec::new()); }
    let n = r.count()?;
    if n > end - r.pos { return Err(bad("convex element count exceeds data")); }
    let elements = (0..n).map(|_| bulk(r, 1)).collect::<Result<_>>()?;
    if r.pos != end { return Err(bad(format!("brush component has {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok(elements)
}

/// `FStaticMeshComponentLODInfo`: static shadow maps (`ShadowMap2D`), shadow vertex buffers (`ShadowMap1D`),
/// light map, then override vertex colors (`FColorVertexBuffer`: stride, vertex count, bulk) when the flag is set (E-067).
#[derive(Debug, Clone)]
pub struct StaticMeshLod {
    pub shadow_maps: Vec<i32>,
    pub shadow_vertex_buffers: Vec<i32>,
    pub light_map: LightMap,
    pub vertex_colors: Option<(u32, u32, Range<usize>)>,
}

fn indices(r: &mut Reader, end: usize) -> Result<Vec<i32>> {
    let n = r.count()?;
    if n > (end - r.pos) / 4 { return Err(bad("object count exceeds data")); }
    (0..n).map(|_| r.i32()).collect()
}

/// Native data of `StaticMeshComponent` and its subclasses: one `StaticMeshLod` per LOD; empty for class defaults.
pub fn read_static_mesh_component(r: &mut Reader, end: usize) -> Result<Vec<StaticMeshLod>> {
    if r.pos == end { return Ok(Vec::new()); }
    let n = r.count()?;
    if n > end - r.pos { return Err(bad("LOD count exceeds data")); }
    let lods = (0..n).map(|_| Ok(StaticMeshLod {
        shadow_maps: indices(r, end)?,
        shadow_vertex_buffers: indices(r, end)?,
        light_map: read_light_map(r, end)?,
        vertex_colors: match r.bytes(1)?[0] {
            0 => None,
            1 => {
                let (stride, count) = (r.u32()?, r.u32()?);
                Some((stride, count, if count > 0 { bulk(r, 4)? } else { r.pos..r.pos }))
            }
            b => return Err(bad(format!("vertex color flag {b}"))),
        },
    })).collect::<Result<_>>()?;
    if r.pos != end { return Err(bad(format!("static mesh component has {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok(lods)
}

/// `FStaticReceiverData` of a `DecalComponent`: decal geometry baked onto one receiving primitive (E-067).
#[derive(Debug, Clone)]
pub struct DecalReceiver {
    pub component: i32,
    /// `FDecalVertex`, 28 bytes: position, packed TangentX/TangentZ, projected UV.
    pub vertices: Range<usize>,
    pub indices: Range<usize>,
    pub num_triangles: u32,
    pub light_map: LightMap,
    pub shadow_maps: Vec<i32>,
    /// Two values after the shadow maps (meaning not established; pairs like (0, 0), (1, -1), (2, -1)).
    pub unknown: [i32; 2],
}

/// `DecalComponent` native data: static receivers; empty for class defaults.
pub fn read_decal_component(r: &mut Reader, end: usize) -> Result<Vec<DecalReceiver>> {
    if r.pos == end { return Ok(Vec::new()); }
    let n = r.count()?;
    if n > end - r.pos { return Err(bad("receiver count exceeds data")); }
    let receivers = (0..n).map(|_| Ok(DecalReceiver {
        component: r.i32()?, vertices: bulk(r, 28)?, indices: bulk(r, 2)?, num_triangles: r.u32()?,
        light_map: read_light_map(r, end)?, shadow_maps: indices(r, end)?, unknown: [r.i32()?, r.i32()?],
    })).collect::<Result<_>>()?;
    if r.pos != end { return Err(bad(format!("decal component has {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok(receivers)
}

/// `ShadowMap1D` native data: per-vertex samples (f32; about 19 % are above 1, so not plain [0, 1] factors), then the
/// GUID of the light (E-067).
pub fn read_shadow_map_1d(r: &mut Reader, end: usize) -> Result<(Vec<f32>, [u8; 16])> {
    let n = r.count()?;
    if n > (end - r.pos) / 4 { return Err(bad("shadow sample count exceeds data")); }
    let samples = (0..n).map(|_| Ok(f32::from_bits(r.u32()?))).collect::<Result<_>>()?;
    let guid = r.bytes(16)?.try_into().unwrap();
    if r.pos != end { return Err(bad(format!("shadow map has {} trailing bytes", end as i64 - r.pos as i64))); }
    Ok((samples, guid))
}
