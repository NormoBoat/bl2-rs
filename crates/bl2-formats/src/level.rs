//! `Level` native data after its tagged properties (T-037). Layout checked on every level of the installation:
//! plan/03-evidence.md E-067.

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::Reader;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    pub protocol: String,
    pub host: String,
    pub map: String,
    pub portal: String,
    pub options: Vec<String>,
    pub port: i32,
    pub valid: i32,
}

/// Bounding sphere + texel factor, texture, attached flag, original radius.
pub type DynamicTextureInstance = ([f32; 5], i32, i32, f32);

#[derive(Debug)]
pub struct Level {
    /// `TTransArray` owner: the level itself.
    pub owner: i32,
    /// Package indices of the level's actors (0 = empty slot).
    pub actors: Vec<i32>,
    pub url: Url,
    pub model: i32,
    pub model_components: Vec<i32>,
    pub game_sequences: Vec<i32>,
    /// Texture -> streaming instances (bounding sphere + texel factor).
    pub texture_instances: Vec<(i32, Vec<[f32; 5]>)>,
    /// Primitive component -> dynamic texture instances (sphere, texel factor, texture, attached, original radius).
    pub dynamic_texture_instances: Vec<(i32, Vec<DynamicTextureInstance>)>,
    /// Cooked PhysX data (BSP, static meshes by scale), kept as byte ranges.
    pub cached_phys_bsp: Range<usize>,
    pub cached_phys_sm_map: Vec<(i32, [f32; 3], i32)>,
    pub cached_phys_sm_store: Vec<Vec<Range<usize>>>,
    pub cached_phys_per_tri_sm_map: Vec<(i32, [f32; 3], i32)>,
    pub cached_phys_per_tri_sm_store: Vec<Range<usize>>,
    pub cached_phys_bsp_version: i32,
    pub cached_phys_sm_version: i32,
    pub force_stream_textures: Vec<(i32, i32)>,
    pub cached_phys_convex_bsp: Vec<Range<usize>>,
    pub cached_phys_convex_bsp_version: i32,
    /// NavListStart/End, CoverListStart/End, PylonListStart/End.
    pub nav_lists: [i32; 6],
    /// Gearbox additions after the navigation lists (meanings not established, E-067):
    /// GUID -> index map, actor list with a parallel (index, flag) list, an i32, bounding boxes, 37-byte records.
    pub guid_indices: Vec<([u8; 16], i32)>,
    pub gbx_actors: Vec<i32>,
    pub gbx_actor_flags: Vec<(i32, u8)>,
    pub gbx_value: i32,
    pub bounds: Vec<Box3>,
    pub gbx_value2: i32,
    pub gbx_records: Vec<Range<usize>>,
    /// Trailer of 73 bytes plus 8 per bounding box; zero in all levels but one (E-067).
    pub trailer: Range<usize>,
}

/// UE3 `FBox`: Min, Max, IsValid (one byte).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub valid: u8,
}

fn array<T>(r: &mut Reader, mut item: impl FnMut(&mut Reader) -> Result<T>) -> Result<Vec<T>> {
    let n = r.count()?;
    if n > r.data.len().saturating_sub(r.pos) { return Err(bad(format!("array count {n} exceeds data at {}", r.pos - 4))); }
    (0..n).map(|_| item(r)).collect()
}

/// UE3 `BulkSerialize`d TArray: element size, count, raw elements.
fn bulk(r: &mut Reader) -> Result<Range<usize>> {
    let (size, count) = (r.count()?, r.count()?);
    let start = r.pos;
    r.bytes(size.checked_mul(count).ok_or_else(|| bad("bulk size overflow"))?)?;
    Ok(start..r.pos)
}

fn floats<const N: usize>(r: &mut Reader) -> Result<[f32; N]> {
    let mut v = [0.0; N];
    for x in &mut v { *x = f32::from_bits(r.u32()?); }
    Ok(v)
}

impl Level {
    /// `r` starts after the level's tagged properties; `end` is the end of the export.
    pub fn read(r: &mut Reader, end: usize) -> Result<Self> {
        let owner = r.i32()?;
        let actors = array(r, |r| r.i32())?;
        let url = Url {
            protocol: r.fstring()?, host: r.fstring()?, map: r.fstring()?, portal: r.fstring()?,
            options: array(r, |r| r.fstring())?, port: r.i32()?, valid: r.i32()?,
        };
        let model = r.i32()?;
        let model_components = array(r, |r| r.i32())?;
        let game_sequences = array(r, |r| r.i32())?;
        let texture_instances = array(r, |r| Ok((r.i32()?, array(r, floats::<5>)?)))?;
        let dynamic_texture_instances = array(r, |r| Ok((r.i32()?, array(r, |r| Ok((floats::<5>(r)?, r.i32()?, r.i32()?, f32::from_bits(r.u32()?))))?)))?;
        // Meaning unknown; 0 in the surveyed levels (E-067).
        if r.i32()? != 0 { return Err(bad("unexpected value before CachedPhysBSPData")); }
        let cached_phys_bsp = bulk(r)?;
        let scaled = |r: &mut Reader| Ok((r.i32()?, floats::<3>(r)?, r.i32()?));
        let cached_phys_sm_map = array(r, scaled)?;
        let cached_phys_sm_store = array(r, |r| array(r, bulk))?;
        let cached_phys_per_tri_sm_map = array(r, scaled)?;
        let cached_phys_per_tri_sm_store = array(r, bulk)?;
        let (cached_phys_bsp_version, cached_phys_sm_version) = (r.i32()?, r.i32()?);
        let force_stream_textures = array(r, |r| Ok((r.i32()?, r.i32()?)))?;
        let cached_phys_convex_bsp = array(r, bulk)?;
        let cached_phys_convex_bsp_version = r.i32()?;
        let mut nav_lists = [0; 6];
        for v in &mut nav_lists { *v = r.i32()?; }
        let guid_indices = array(r, |r| Ok((r.bytes(16)?.try_into().unwrap(), r.i32()?)))?;
        let gbx_actors = array(r, |r| r.i32())?;
        let gbx_actor_flags = array(r, |r| Ok((r.i32()?, r.bytes(1)?[0])))?;
        let gbx_value = r.i32()?;
        let bounds = array(r, |r| Ok(Box3 { min: floats(r)?, max: floats(r)?, valid: r.bytes(1)?[0] }))?;
        let gbx_value2 = r.i32()?;
        let gbx_records = array(r, |r| { let s = r.pos; r.bytes(37)?; Ok(s..r.pos) })?;
        if end.checked_sub(r.pos) != Some(73 + 8 * bounds.len()) {
            return Err(bad(format!("level trailer is {} bytes, expected {}", end as i64 - r.pos as i64, 73 + 8 * bounds.len())));
        }
        Ok(Self {
            owner, actors, url, model, model_components, game_sequences, texture_instances, dynamic_texture_instances,
            cached_phys_bsp, cached_phys_sm_map, cached_phys_sm_store, cached_phys_per_tri_sm_map, cached_phys_per_tri_sm_store,
            cached_phys_bsp_version, cached_phys_sm_version, force_stream_textures, cached_phys_convex_bsp,
            cached_phys_convex_bsp_version, nav_lists, guid_indices, gbx_actors, gbx_actor_flags, gbx_value, bounds,
            gbx_value2, gbx_records, trailer: r.pos..end,
        })
    }
}

/// `World` native data: persistent level, FaceFX anim set, four editor viewports, then two values (E-067).
#[derive(Debug)]
pub struct World {
    pub persistent_level: i32,
    pub persistent_face_fx_anim_set: i32,
    /// Camera position, rotation (pitch, yaw, roll), ortho zoom.
    pub editor_views: [([f32; 3], [i32; 3], f32); 4],
    /// Deprecated save-game summary reference.
    pub save_game_summary: i32,
    pub extra_referenced_objects: Vec<i32>,
}

impl World {
    pub fn read(r: &mut Reader, end: usize) -> Result<Self> {
        let (persistent_level, persistent_face_fx_anim_set) = (r.i32()?, r.i32()?);
        let mut editor_views = [([0.0; 3], [0; 3], 0.0); 4];
        for v in &mut editor_views { *v = (floats(r)?, [r.i32()?, r.i32()?, r.i32()?], f32::from_bits(r.u32()?)); }
        let save_game_summary = r.i32()?;
        let extra_referenced_objects = array(r, |r| r.i32())?;
        if r.pos != end { return Err(bad(format!("world has {} trailing bytes", end as i64 - r.pos as i64))); }
        Ok(Self { persistent_level, persistent_face_fx_anim_set, editor_views, save_game_summary, extra_referenced_objects })
    }
}

/// Serialized `Location` of an actor export, looked up on the export and then its archetypes inside the package.
/// `None`: no serialized value in that chain (the class default applies; `Actor.Location` defaults to zero).
pub fn actor_location(p: &crate::package::Package, tree: &crate::properties::ClassTree, export: usize) -> Result<Option<[f32; 3]>> {
    let mut index = export;
    for _ in 0..64 {
        let e = &p.exports[index];
        let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
        p.read_object_header(e, tree, &mut r)?;
        if let Some(tag) = p.read_tags(&mut r)?.into_iter().find(|t| p.name(t.name) == "Location") {
            if tag.value.len() != 12 { return Err(bad("Location is not an immutable Vector")); }
            return floats(&mut Reader::new(&p.data[..tag.value.end], tag.value.start)).map(Some);
        }
        if e.archetype <= 0 { return Ok(None); }
        index = e.archetype as usize - 1;
    }
    Err(bad("archetype chain longer than 64"))
}

/// Native data of a `StaticMeshCollectionActor`/`BlockingMeshCollectionActor` (`extra` = true: matrix + 5 floats,
/// by the samples Scale3D, Scale and one unidentified value) or `StaticLightCollectionActor` (matrix only):
/// one entry per element of the actor's component array, in array order (E-067).
pub fn read_collection(r: &mut Reader, end: usize, components: usize, extra: bool) -> Result<Vec<([f32; 16], [f32; 5])>> {
    let size = if extra { 84 } else { 64 };
    if end.checked_sub(r.pos) != Some(components * size) { return Err(bad("collection data does not match its component count")); }
    (0..components).map(|_| Ok((floats(r)?, if extra { floats(r)? } else { [0.0; 5] }))).collect()
}

/// `FTerrainMaterialResource` cached in a `Terrain`: the material resource plus the terrain-specific part (E-067).
#[derive(Debug)]
pub struct TerrainMaterial {
    pub resource: crate::material::MaterialResource,
    pub terrain: i32,
    /// `FTerrainMaterialMask`: number of bits, bit mask of the layers using this material.
    pub mask: (i32, u64),
    pub material_ids: Vec<[u8; 16]>,
    pub lighting_guid: [u8; 16],
    /// u32 (0 or 1; by position `bEnableSpecular`, HYPOTHESIS) and 24-byte records (meaning not established).
    pub unknown: u32,
    pub unknown_records: Vec<Range<usize>>,
}

/// `Terrain` native data (E-067).
#[derive(Debug)]
pub struct Terrain {
    /// u16 heights, one per vertex ((NumPatchesX + 1) * (NumPatchesY + 1)).
    pub heights: Range<usize>,
    /// One byte per vertex.
    pub info_data: Range<usize>,
    /// Alpha maps of the layers, one byte per weightmap sample.
    pub alpha_maps: Vec<Range<usize>>,
    /// Per terrain material weight data (`FTerrainWeightedMaterial`, by layout): bytes, size X, size Y, terrain, `TerrainMaterial`.
    pub weight_maps: Vec<(Range<usize>, u32, u32, i32, i32)>,
    pub weighted_texture_maps: Vec<i32>,
    pub materials: Vec<TerrainMaterial>,
}

fn byte_array(r: &mut Reader, element: usize) -> Result<Range<usize>> {
    let n = r.count()?;
    let start = r.pos;
    r.bytes(n.checked_mul(element).ok_or_else(|| bad("array size overflow"))?)?;
    Ok(start..r.pos)
}

fn guid(r: &mut Reader) -> Result<[u8; 16]> { Ok(r.bytes(16)?.try_into().unwrap()) }

impl Terrain {
    /// Empty for class defaults.
    pub fn read(r: &mut Reader, end: usize) -> Result<Option<Self>> {
        if r.pos == end { return Ok(None); }
        let heights = byte_array(r, 2)?;
        let info_data = byte_array(r, 1)?;
        let alpha_maps = array(r, |r| byte_array(r, 1))?;
        let weight_maps = array(r, |r| Ok((byte_array(r, 1)?, r.u32()?, r.u32()?, r.i32()?, r.i32()?)))?;
        let weighted_texture_maps = array(r, |r| r.i32())?;
        let materials = array(r, |r| Ok(TerrainMaterial {
            resource: crate::material::MaterialResource::read(r)?,
            terrain: r.i32()?,
            mask: (r.i32()?, r.u64()?),
            material_ids: array(r, guid)?,
            lighting_guid: guid(r)?,
            unknown: r.u32()?,
            unknown_records: array(r, |r| { let s = r.pos; r.bytes(24)?; Ok(s..r.pos) })?,
        }))?;
        if r.pos != end { return Err(bad(format!("terrain has {} trailing bytes", end as i64 - r.pos as i64))); }
        Ok(Some(Self { heights, info_data, alpha_maps, weight_maps, weighted_texture_maps, materials }))
    }
}

/// `GBXNavMeshSpecialMove` (48 bytes): ActorSpecialMoveId, CrossLevelMoverIdx, DestinationMeshIdx, DestinationPolyIdx,
/// Mover (interface object), LocalSpaceSrcLocation, LocalSpaceDestLocation.
#[derive(Debug, Clone)]
pub struct NavSpecialMove {
    pub ids: [i32; 4],
    pub mover: i32,
    pub locations: [[f32; 3]; 2],
}

/// `GBXNavMeshPolyData`: CrossLevelConnections (ConnectedNavmeshIdx, EdgeIdx, PolyIdx), SpecialMoves, Obstacles (6 x u16).
#[derive(Debug, Clone)]
pub struct NavPolyData {
    pub cross_level_connections: Vec<[u16; 3]>,
    pub special_moves: Vec<NavSpecialMove>,
    pub obstacles: [u16; 6],
}

/// `GBXNavMesh` (Gearbox) native data, `AGBXNavMesh::Serialize` 0xd42410 (E-067); present when `BuildVersion` > 3.
/// Names are the script struct members of `Engine.GBXNavMesh`; the native serializer writes them in this order.
#[derive(Debug)]
pub struct NavMesh {
    pub vertices: Vec<[f32; 3]>,
    /// `GBXNavMeshPoly`: Verts[3], ConnectedPolys[3] (0xFFFF = none), PolyDataIdx.
    pub polys: Vec<([u16; 3], [u16; 3], u16)>,
    pub poly_data: Vec<NavPolyData>,
    /// `GBXNavMeshPathSize`: Radius, Height, DrawColor, ObstacleDrawColor, StartIndex (raw u32 each).
    pub path_sizes: Vec<[u32; 5]>,
    /// `GBXNavMeshPolyLookup`: eight u32 (Origin, CellSize, XCells, YCells by the script struct; split not checked),
    /// CellIndices, Cells (IntersectingPolys each).
    pub poly_lookup: ([u32; 8], Vec<i32>, Vec<Vec<u16>>),
    /// `GBXNavMeshObstacleData`: Obstacle (object, cross-level through a container), MinAffectedPathSize.
    pub obstacles: Vec<(i32, i32)>,
    pub special_movers: Vec<i32>,
    pub mesh_base: i32,
}

fn words<const N: usize>(r: &mut Reader) -> Result<[u16; N]> {
    let mut v = [0; N];
    for x in &mut v { *x = r.u16()?; }
    Ok(v)
}

impl NavMesh {
    pub fn read(r: &mut Reader, end: usize) -> Result<Self> {
        let vertices = array(r, floats::<3>)?;
        let polys = array(r, |r| Ok((words(r)?, words(r)?, r.u16()?)))?;
        let poly_data = array(r, |r| Ok(NavPolyData {
            cross_level_connections: array(r, words::<3>)?,
            special_moves: array(r, |r| Ok(NavSpecialMove { ids: [r.i32()?, r.i32()?, r.i32()?, r.i32()?], mover: r.i32()?, locations: [floats(r)?, floats(r)?] }))?,
            obstacles: words(r)?,
        }))?;
        let path_sizes = array(r, |r| Ok([r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?]))?;
        let mut head = [0; 8];
        for v in &mut head { *v = r.u32()?; }
        let poly_lookup = (head, array(r, |r| r.i32())?, array(r, |r| array(r, |r| r.u16()))?);
        let obstacles = array(r, |r| Ok((r.i32()?, r.i32()?)))?;
        let special_movers = array(r, |r| r.i32())?;
        let mesh_base = r.i32()?;
        if r.pos != end { return Err(bad(format!("nav mesh has {} trailing bytes", end as i64 - r.pos as i64))); }
        Ok(Self { vertices, polys, poly_data, path_sizes, poly_lookup, obstacles, special_movers, mesh_base })
    }
}

/// `FTerrainBVNode` as stored (37 bytes): box (min, max, valid byte), `bIsLeaf`, four u16 (children or patch range).
#[derive(Debug, Clone)]
pub struct TerrainBvNode {
    pub bounds: Box3,
    pub is_leaf: u32,
    pub indices: [u16; 4],
}

/// Cooked platform data of a `TerrainComponent` (`FUN_005cdc30`): owner, full vertex buffer (8-byte vertices:
/// index u16, height u16, two i16 by the observed values), a u32, then batches (`TerrainBatch`).
#[derive(Debug)]
pub struct TerrainPlatformData {
    pub owner: i32,
    pub vertices: Range<usize>,
    pub value: u32,
    pub batches: Vec<TerrainBatch>,
}

/// Batch of a terrain component (`FUN_005cd4a0`): u16 index bulk, u32, 44-byte tree nodes (four u16, three u32,
/// six f32 by the observed values; `FUN_005c57d0`, written for LicenseeVer >= 0x2a), u32, eight u32.
#[derive(Debug)]
pub struct TerrainBatch {
    pub indices: Range<usize>,
    pub value: u32,
    pub nodes: Vec<Range<usize>>,
    pub value2: u32,
    pub values: [u32; 8],
}

/// `TerrainComponent` native data, `UTerrainComponent::Serialize` 0x5cffc0 (E-067); `None` for class defaults.
#[derive(Debug)]
pub struct TerrainComponent {
    pub bv_tree: Vec<TerrainBvNode>,
    pub light_map: crate::model::LightMap,
    pub platform_data: Option<TerrainPlatformData>,
}

impl TerrainComponent {
    pub fn read(r: &mut Reader, end: usize) -> Result<Option<Self>> {
        if r.pos == end { return Ok(None); }
        let bv_tree = array(r, |r| Ok(TerrainBvNode {
            bounds: Box3 { min: floats(r)?, max: floats(r)?, valid: r.bytes(1)?[0] }, is_leaf: r.u32()?, indices: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
        }))?;
        let light_map = crate::model::read_light_map(r, end)?;
        let platform_data = match r.u32()? {
            0 => None,
            1 => {
                let owner = r.i32()?;
                let vertices = bulk(r)?;
                let value = r.u32()?;
                let batches = array(r, |r| {
                    let (indices, value) = (bulk(r)?, r.u32()?);
                    let nodes = array(r, |r| { let s = r.pos; r.bytes(44)?; Ok(s..r.pos) })?;
                    let value2 = r.u32()?;
                    let mut values = [0; 8];
                    for x in &mut values { *x = r.u32()?; }
                    Ok(TerrainBatch { indices, value, nodes, value2, values })
                })?;
                Some(TerrainPlatformData { owner, vertices, value, batches })
            }
            v => return Err(bad(format!("terrain platform data flag {v}"))),
        };
        if r.pos != end { return Err(bad(format!("terrain component has {} trailing bytes", end as i64 - r.pos as i64))); }
        Ok(Some(Self { bv_tree, light_map, platform_data }))
    }
}
