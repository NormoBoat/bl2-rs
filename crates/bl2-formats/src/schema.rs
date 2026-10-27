//! Package-qualified script metadata, without executing bytecode (E-026, E-063).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind, Result};

use crate::package::{Package, Reader};
use crate::properties::is_script_definition;
use crate::script::{Definition, PropertyKind};

#[derive(Debug)]
pub struct Field {
    pub class: String,
    pub outer: Option<String>,
    pub super_struct: Option<String>,
    /// Original metadata, including ArrayDim, flags and PropertyKind.
    /// Object indices belong to this schema's package; use `Schema::reference`.
    /// NameRef and byte ranges still refer to the original Package; keep it for decoding defaults.
    pub definition: Definition,
    /// `UField::Next`: index (in this schema's package) of the next field of the owner, 0 at the end.
    pub next: i32,
}

#[derive(Debug)]
pub struct Schema {
    /// Keys include the package and every outer, e.g. `Core.Object.Vector.X`.
    pub fields: BTreeMap<String, Field>,
    references: BTreeMap<i32, String>,
}

const EF_FORCED_EXPORT: u32 = 1;

fn bad(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, message.into())
}

impl Schema {
    /// `package_name` is the logical package name, normally the file stem.
    /// Imports already include their package outer; exports receive this prefix.
    pub fn read(package_name: &str, package: &Package) -> Result<Self> {
        if package_name.is_empty() || package_name.contains('.') {
            return Err(bad("expected a nonempty package name without dots"));
        }
        let mut schema = Self { fields: BTreeMap::new(), references: BTreeMap::new() };
        for i in 0..package.imports.len() {
            let index = -i32::try_from(i + 1).map_err(|_| bad("too many imports"))?;
            schema.references.insert(index, object_path(package, package_name, index)?);
        }
        for i in 0..package.exports.len() {
            let index = i32::try_from(i + 1).map_err(|_| bad("too many exports"))?;
            schema.references.insert(index, object_path(package, package_name, index)?);
        }
        for (i, export) in package.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0) {
            let class = if export.class == 0 { "Class".to_owned() } else {
                schema.reference(export.class)?.unwrap().rsplit('.').next().unwrap().to_owned()
            };
            if !is_script_definition(&class) {
                continue;
            }
            let end = export.serial_offset.checked_add(export.serial_size).ok_or_else(|| bad("export range overflow"))?;
            let data = package.data.get(..end).ok_or_else(|| bad("export outside package"))?;
            let mut reader = Reader::new(data, export.serial_offset);
            let (definition, next) = package.read_definition_and_next(export, &class, &mut reader)?;
            if reader.pos != end {
                return Err(bad(format!("{class}: definition did not consume export")));
            }
            schema.reference(next)?;
            let super_index = match &definition {
                Definition::Struct(header) | Definition::ScriptStruct { header, .. }
                | Definition::Class { header, .. } | Definition::Function { header, .. }
                | Definition::State { header, .. } => header.super_struct,
                _ => 0,
            };
            if let Definition::Property { array_dim, array_enum, kind, .. } = &definition {
                if *array_dim <= 0 { return Err(bad("nonpositive property ArrayDim")); }
                schema.reference(*array_enum)?;
                let refs = match kind {
                    PropertyKind::Plain => vec![],
                    PropertyKind::Byte { enum_ } => vec![*enum_],
                    PropertyKind::Object { class } | PropertyKind::Interface { class } => vec![*class],
                    PropertyKind::Class { class, meta_class } => vec![*class, *meta_class],
                    PropertyKind::Struct { struct_ } => vec![*struct_],
                    PropertyKind::Array { inner } => vec![*inner],
                    PropertyKind::Map { key, value } => vec![*key, *value],
                    PropertyKind::Delegate { function, source_delegate } => vec![*function, *source_delegate],
                    PropertyKind::Attribute { enum_, refs } => vec![enum_.unwrap_or(0), refs[0], refs[1]],
                };
                for index in refs { schema.reference(index)?; }
            }
            let field = Field {
                class,
                outer: schema.reference(export.outer)?.map(str::to_owned),
                super_struct: schema.reference(super_index)?.map(str::to_owned),
                definition,
                next,
            };
            let path = schema.reference((i + 1) as i32)?.unwrap().to_owned();
            if schema.fields.insert(path.clone(), field).is_some() {
                return Err(bad(format!("duplicate definition path {path}")));
            }
        }
        Ok(schema)
    }

    /// Resolves a metadata package index. Zero is null, never an object named None.
    pub fn reference(&self, index: i32) -> Result<Option<&str>> {
        if index == 0 { return Ok(None); }
        self.references.get(&index).map(|s| Some(s.as_str())).ok_or_else(|| bad(format!("invalid metadata reference {index}")))
    }
}

/// Qualified path of a non-null import/export, rejecting invalid indices and outer cycles.
/// Exports receive the `package_name` prefix unless their top-level outer is a forced export.
pub fn object_path(package: &Package, package_name: &str, mut index: i32) -> Result<String> {
    if index == 0 { return Err(bad("null object has no path")); }
    let mut parts = Vec::new();
    let mut visited = BTreeSet::new();
    while index != 0 {
        if !visited.insert(index) { return Err(bad("cycle in object outers")); }
        let (name, outer, forced) = if index > 0 {
            let e = package.exports.get(index as usize - 1).ok_or_else(|| bad("invalid export outer"))?;
            (e.object_name, e.outer, e.export_flags & EF_FORCED_EXPORT != 0)
        } else {
            let i = package.imports.get(index.unsigned_abs() as usize - 1).ok_or_else(|| bad("invalid import outer"))?;
            (i.object_name, i.outer, false)
        };
        if name.index >= package.names.len() || name.number < 0 { return Err(bad("invalid object name")); }
        parts.push(package.name(name));
        // A top-level forced export is a package of its own (seen in WillowGame.upk: Package WillowHUD).
        if outer == 0 && index > 0 && !forced { parts.push(package_name.to_owned()); }
        index = outer;
    }
    parts.reverse();
    Ok(parts.join("."))
}
