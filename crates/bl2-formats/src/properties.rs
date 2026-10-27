//! UE3 object bodies: the per-object header and tagged properties.
//! Layout checked on the installation: plan/03-evidence.md E-035.

use std::collections::HashMap;
use std::io::{Error, ErrorKind, Result};
use std::ops::Range;

use crate::package::{Export, NameRef, Package, Reader};

const RF_HAS_STACK: u64 = 0x0200_0000_0000_0000;
const RF_CLASS_DEFAULT_OBJECT: u64 = 0x200;
const RF_ARCHETYPE_OBJECT: u64 = 0x400;

fn bad(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

/// Class name -> super class name, from class exports (`Export::class == 0`) of script packages.
#[derive(Default)]
pub struct ClassTree {
    parent: HashMap<String, String>,
}

impl ClassTree {
    pub fn add_package(&mut self, p: &Package) {
        for e in p.exports.iter().filter(|e| e.class == 0) {
            self.parent.insert(p.name(e.object_name), p.object_name(e.super_));
        }
    }
    pub fn contains(&self, class: &str) -> bool {
        self.parent.contains_key(class)
    }
    /// Classes used by cooked data but not defined in the shipped script packages
    /// (editor-only or intrinsic native classes).
    pub fn add_editor_classes(&mut self) {
        self.parent.insert("CascadeParticleSystemComponent".into(), "ParticleSystemComponent".into());
        self.parent.insert("LightMapTexture2D".into(), "Texture2D".into());
    }
    pub fn is_a(&self, class: &str, base: &str) -> bool {
        let mut c = class;
        loop {
            if c == base {
                return true;
            }
            match self.parent.get(c) {
                Some(p) => c = p,
                None => return false,
            }
        }
    }
}

/// Script definition objects (UField subclasses) use their own native layout (script corpus, T-015).
/// Property classes are listed explicitly: ordinary classes such as `SeqAct_GetProperty` also end in "Property".
pub fn is_script_definition(class: &str) -> bool {
    matches!(
        class,
        "Class" | "Function" | "State" | "ScriptStruct" | "Struct" | "Enum" | "Const" | "TextBuffer"
            | "ByteProperty" | "IntProperty" | "BoolProperty" | "FloatProperty" | "StrProperty" | "NameProperty"
            | "ObjectProperty" | "ClassProperty" | "ComponentProperty" | "InterfaceProperty" | "StructProperty"
            | "ArrayProperty" | "MapProperty" | "DelegateProperty"
            | "ByteAttributeProperty" | "IntAttributeProperty" | "FloatAttributeProperty"
    )
}

/// One serialized property; `value` is the byte range of its payload in `Package::data`.
#[derive(Clone, Debug)]
pub struct Tag {
    pub name: NameRef,
    pub type_name: NameRef,
    pub array_index: i32,
    /// StructProperty: struct name. ByteProperty: enum name (`None` for plain bytes).
    pub inner_name: Option<NameRef>,
    /// BoolProperty stores its value in the tag; `value` is then empty.
    pub bool_value: Option<bool>,
    pub value: Range<usize>,
}

/// A serialized object reference. Indices are zero-based and safe to use with this package's tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectReference {
    Null,
    Import(usize),
    Export(usize),
    /// Not a local table index; retain the encoding, never dereference it as one.
    /// CrossLevelObjectRef values (E-062) are `0xF0LLNNNN`: resolve with `CrossLevelGuids::import_guid` (E-067).
    Unresolved(i32),
}

impl ObjectReference {
    pub(crate) fn from_serialized(i: i32, imports: usize, exports: usize) -> Self {
        match i {
            0 => Self::Null,
            i if i > 0 && i as usize <= exports => Self::Export(i as usize - 1),
            i if i < 0 && i.unsigned_abs() as usize <= imports => Self::Import(i.unsigned_abs() as usize - 1),
            i => Self::Unresolved(i),
        }
    }
}

/// Scalar values retain package-local name references and unresolved object encodings.
#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Byte(u8),
    Enum(NameRef),
    Name(NameRef),
    String(String),
    Object(ObjectReference),
    Delegate { object: ObjectReference, function: NameRef },
}

fn scalar_tag(tag: &Tag, data: &[u8], names: &[String], imports: usize, exports: usize) -> Result<Option<ScalarValue>> {
    let checked_name = |n: NameRef| -> Result<NameRef> {
        if n.index >= names.len() || n.number < 0 {
            return Err(bad(format!("invalid property name reference {n:?}")));
        }
        Ok(n)
    };
    checked_name(tag.name)?;
    checked_name(tag.type_name)?;
    if tag.type_name.number != 0 || tag.array_index < 0 {
        return Err(bad("invalid property type suffix or array index".into()));
    }
    if let Some(n) = tag.inner_name {
        checked_name(n)?;
    }
    let payload = data.get(tag.value.clone()).ok_or_else(|| bad("property payload outside package".into()))?;
    let mut r = Reader::new(payload, 0);
    // orig: scalar tag payloads validated against the installation in E-062; composites need T-032a's schema.
    let value = match names[tag.type_name.index].as_str() {
        "BoolProperty" => ScalarValue::Bool(tag.bool_value.ok_or_else(|| bad("missing bool tag value".into()))?),
        "IntProperty" => ScalarValue::Int(r.i32()?),
        "FloatProperty" => ScalarValue::Float(f32::from_bits(r.u32()?)),
        "NameProperty" => ScalarValue::Name(checked_name(r.name()?)?),
        "StrProperty" => ScalarValue::String(r.fstring()?),
        "ByteProperty" => {
            let n = tag.inner_name.ok_or_else(|| bad("missing byte enum name".into()))?;
            if n.number == 0 && names[n.index] == "None" {
                ScalarValue::Byte(r.bytes(1)?[0])
            } else {
                ScalarValue::Enum(checked_name(r.name()?)?)
            }
        }
        "ObjectProperty" | "ClassProperty" | "ComponentProperty" | "InterfaceProperty" => {
            ScalarValue::Object(ObjectReference::from_serialized(r.i32()?, imports, exports))
        }
        "DelegateProperty" => ScalarValue::Delegate {
            object: ObjectReference::from_serialized(r.i32()?, imports, exports), function: checked_name(r.name()?)?,
        },
        _ => return Ok(None),
    };
    if r.pos != payload.len() {
        return Err(bad(format!("{} has {} trailing payload bytes", names[tag.type_name.index], payload.len() - r.pos)));
    }
    Ok(Some(value))
}

impl Package {
    /// Decode a scalar tag, requiring exact payload consumption and valid name references.
    /// Object encodings outside local tables remain explicitly unresolved, not validated objects.
    /// `None` means the type is not supported here (including arrays, structs and attribute properties).
    pub fn read_scalar_tag(&self, tag: &Tag) -> Result<Option<ScalarValue>> {
        scalar_tag(tag, &self.data, &self.names, self.imports.len(), self.exports.len())
    }

    fn is(&self, n: NameRef, s: &str) -> bool {
        n.number == 0 && self.names[n.index] == s
    }

    /// UE3 `IsTemplate(flags)`: the object or one of its outers (within this package) has any of `flags`.
    pub fn is_template(&self, e: &Export, flags: u64) -> bool {
        let mut cur = Some(e);
        while let Some(x) = cur {
            if x.object_flags & flags != 0 {
                return true;
            }
            cur = (x.outer > 0).then(|| &self.exports[x.outer as usize - 1]);
        }
        false
    }

    /// Skips what precedes the tagged properties and returns the object's NetIndex:
    /// class-specific native prefix, state frame (RF_HasStack), component template info, NetIndex.
    pub fn read_object_header(&self, e: &Export, tree: &ClassTree, r: &mut Reader) -> Result<i32> {
        let class = self.object_name(e.class);
        let template = self.is_template(e, RF_CLASS_DEFAULT_OBJECT | RF_ARCHETYPE_OBJECT);
        let cdo = e.object_flags & RF_CLASS_DEFAULT_OBJECT != 0;
        if !cdo && (tree.is_a(&class, "DominantDirectionalLightComponent") || tree.is_a(&class, "DominantSpotLightComponent")) {
            let n = r.count()?;
            r.bytes(n * 2)?; // DominantLightShadowMap: TArray<u16>, stored before the object header
        }
        let is_component = tree.is_a(&class, "Component");
        if e.object_flags & RF_HAS_STACK != 0 {
            let node = r.i32()?;
            r.i32()?; // StateNode
            r.u32()?; // ProbeMask
            r.u16()?; // LatentAction
            for _ in 0..r.count()? {
                r.i32()?; // pushed state
                r.i32()?; // its node
            }
            if node != 0 {
                r.i32()?; // code offset
            }
        }
        // Component template info: absent on class default objects themselves; TemplateName for objects
        // inside a class default object, or templates owned by an actor template that name their owner
        // class (rules found on the install, E-035).
        if is_component && !cdo {
            let template_owner_class = r.i32()?;
            let outer_is_actor = e.outer > 0 && tree.is_a(&self.object_name(self.exports[e.outer as usize - 1].class), "Actor");
            if self.is_template(e, RF_CLASS_DEFAULT_OBJECT) || (template_owner_class != 0 && template && outer_is_actor) {
                r.name()?;
            }
        }
        r.i32()
    }

    /// Reads tags up to and including the terminating `None`.
    pub fn read_tags(&self, r: &mut Reader) -> Result<Vec<Tag>> {
        let mut tags = Vec::new();
        loop {
            let name = r.name()?;
            if name.index >= self.names.len() {
                return Err(bad(format!("tag name index {} out of range at {}", name.index, r.pos - 8)));
            }
            if self.is(name, "None") {
                return Ok(tags);
            }
            let type_name = r.name()?;
            if type_name.index >= self.names.len() || !self.names[type_name.index].ends_with("Property") {
                return Err(bad(format!("bad property type for '{}' at {}", self.name(name), r.pos - 8)));
            }
            let size = r.count()?;
            let array_index = r.i32()?;
            let (mut inner_name, mut bool_value) = (None, None);
            match self.names[type_name.index].as_str() {
                "StructProperty" | "ByteProperty" => inner_name = Some(r.name()?),
                "BoolProperty" => bool_value = Some(r.bytes(1)?[0] != 0),
                _ => {}
            }
            let start = r.pos;
            r.bytes(size)?;
            tags.push(Tag { name, type_name, array_index, inner_name, bool_value, value: start..r.pos });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(kind: &str, payload: &[u8], inner: Option<usize>, boolean: Option<bool>) -> Result<Option<ScalarValue>> {
        let names = ["Value", kind, "None", "EnumType", "EnumValue"].map(String::from);
        let tag = Tag {
            name: NameRef { index: 0, number: 0 }, type_name: NameRef { index: 1, number: 0 },
            array_index: 0, inner_name: inner.map(|index| NameRef { index, number: 0 }),
            bool_value: boolean, value: 0..payload.len(),
        };
        scalar_tag(&tag, payload, &names, 2, 3)
    }

    #[test]
    fn scalar_values_preserve_bits_names_and_references() {
        assert_eq!(decode("IntProperty", &(-123i32).to_le_bytes(), None, None).unwrap(), Some(ScalarValue::Int(-123)));
        let Some(ScalarValue::Float(v)) = decode("FloatProperty", &0x80000000u32.to_le_bytes(), None, None).unwrap() else { panic!() };
        assert_eq!(v.to_bits(), 0x80000000); // preserve negative zero
        let Some(ScalarValue::Float(v)) = decode("FloatProperty", &0x7fc01234u32.to_le_bytes(), None, None).unwrap() else { panic!() };
        assert_eq!(v.to_bits(), 0x7fc01234); // preserve NaN payload
        assert_eq!(decode("BoolProperty", &[], None, Some(true)).unwrap(), Some(ScalarValue::Bool(true)));
        assert_eq!(decode("ByteProperty", &[255], Some(2), None).unwrap(), Some(ScalarValue::Byte(255)));
        let name = [4, 0, 0, 0, 2, 0, 0, 0];
        assert_eq!(decode("ByteProperty", &name, Some(3), None).unwrap(), Some(ScalarValue::Enum(NameRef { index: 4, number: 2 })));
        assert_eq!(decode("NameProperty", &name, None, None).unwrap(), Some(ScalarValue::Name(NameRef { index: 4, number: 2 })));
        for kind in ["ObjectProperty", "ClassProperty", "ComponentProperty", "InterfaceProperty"] {
            for (index, reference) in [(-2i32, ObjectReference::Import(1)), (0, ObjectReference::Null), (3, ObjectReference::Export(2))] {
                assert_eq!(decode(kind, &index.to_le_bytes(), None, None).unwrap(), Some(ScalarValue::Object(reference)));
            }
        }
        let mut delegate = (-2i32).to_le_bytes().to_vec();
        delegate.extend_from_slice(&name);
        assert_eq!(decode("DelegateProperty", &delegate, None, None).unwrap(),
            Some(ScalarValue::Delegate { object: ObjectReference::Import(1), function: NameRef { index: 4, number: 2 } }));
    }

    #[test]
    fn strings_and_unsupported_composites() {
        assert_eq!(decode("StrProperty", &[2, 0, 0, 0, 0xe9, 0], None, None).unwrap(), Some(ScalarValue::String("é".into())));
        let mut utf16 = (-3i32).to_le_bytes().to_vec();
        for unit in [0xd83du16, 0xde80, 0] { utf16.extend_from_slice(&unit.to_le_bytes()); }
        assert_eq!(decode("StrProperty", &utf16, None, None).unwrap(), Some(ScalarValue::String("🚀".into())));
        assert_eq!(decode("StrProperty", &[0; 4], None, None).unwrap(), Some(ScalarValue::String(String::new())));
        assert_eq!(decode("ArrayProperty", &[255], None, None).unwrap(), None);
        assert_eq!(decode("StructProperty", &[255], Some(3), None).unwrap(), None);
    }

    #[test]
    fn rejects_malformed_scalar_payloads() {
        for payload in [&[0u8; 3][..], &[0; 5][..]] {
            assert!(decode("IntProperty", payload, None, None).is_err());
        }
        assert!(decode("BoolProperty", &[0], None, Some(false)).is_err());
        assert!(decode("BoolProperty", &[], None, None).is_err());
        assert!(decode("ByteProperty", &[0], None, None).is_err());
        assert!(decode("ByteProperty", &[0], Some(99), None).is_err());
        for index in [-3i32, 4, i32::MIN] {
            assert_eq!(decode("ObjectProperty", &index.to_le_bytes(), None, None).unwrap(), Some(ScalarValue::Object(ObjectReference::Unresolved(index))));
        }
        assert!(decode("NameProperty", &[5, 0, 0, 0, 0, 0, 0, 0], None, None).is_err());
        assert!(decode("NameProperty", &[0, 0, 0, 0, 255, 255, 255, 255], None, None).is_err());
        assert!(decode("StrProperty", &[2, 0, 0, 0, 65, 66], None, None).is_err());
        assert!(decode("StrProperty", &[255, 255, 255, 255, 65, 0], None, None).is_err());
        assert!(Reader::new(&[], usize::MAX).bytes(1).is_err());
    }
}
