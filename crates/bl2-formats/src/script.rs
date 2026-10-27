//! UnrealScript definition objects (UField subclasses) as stored in the cooked script packages
//! (plan/12-tasks.md T-015). Layout found on the installation: plan/03-evidence.md E-026.
//! Bytecode is kept as raw bytes here; it is read for research only and never executed (D-005).

use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::{Export, NameRef, Package, Reader};
use crate::properties::Tag;

const CPF_NET: u64 = 0x20;
const FUNC_NET: u32 = 0x40;

fn bad(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

#[derive(Clone, Debug)]
pub struct StructHeader {
    pub super_struct: i32,
    pub script_text: i32,
    pub children: i32,
    pub cpp_text: i32,
    pub line: i32,
    pub text_pos: i32,
    /// Bytecode size in memory (object references are wider in memory than on disk).
    pub bytecode_size: i32,
    pub script: Range<usize>,
}

#[derive(Clone, Debug)]
pub enum PropertyKind {
    Plain,
    Byte { enum_: i32 },
    Object { class: i32 },
    Class { class: i32, meta_class: i32 },
    Interface { class: i32 },
    Struct { struct_: i32 },
    Array { inner: i32 },
    Map { key: i32, value: i32 },
    Delegate { function: i32, source_delegate: i32 },
    /// Gearbox attribute properties (Byte/Int/FloatAttributeProperty): base kind plus two object references
    /// (meanings not established; E-026).
    Attribute { enum_: Option<i32>, refs: [i32; 2] },
}

#[derive(Clone, Debug)]
pub enum Definition {
    Property { array_dim: i32, flags: u64, category: NameRef, array_enum: i32, rep_offset: Option<u16>, kind: PropertyKind },
    Enum { names: Vec<NameRef> },
    Const { value: String },
    Struct(StructHeader),
    ScriptStruct { header: StructHeader, struct_flags: u32, defaults: Vec<Tag> },
    Function { header: StructHeader, native: u16, oper_precedence: u8, flags: u32, rep_offset: Option<u16>, friendly_name: NameRef },
    State { header: StructHeader, state: StateInfo },
    Class { header: StructHeader, state: StateInfo, class: ClassInfo },
}

#[derive(Clone, Debug)]
pub struct StateInfo {
    pub probe_mask: u32,
    pub label_table_offset: u16,
    pub state_flags: u16,
    pub func_map: Vec<(NameRef, i32)>,
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub class_flags: u32,
    pub within: i32,
    pub config_name: NameRef,
    pub component_defaults: Vec<(NameRef, i32)>,
    pub interfaces: Vec<(i32, i32)>,
    pub name_lists: Vec<Vec<NameRef>>,
    pub force_script_order: u32,
    pub class_group_names: Vec<NameRef>,
    pub header_filename: String,
    pub dll_bind_name: NameRef,
    pub unknown_byte: u8,
    pub default_object: i32,
}

fn names(r: &mut Reader) -> Result<Vec<NameRef>> {
    (0..r.count()?).map(|_| r.name()).collect()
}

impl StructHeader {
    fn read(r: &mut Reader) -> Result<Self> {
        let (super_struct, script_text, children, cpp_text, line, text_pos) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?);
        let bytecode_size = r.i32()?;
        let storage = r.count()?;
        let start = r.pos;
        r.bytes(storage)?;
        Ok(Self { super_struct, script_text, children, cpp_text, line, text_pos, bytecode_size, script: start..r.pos })
    }
}

impl StateInfo {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            probe_mask: r.u32()?,
            label_table_offset: r.u16()?,
            state_flags: r.u16()?,
            func_map: (0..r.count()?).map(|_| Ok((r.name()?, r.i32()?))).collect::<Result<_>>()?,
        })
    }
}

impl Package {
    /// Reads a script definition export; `class` is the export's class name.
    pub fn read_definition(&self, e: &Export, class: &str, r: &mut Reader) -> Result<Definition> {
        Ok(self.read_definition_and_next(e, class, r)?.0)
    }

    /// Also returns `UField::Next`, the package index of the next field in the owner's children list.
    pub fn read_definition_and_next(&self, e: &Export, class: &str, r: &mut Reader) -> Result<(Definition, i32)> {
        // Functions carry a u16-counted list of u16 values before the object header; meaning not established (E-026).
        if class == "Function" {
            let n = r.u16()? as usize;
            r.bytes(n * 2)?;
        }
        r.i32()?; // NetIndex
        if class != "Class" {
            let tags = self.read_tags(r)?;
            if !tags.is_empty() {
                return Err(bad(format!("{class} {} has tagged properties", self.name(e.object_name))));
            }
        }
        let next = r.i32()?;
        let definition = match class {
            "Enum" => Definition::Enum { names: names(r)? },
            "Const" => Definition::Const { value: r.fstring()? },
            "Struct" => Definition::Struct(StructHeader::read(r)?),
            "ScriptStruct" => {
                let header = StructHeader::read(r)?;
                let struct_flags = r.u32()?;
                Definition::ScriptStruct { header, struct_flags, defaults: self.read_tags(r)? }
            }
            "Function" => {
                let header = StructHeader::read(r)?;
                let native = r.u16()?;
                let oper_precedence = r.bytes(1)?[0];
                let flags = r.u32()?;
                let rep_offset = if flags & FUNC_NET != 0 { Some(r.u16()?) } else { None };
                Definition::Function { header, native, oper_precedence, flags, rep_offset, friendly_name: r.name()? }
            }
            "State" => Definition::State { header: StructHeader::read(r)?, state: StateInfo::read(r)? },
            "Class" => {
                let header = StructHeader::read(r)?;
                let state = StateInfo::read(r)?;
                let class_flags = r.u32()?;
                let within = r.i32()?;
                let config_name = r.name()?;
                let component_defaults = (0..r.count()?).map(|_| Ok((r.name()?, r.i32()?))).collect::<Result<_>>()?;
                let interfaces = (0..r.count()?).map(|_| Ok((r.i32()?, r.i32()?))).collect::<Result<_>>()?;
                // DontSortCategories, HideCategories, AutoExpandCategories, AutoCollapseCategories (UE3 order; E-026).
                let name_lists = (0..4).map(|_| names(r)).collect::<Result<_>>()?;
                let force_script_order = r.u32()?;
                let class_group_names = names(r)?;
                let header_filename = r.fstring()?;
                let dll_bind_name = r.name()?;
                let unknown_byte = r.bytes(1)?[0];
                Definition::Class {
                    header,
                    state,
                    class: ClassInfo {
                        class_flags, within, config_name, component_defaults, interfaces, name_lists, force_script_order,
                        class_group_names, header_filename, dll_bind_name, unknown_byte, default_object: r.i32()?,
                    },
                }
            }
            _ if class.ends_with("Property") => {
                let array_dim = r.i32()?;
                let flags = r.u64()?;
                let category = r.name()?;
                let array_enum = r.i32()?;
                let rep_offset = if flags & CPF_NET != 0 { Some(r.u16()?) } else { None };
                let kind = match class {
                    "ByteProperty" => PropertyKind::Byte { enum_: r.i32()? },
                    "ObjectProperty" | "ComponentProperty" => PropertyKind::Object { class: r.i32()? },
                    "ClassProperty" => PropertyKind::Class { class: r.i32()?, meta_class: r.i32()? },
                    "InterfaceProperty" => PropertyKind::Interface { class: r.i32()? },
                    "StructProperty" => PropertyKind::Struct { struct_: r.i32()? },
                    "ArrayProperty" => PropertyKind::Array { inner: r.i32()? },
                    "MapProperty" => PropertyKind::Map { key: r.i32()?, value: r.i32()? },
                    "DelegateProperty" => PropertyKind::Delegate { function: r.i32()?, source_delegate: r.i32()? },
                    "ByteAttributeProperty" => PropertyKind::Attribute { enum_: Some(r.i32()?), refs: [r.i32()?, r.i32()?] },
                    "IntAttributeProperty" | "FloatAttributeProperty" => PropertyKind::Attribute { enum_: None, refs: [r.i32()?, r.i32()?] },
                    _ => PropertyKind::Plain,
                };
                Definition::Property { array_dim, flags, category, array_enum, rep_offset, kind }
            }
            _ => return Err(bad(format!("unknown definition class {class}"))),
        };
        Ok((definition, next))
    }
}
