//! Inherited, explicitly serialized class-default tags (E-065).
//! This does not synthesize zero values, apply ini settings, or instantiate archetype subobjects.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind, Result};
use crate::package::{Package, Reader};
use crate::properties::{ClassTree, Tag};
use crate::schema::Schema;
use crate::script::Definition;

const RF_CLASS_DEFAULT_OBJECT: u64 = 0x200;

pub type PropertyKey = (String, i32);

pub struct SourceTag<'a> {
    pub package: &'a Package,
    pub source_class: &'a str,
    pub tag: Tag,
}

pub struct ClassDefaults<'a> {
    pub parent: Option<&'a str>,
    /// Null class.default_object is retained rather than treated as an empty CDO.
    pub cdo_path: Option<&'a str>,
    pub tags: BTreeMap<PropertyKey, SourceTag<'a>>,
}

pub struct Defaults<'a> {
    pub classes: BTreeMap<&'a str, ClassDefaults<'a>>,
}

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

impl<'a> Defaults<'a> {
    /// Sources must pair each schema with the package from which it was read.
    pub fn read(sources: &[(&'a Schema, &'a Package)]) -> Result<Self> {
        let mut objects = BTreeMap::new();
        let mut tree = ClassTree::default();
        for &(schema, package) in sources {
            tree.add_package(package);
            // Only class default objects are looked up; forced exports repeat other objects across packages.
            for (i, export) in package.exports.iter().enumerate().filter(|(_, e)| e.object_flags & RF_CLASS_DEFAULT_OBJECT != 0) {
                let path = schema.reference((i + 1) as i32)?.ok_or_else(|| bad("null export path"))?;
                if objects.insert(path, (schema, package, export)).is_some() { return Err(bad(format!("duplicate object path {path}"))); }
            }
        }
        tree.add_editor_classes();
        let mut result = Self {classes: BTreeMap::new()};
        for &(schema, _) in sources {
            for (path, field) in &schema.fields {
                let Definition::Class {class, ..} = &field.definition else {continue;};
                let cdo_path = schema.reference(class.default_object)?;
                let mut tags = BTreeMap::new();
                if let Some(cdo_path) = cdo_path {
                    let &(cdo_schema, package, export) = objects.get(cdo_path).ok_or_else(|| bad(format!("missing CDO {cdo_path}")))?;
                    if export.object_flags & RF_CLASS_DEFAULT_OBJECT == 0 || cdo_schema.reference(export.class)? != Some(path) {
                        return Err(bad(format!("CDO flags or class mismatch: {cdo_path}")));
                    }
                    let end = export.serial_offset.checked_add(export.serial_size).ok_or_else(|| bad("CDO range overflow"))?;
                    let mut r = Reader::new(package.data.get(..end).ok_or_else(|| bad("CDO outside package"))?, export.serial_offset);
                    package.read_object_header(export, &tree, &mut r)?;
                    for tag in package.read_tags(&mut r)? {
                        package.read_scalar_tag(&tag)?; // Also validates names and nonnegative array index for composites.
                        let key = (package.name(tag.name), tag.array_index);
                        if tags.insert(key, SourceTag {package, source_class: path, tag}).is_some() {
                            return Err(bad(format!("duplicate CDO property: {cdo_path}")));
                        }
                    }
                    // Native tails are outside explicit tagged defaults and remain in the source package.
                }
                let defaults = ClassDefaults {parent: field.super_struct.as_deref(), cdo_path, tags};
                if result.classes.insert(path, defaults).is_some() {return Err(bad("duplicate class defaults"));}
            }
        }
        for class in result.classes.keys() { result.chain(class)?; }
        Ok(result)
    }

    fn chain(&self, class: &str) -> Result<Vec<&ClassDefaults<'a>>> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = Some(class);
        while let Some(path) = next {
            if !seen.insert(path) {return Err(bad("class-default inheritance cycle"));}
            let defaults = self.classes.get(path).ok_or_else(|| bad(format!("missing superclass defaults: {path}")))?;
            result.push(defaults);
            next = defaults.parent;
        }
        Ok(result)
    }

    /// Derived explicit tags replace base tags by (name, static array index).
    /// An absent key means no serialized value in this chain, not an implicit zero/null.
    pub fn effective(&self, class: &str) -> Result<BTreeMap<PropertyKey, &SourceTag<'a>>> {
        let mut values = BTreeMap::new();
        for defaults in self.chain(class)?.into_iter().rev() {
            for (key, value) in &defaults.tags {values.insert(key.clone(), value);}
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{NameRef, Summary};
    use crate::properties::{ObjectReference, ScalarValue};

    fn fixture() -> Package {
        let mut summary = vec![0; 128];
        summary[..4].copy_from_slice(&0x9e2a83c1u32.to_le_bytes());
        summary[4..6].copy_from_slice(&832u16.to_le_bytes());
        summary[6..8].copy_from_slice(&46u16.to_le_bytes());
        Package {summary: Summary::read(&mut Reader::new(&summary, 0)).unwrap(), data: vec![0; 4],
            names: vec!["Value".into(), "ObjectProperty".into()], imports: vec![], exports: vec![]}
    }

    #[test]
    fn explicit_null_overrides_and_array_indices_stay_separate() {
        let base = fixture(); let child = fixture();
        let tag = Tag {name: NameRef {index: 0, number: 0}, type_name: NameRef {index: 1, number: 0},
            array_index: 0, inner_name: None, bool_value: None, value: 0..4};
        let source = |package, source_class, index| SourceTag {package, source_class, tag: Tag {array_index: index, ..tag.clone()}};
        let defaults = Defaults {classes: BTreeMap::from([
            ("Base", ClassDefaults {parent: None, cdo_path: Some("BaseCDO"), tags: BTreeMap::from([
                (("Value".into(), 0), source(&base, "Base", 0)), (("Value".into(), 1), source(&base, "Base", 1)),
            ])}),
            ("Child", ClassDefaults {parent: Some("Base"), cdo_path: Some("ChildCDO"), tags: BTreeMap::from([
                (("Value".into(), 0), source(&child, "Child", 0)),
            ])}),
        ])};
        let values = defaults.effective("Child").unwrap();
        assert_eq!(values.len(), 2);
        let overridden = values[&("Value".into(), 0)];
        assert!(std::ptr::eq(overridden.package, &child));
        assert_eq!(overridden.source_class, "Child");
        assert_eq!(overridden.package.read_scalar_tag(&overridden.tag).unwrap(), Some(ScalarValue::Object(ObjectReference::Null)));
        assert_eq!(values[&("Value".into(), 1)].source_class, "Base");
        assert!(!values.contains_key(&("Absent".into(), 0)));
    }

    #[test]
    fn rejects_cycles_and_missing_superclasses() {
        let mut defaults = Defaults {classes: BTreeMap::from([("A", ClassDefaults {parent: Some("A"), cdo_path: None, tags: BTreeMap::new()})])};
        assert!(defaults.effective("A").is_err());
        defaults.classes.get_mut("A").unwrap().parent = Some("Missing");
        assert!(defaults.effective("A").is_err());
    }
}
