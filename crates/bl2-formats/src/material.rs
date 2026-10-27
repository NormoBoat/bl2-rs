//! `FMaterial` resource data serialized in the native data of materials and terrain (T-038, T-037).
//! Layout checked on the installation: plan/03-evidence.md E-068.

use std::io::{Error, ErrorKind, Result};

use crate::package::Reader;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

/// `FTextureLookup`: texture coordinate index, texture index, U and V scale.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureLookup {
    pub tex_coord_index: i32,
    pub texture_index: i32,
    pub u_scale: f32,
    pub v_scale: f32,
}

#[derive(Debug, Clone)]
pub struct MaterialResource {
    pub compile_errors: Vec<String>,
    /// Material expression -> texture dependency length.
    pub texture_dependency_lengths: Vec<(i32, i32)>,
    pub max_texture_dependency_length: i32,
    /// Key of the shader map in the shader cache (E-030).
    pub id: [u8; 16],
    pub num_user_tex_coords: u32,
    pub uniform_expression_textures: Vec<i32>,
    pub uses_scene_color: u32,
    pub uses_scene_depth: u32,
    pub uses_dynamic_parameter: u32,
    pub uses_light_map_uvs: u32,
    pub uses_vertex_position_offset: u32,
    pub using_transforms: u32,
    pub texture_lookups: Vec<TextureLookup>,
    /// Last u32; meaning not established (E-068).
    pub unknown: u32,
}

fn array<T>(r: &mut Reader, min_size: usize, mut item: impl FnMut(&mut Reader) -> Result<T>) -> Result<Vec<T>> {
    let n = r.count()?;
    if n.checked_mul(min_size).is_none_or(|b| b > r.data.len().saturating_sub(r.pos)) { return Err(bad(format!("array count {n} exceeds data at {}", r.pos - 4))); }
    (0..n).map(|_| item(r)).collect()
}

impl MaterialResource {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            compile_errors: array(r, 4, |r| r.fstring())?,
            texture_dependency_lengths: array(r, 8, |r| Ok((r.i32()?, r.i32()?)))?,
            max_texture_dependency_length: r.i32()?,
            id: r.bytes(16)?.try_into().unwrap(),
            num_user_tex_coords: r.u32()?,
            uniform_expression_textures: array(r, 4, |r| r.i32())?,
            uses_scene_color: r.u32()?,
            uses_scene_depth: r.u32()?,
            uses_dynamic_parameter: r.u32()?,
            uses_light_map_uvs: r.u32()?,
            uses_vertex_position_offset: r.u32()?,
            using_transforms: r.u32()?,
            texture_lookups: array(r, 16, |r| Ok(TextureLookup {
                tex_coord_index: r.i32()?, texture_index: r.i32()?, u_scale: f32::from_bits(r.u32()?), v_scale: f32::from_bits(r.u32()?),
            }))?,
            unknown: r.u32()?,
        })
    }
}

/// `FStaticParameterSet` of a static-permutation material instance (also the shader map key, E-030).
#[derive(Debug, Clone)]
pub struct StaticParameterSet {
    pub base_material_id: [u8; 16],
    /// Parameter name, value, override, expression GUID.
    pub switches: Vec<(crate::package::NameRef, u32, u32, [u8; 16])>,
    /// Parameter name, R G B A, override, expression GUID.
    pub component_masks: Vec<(crate::package::NameRef, [u32; 4], u32, [u8; 16])>,
    /// Parameter name, compression settings, override, expression GUID.
    pub normals: Vec<(crate::package::NameRef, u8, u32, [u8; 16])>,
    /// Parameter name, weightmap index, override, expression GUID.
    pub terrain_layer_weights: Vec<(crate::package::NameRef, i32, u32, [u8; 16])>,
}

fn guid(r: &mut Reader) -> Result<[u8; 16]> { Ok(r.bytes(16)?.try_into().unwrap()) }

impl StaticParameterSet {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            base_material_id: guid(r)?,
            switches: array(r, 32, |r| Ok((r.name()?, r.u32()?, r.u32()?, guid(r)?)))?,
            component_masks: array(r, 44, |r| Ok((r.name()?, [r.u32()?, r.u32()?, r.u32()?, r.u32()?], r.u32()?, guid(r)?)))?,
            normals: array(r, 29, |r| Ok((r.name()?, r.bytes(1)?[0], r.u32()?, guid(r)?)))?,
            terrain_layer_weights: array(r, 32, |r| Ok((r.name()?, r.i32()?, r.u32()?, guid(r)?)))?,
        })
    }
}
