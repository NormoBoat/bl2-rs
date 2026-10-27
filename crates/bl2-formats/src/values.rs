//! Schema-driven property values (T-032a). Every layout rule is checked by exact payload consumption on the
//! installation (E-064); anything the schema cannot describe is an error, never a guess.

use std::collections::BTreeMap;
use std::io::{Error, ErrorKind, Result};
use std::ops::Range;
use crate::package::{NameRef, Package, Reader};
use crate::properties::{ObjectReference, ScalarValue, Tag};
use crate::schema::{Field, Schema};
use crate::script::{Definition, PropertyKind};

const STRUCT_IMMUTABLE: u32 = 0x20;
const STRUCT_IMMUTABLE_WHEN_COOKED: u32 = 0x80;
const CPF_TRANSIENT: u64 = 0x2000;

#[derive(Debug)]
pub enum Value {
    Scalar(ScalarValue),
    Array(Vec<Value>),
    /// Tagged struct, in serialized order.
    Struct(Vec<(Tag, Value)>),
    /// Immutable struct, serialized without tags: each property in link order (super first), ArrayDim values.
    Immutable(Vec<(String, Vec<Value>)>),
    /// No script schema describes the property: editor-only data left in cooked packages, or a class that is
    /// not defined in the script packages (intrinsic/editor classes). `value` is decoded when the tag alone
    /// names its type (scalars, structs); otherwise only the byte range is kept.
    Untyped { value: Option<Box<Value>>, bytes: Range<usize> },
}

type Entry<'a> = (&'a Schema, &'a Field);

pub struct Decoder<'a> {
    fields: BTreeMap<&'a str, Entry<'a>>,
    /// Short struct name -> path; `None` when the name is ambiguous.
    structs: BTreeMap<&'a str, Option<&'a str>>,
    /// ScriptStruct path -> its binary layout (super properties first, then `Children`/`Next` order),
    /// with whether each property is transient.
    links: BTreeMap<&'a str, Vec<(&'a str, Entry<'a>, bool)>>,
}

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

fn array_count(r: &mut Reader) -> Result<usize> {
    let count = r.count()?;
    if count > r.data.len().saturating_sub(r.pos) { return Err(bad("array count exceeds remaining payload")); }
    Ok(count)
}

fn short(path: &str) -> &str { path.rsplit('.').next().unwrap() }

impl<'a> Decoder<'a> {
    pub fn new(schemas: &'a [Schema]) -> Result<Self> {
        let mut fields = BTreeMap::new();
        let mut structs = BTreeMap::new();
        for schema in schemas {
            for (path, field) in &schema.fields {
                if fields.insert(path.as_str(), (schema, field)).is_some() { return Err(bad("duplicate schema field")); }
                if let Definition::ScriptStruct { .. } = field.definition {
                    structs.entry(short(path)).and_modify(|p| *p = None).or_insert(Some(path.as_str()));
                }
            }
        }
        let mut decoder = Self { fields, structs, links: BTreeMap::new() };
        for &path in decoder.structs.values().flatten() {
            let link = decoder.link(path, 0)?;
            decoder.links.insert(path, link);
        }
        // Ambiguous short names still need their own layouts.
        for (&path, (_, field)) in &decoder.fields {
            if matches!(field.definition, Definition::ScriptStruct { .. }) && !decoder.links.contains_key(path) {
                let link = decoder.link(path, 0)?;
                decoder.links.insert(path, link);
            }
        }
        Ok(decoder)
    }

    fn entry(&self, path: &str) -> Result<(&'a str, Entry<'a>)> {
        self.fields.get_key_value(path).map(|(k, v)| (*k, *v)).ok_or_else(|| bad(format!("missing schema: {path}")))
    }

    fn link(&self, path: &str, depth: usize) -> Result<Vec<(&'a str, Entry<'a>, bool)>> {
        if depth > 64 { return Err(bad("struct inheritance too deep")); }
        let (_, (schema, field)) = self.entry(path)?;
        let (Definition::ScriptStruct { header, .. } | Definition::Struct(header)) = &field.definition else {
            return Err(bad(format!("not a struct: {path}")));
        };
        let mut out = match &field.super_struct { Some(parent) => self.link(parent, depth + 1)?, None => Vec::new() };
        let mut next = header.children;
        while next != 0 {
            if out.len() > self.fields.len() { return Err(bad(format!("cycle in children of {path}"))); }
            let (child, entry) = self.entry(schema.reference(next)?.unwrap())?;
            if let Definition::Property { flags, .. } = entry.1.definition { out.push((child, entry, flags & CPF_TRANSIENT != 0)); }
            next = entry.1.next;
        }
        Ok(out)
    }

    /// Property `name` of class or struct `owner`, searching super classes. `None` when there is no such
    /// property or `owner` itself is not in the schema.
    fn property(&self, owner: &str, name: &str) -> Result<Option<Entry<'a>>> {
        if !self.fields.contains_key(owner) { return Ok(None); }
        let mut current = owner;
        for _ in 0..=self.fields.len() {
            if let Some(field) = self.fields.get(format!("{current}.{name}").as_str()) { return Ok(Some(*field)); }
            match self.entry(current)?.1.1.super_struct.as_deref() {
                Some(parent) => current = parent,
                None => return Ok(None),
            }
        }
        Err(bad("inheritance cycle"))
    }

    /// Decodes one tag of an object whose class (or struct) path is `owner`.
    /// `class_default`: the object is a class default object (RF_ClassDefaultObject export).
    pub fn read_tag(&self, package: &Package, owner: &str, tag: &Tag, class_default: bool) -> Result<Value> {
        self.tag(package, owner, tag, class_default, 0)
    }

    fn tag(&self, p: &Package, owner: &str, tag: &Tag, cdo: bool, depth: usize) -> Result<Value> {
        let scalar = p.read_scalar_tag(tag)?;
        let type_name = p.name(tag.type_name);
        let mut r = Reader::new(p.data.get(..tag.value.end).ok_or_else(|| bad("payload outside package"))?, tag.value.start);
        let Some((schema, field)) = self.property(owner, &p.name(tag.name))? else {
            let value = match scalar {
                Some(s) => Some(Value::Scalar(s)),
                None if type_name == "StructProperty" => {
                    let inner = p.name(tag.inner_name.unwrap());
                    let path = self.structs.get(inner.as_str()).ok_or_else(|| bad(format!("unknown struct {inner}")))?
                        .ok_or_else(|| bad(format!("ambiguous struct name {inner}")))?;
                    Some(self.structure(p, path, &mut r, cdo, depth + 1)?)
                }
                None => None,
            };
            if value.is_some() && !matches!(value, Some(Value::Scalar(_))) && r.pos != tag.value.end {
                return Err(bad(format!("untyped struct has {} trailing bytes", tag.value.end - r.pos)));
            }
            return Ok(Value::Untyped { value: value.map(Box::new), bytes: tag.value.clone() });
        };
        let Definition::Property { array_dim, kind, .. } = &field.definition else { return Err(bad("schema field is not a property")); };
        if tag.array_index >= *array_dim { return Err(bad("static array index exceeds ArrayDim")); }
        // orig: Class and Component properties are tagged as ObjectProperty (E-064).
        let tag_class = if matches!(field.class.as_str(), "ClassProperty" | "ComponentProperty") { "ObjectProperty" } else { &field.class };
        if type_name != tag_class { return Err(bad(format!("tag type {type_name} does not match {}", field.class))); }
        let expected_inner = match kind {
            PropertyKind::Struct { struct_ } => Some(short(schema.reference(*struct_)?.ok_or_else(|| bad("null struct type"))?)),
            PropertyKind::Byte { enum_: 0 } => Some("None"),
            PropertyKind::Byte { enum_ } => Some(short(schema.reference(*enum_)?.unwrap())),
            _ => None,
        };
        if tag.inner_name.map(|n| p.name(n)).as_deref() != expected_inner {
            return Err(bad(format!("tag inner type does not match schema {expected_inner:?}")));
        }
        if let Some(s) = scalar { return Ok(Value::Scalar(s)); }
        let value = self.item(p, schema, field, &mut r, cdo, depth + 1)?;
        if r.pos != tag.value.end { return Err(bad(format!("{type_name} has {} trailing bytes", tag.value.end - r.pos))); }
        Ok(value)
    }

    fn structure(&self, p: &Package, path: &str, r: &mut Reader, cdo: bool, depth: usize) -> Result<Value> {
        if depth > 64 { return Err(bad("value nesting exceeds 64")); }
        let (path, (_, field)) = self.entry(path)?;
        let Definition::ScriptStruct { struct_flags, .. } = field.definition else { return Err(bad(format!("not a ScriptStruct: {path}"))); };
        // orig: UE3 serializes Immutable (and, in cooked packages, ImmutableWhenCooked) structs without tags.
        if struct_flags & (STRUCT_IMMUTABLE | STRUCT_IMMUTABLE_WHEN_COOKED) == 0 {
            return p.read_tags(r)?.into_iter().map(|tag| Ok((tag.clone(), self.tag(p, path, &tag, cdo, depth)?)))
                .collect::<Result<_>>().map(Value::Struct);
        }
        // orig: transient properties are serialized only for class defaults (UE3 `!IsSerializingDefaults`;
        // exact consumption on the installation, E-064).
        self.links[path].iter().filter(|&&(_, _, transient)| cdo || !transient).map(|&(name, (schema, field), _)| {
            let Definition::Property { array_dim, .. } = field.definition else { unreachable!() };
            let values = (0..array_dim).map(|_| self.item(p, schema, field, r, cdo, depth + 1)).collect::<Result<_>>()?;
            Ok((short(name).to_owned(), values))
        }).collect::<Result<_>>().map(Value::Immutable)
    }

    /// One value of a property without its tag: an array element, an immutable struct member or a composite tag payload.
    fn item(&self, p: &Package, schema: &Schema, field: &Field, r: &mut Reader, cdo: bool, depth: usize) -> Result<Value> {
        if depth > 64 { return Err(bad("value nesting exceeds 64")); }
        let Definition::Property { kind, .. } = &field.definition else { return Err(bad("not a property")); };
        let name = |r: &mut Reader| -> Result<NameRef> {
            let n = r.name()?;
            if n.index >= p.names.len() || n.number < 0 { return Err(bad("invalid name reference")); }
            Ok(n)
        };
        let object = |r: &mut Reader| -> Result<ObjectReference> {
            Ok(ObjectReference::from_serialized(r.i32()?, p.imports.len(), p.exports.len()))
        };
        Ok(Value::Scalar(match kind {
            PropertyKind::Array { inner } => {
                let (_, (inner_schema, inner_field)) = self.entry(schema.reference(*inner)?.ok_or_else(|| bad("null array inner"))?)?;
                if !matches!(inner_field.definition, Definition::Property { array_dim: 1, .. }) { return Err(bad("array inner ArrayDim is not 1")); }
                let count = array_count(r)?;
                return (0..count).map(|_| self.item(p, inner_schema, inner_field, r, cdo, depth + 1)).collect::<Result<_>>().map(Value::Array);
            }
            PropertyKind::Struct { struct_ } => {
                return self.structure(p, schema.reference(*struct_)?.ok_or_else(|| bad("null struct type"))?, r, cdo, depth + 1);
            }
            // Enumerations are stored as their value names, as in scalar tags (E-062).
            PropertyKind::Byte { enum_: 0 } | PropertyKind::Attribute { enum_: Some(0), .. } => ScalarValue::Byte(r.bytes(1)?[0]),
            PropertyKind::Byte { .. } | PropertyKind::Attribute { enum_: Some(_), .. } => ScalarValue::Enum(name(r)?),
            _ => match field.class.as_str() {
                "BoolProperty" => match r.bytes(1)?[0] {
                    0 => ScalarValue::Bool(false),
                    1 => ScalarValue::Bool(true),
                    b => return Err(bad(format!("bool byte {b}"))),
                },
                "IntProperty" | "IntAttributeProperty" => ScalarValue::Int(r.i32()?),
                "FloatProperty" | "FloatAttributeProperty" => ScalarValue::Float(f32::from_bits(r.u32()?)),
                "NameProperty" => ScalarValue::Name(name(r)?),
                "StrProperty" => ScalarValue::String(r.fstring()?),
                "ObjectProperty" | "ClassProperty" | "ComponentProperty" | "InterfaceProperty" => ScalarValue::Object(object(r)?),
                "DelegateProperty" => ScalarValue::Delegate { object: object(r)?, function: name(r)? },
                class => return Err(bad(format!("unsupported property type {class}"))),
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_array_counts() {
        for data in [&[0u8; 3][..], &[255, 255, 255, 255][..], &[255, 255, 255, 127][..], &[1, 0, 0, 0][..]] {
            assert!(array_count(&mut Reader::new(data, 0)).is_err());
        }
        assert_eq!(array_count(&mut Reader::new(&[0; 4], 0)).unwrap(), 0);
        assert_eq!(array_count(&mut Reader::new(&[1, 0, 0, 0, 1], 0)).unwrap(), 1);
    }
}
