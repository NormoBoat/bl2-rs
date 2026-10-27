//! Local script-schema verification; no derived assets are written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use bl2_formats::package::Package;
use bl2_formats::schema::Schema;
use bl2_formats::script::{Definition, PropertyKind};

const PACKAGES: [&str; 9] = ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"];

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn script_schemas_link() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let schemas: Vec<_> = PACKAGES.iter().map(|name| {
        let p = Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap();
        Schema::read(name, &p).unwrap()
    }).collect();
    let mut all = BTreeMap::new();
    for schema in &schemas {
        for (path, field) in &schema.fields { assert!(all.insert(path.clone(), field).is_none(), "duplicate {path}"); }
    }
    let (mut properties, mut arrays, mut structs, mut classes, mut inherited) = (0, 0, 0, 0, 0);
    for schema in &schemas {
        assert!(schema.reference(0).unwrap().is_none());
        assert!(schema.reference(i32::MIN).is_err());
        for (path, field) in &schema.fields {
            if let Some(parent) = &field.super_struct {
                assert!(all.contains_key(parent), "missing super {path} -> {parent}");
                inherited += 1;
                let mut seen = BTreeSet::from([path.as_str()]);
                let mut next = Some(parent);
                while let Some(parent) = next {
                    assert!(seen.insert(parent.as_str()), "inheritance cycle at {parent}");
                    next = all[parent].super_struct.as_ref();
                }
            }
            match &field.definition {
                Definition::Property { array_dim, kind, .. } => {
                    properties += 1;
                    assert!(*array_dim > 0);
                    assert!(field.class.ends_with("Property"));
                    assert!(all.contains_key(field.outer.as_ref().expect("property owner")), "missing owner {path}");
                    let target = match kind {
                        PropertyKind::Array { inner } => { arrays += 1; Some((*inner, "Property")) },
                        PropertyKind::Struct { struct_ } => Some((*struct_, "ScriptStruct")),
                        _ => None,
                    };
                    if let Some((index, expected)) = target {
                        let target = schema.reference(index).unwrap().expect("required type reference");
                        assert!(all[target].class.ends_with(expected), "{path} -> {target}");
                    }
                }
                Definition::Class { .. } => classes += 1,
                Definition::ScriptStruct { .. } => structs += 1,
                _ => {}
            }
        }
    }
    println!("schemas={} definitions={} properties={properties} classes={classes} structs={structs} arrays={arrays} inherited={inherited}", schemas.len(), all.len());
    assert!(properties > 0 && arrays > 0 && classes > 0 && structs > 0 && inherited > 0);

    let mut core = Package::open(&root.join("WillowGame/CookedPCConsole/Core.upk")).unwrap();
    core.exports[0].outer = 1;
    assert!(Schema::read("Core", &core).unwrap_err().to_string().contains("cycle"));
    core.exports[0].outer = i32::MIN;
    assert!(Schema::read("Core", &core).is_err());
}

