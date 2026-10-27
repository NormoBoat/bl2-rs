use bl2_formats::{package, properties, schema, script};
use package::Package;
use schema::{Field, Schema};
use script::Definition;
use bl2_formats::values::{Decoder, Value};
use properties::{ClassTree, ObjectReference, ScalarValue, is_script_definition};
use package::Reader;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PACKAGES: [&str; 9] = ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"];

#[test]
#[ignore = "local metadata investigation"]
fn struct_flags() {
    let root = std::path::PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut counts = std::collections::BTreeMap::new();
    for name in PACKAGES {
        let p = Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap();
        let s = Schema::read(name, &p).unwrap();
        for (path, field) in s.fields {
            if let Definition::ScriptStruct {struct_flags, ..} = field.definition {
                let e = counts.entry(struct_flags).or_insert_with(|| (0, Vec::new()));
                e.0 += 1;
                if e.1.len() < 8 {e.1.push(path);}
            }
        }
    }
    println!("{counts:#?}");
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {files(&p, out);}
        else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) &&
            !matches!(p.file_name().unwrap().to_str().unwrap(), "Mancana_Textures_Data.upk" | "Mancana_Textures_Meta.upk") {out.push(p);}
    }
}

/// stats: scalar, array, tagged struct, immutable struct, unresolved reference, untyped (decoded), untyped (raw).
/// `key` is the path of the tag holding `value`; untyped values are counted per path and tag type.
fn count(value: &Value, p: &Package, stats: &mut [usize; 7], untyped: &mut BTreeMap<String, (usize, usize)>, key: &str) {
    match value {
        Value::Scalar(s) => {
            stats[0] += 1;
            if matches!(s, ScalarValue::Object(ObjectReference::Unresolved(_)) | ScalarValue::Delegate { object: ObjectReference::Unresolved(_), .. }) {stats[4] += 1;}
        }
        Value::Array(v) => {stats[1] += 1; for v in v {count(v, p, stats, untyped, &format!("{key}[]"));}}
        Value::Struct(v) => {stats[2] += 1; for (tag, v) in v {count(v, p, stats, untyped, &format!("{key}.{} {}", p.name(tag.name), p.name(tag.type_name)));}}
        Value::Immutable(v) => {stats[3] += 1; for (name, v) in v {for v in v {count(v, p, stats, untyped, &format!("{key}.{name}"));}}}
        Value::Untyped {value, bytes} => {
            assert!(bytes.start <= bytes.end);
            let entry = untyped.entry(key.to_owned()).or_default();
            match value {Some(v) => {stats[5] += 1; entry.0 += 1; count(v, p, stats, &mut BTreeMap::new(), key);} None => {stats[6] += 1; entry.1 += 1;}}
        }
    }
}

#[test]
#[ignore = "needs the user's installation copy in BL2_INSTALL"]
fn every_composite_value() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut tree = ClassTree::default();
    let schemas: Vec<_> = PACKAGES.iter().map(|name| {
        let p = Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap();
        tree.add_package(&p);
        Schema::read(name, &p).unwrap()
    }).collect();
    tree.add_editor_classes();
    let decoder = Decoder::new(&schemas).unwrap();
    let mut paths = Vec::new(); files(&root, &mut paths); paths.sort();
    let mut stats = [0; 7];
    let mut absent = BTreeMap::<String, (usize, usize)>::new();
    let mut errors = BTreeMap::<String, (usize, String)>::new();
    let mut top = 0;
    let mut malformed = false;
    let mut malformed_struct = false;
    let mut malformed_immutable = false;
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && !is_script_definition(&p.object_name(e.class))) {
            let owner = schema::object_path(&p, path.file_stem().unwrap().to_str().unwrap(), e.class).unwrap();
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let cdo = e.object_flags & 0x200 != 0;
            for tag in p.read_tags(&mut r).unwrap() {
                top += 1;
                match decoder.read_tag(&p, &owner, &tag, cdo) {
                    Ok(value) => {
                        if !malformed && matches!(value, Value::Array(_)) && tag.value.len() >= 4 {
                            let mut broken = tag.clone(); broken.value.end = broken.value.start + 3;
                            assert!(decoder.read_tag(&p, &owner, &broken, cdo).is_err());
                            broken = tag.clone(); broken.array_index = i32::MAX;
                            assert!(decoder.read_tag(&p, &owner, &broken, cdo).is_err());
                            malformed = true;
                        }
                        if !malformed_immutable && matches!(value, Value::Immutable(_)) {
                            let mut broken = tag.clone(); broken.value.end -= 1;
                            assert!(decoder.read_tag(&p, &owner, &broken, cdo).is_err());
                            malformed_immutable = true;
                        }
                        if !malformed_struct && matches!(value, Value::Struct(_)) {
                            let mut broken = tag.clone(); broken.value.end -= 1;
                            assert!(decoder.read_tag(&p, &owner, &broken, cdo).is_err());
                            malformed_struct = true;
                        }
                        let key = format!("{owner}.{} {}", p.name(tag.name), p.name(tag.type_name));
                        count(&value, &p, &mut stats, &mut absent, &key);
                    }
                    Err(err) => {
                        let key = format!("{} {}", p.name(tag.type_name), err);
                        let v = errors.entry(key).or_insert_with(|| (0, format!("{} {}.{}", path.display(), owner, p.name(tag.name)))); v.0 += 1;
                    }
                }
            }
        }
    }
    println!("packages={} top={top} scalar={} arrays={} structs={} immutable={} unresolved={} untyped_decoded={} untyped_raw={}",
        paths.len(), stats[0], stats[1], stats[2], stats[3], stats[4], stats[5], stats[6]);
    let mut absent: Vec<_> = absent.into_iter().collect(); absent.sort_by_key(|(_, (d, r))| std::cmp::Reverse(d + r));
    println!("untyped paths={}", absent.len());
    for (key, (decoded, raw)) in &absent {println!("UNTYPED decoded={decoded} raw={raw} {key}");}
    println!("ERRORS {errors:#?}");
    assert!(errors.is_empty()); assert!(malformed && malformed_struct && malformed_immutable);
}

#[test]
#[ignore = "local metadata investigation"]
fn struct_layouts() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let schemas: Vec<_> = PACKAGES.iter().map(|name| {
        Schema::read(name, &Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()).unwrap()
    }).collect();
    let all: BTreeMap<_, _> = schemas.iter().flat_map(|s| s.fields.iter().map(move |(k, v)| (k.clone(), (s, v)))).collect();
    for want in std::env::var("STRUCTS").unwrap().split(',') {
        let (s, f) = all[want];
        let Definition::ScriptStruct {header, struct_flags, ..} = &f.definition else {panic!()};
        println!("{want} flags={struct_flags:#x} super={:?}", f.super_struct);
        let mut next = header.children;
        while next != 0 {
            let path = s.reference(next).unwrap().unwrap();
            let (_, c) = all[path];
            if let Definition::Property {array_dim, flags, kind, ..} = &c.definition {
                let target = match kind {
                    script::PropertyKind::Struct {struct_} => s.reference(*struct_).unwrap().map(str::to_owned),
                    script::PropertyKind::Array {inner} => s.reference(*inner).unwrap().map(|p| format!("{p} {}", all[p].1.class)),
                    script::PropertyKind::Byte {enum_} => s.reference(*enum_).unwrap().map(str::to_owned),
                    _ => None,
                };
                println!("  {path} {} dim={array_dim} flags={flags:#x} {target:?}", c.class);
            }
            next = c.next;
        }
    }
}

#[test]
#[ignore = "local investigation: DUMP_PKG, DUMP_PROP"]
fn dump_tag() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut tree = ClassTree::default();
    for name in PACKAGES { tree.add_package(&Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()); }
    tree.add_editor_classes();
    let p = Package::open(&root.join(std::env::var("DUMP_PKG").unwrap())).unwrap();
    let want = std::env::var("DUMP_PROP").unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && !is_script_definition(&p.object_name(e.class))) {
        let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
        p.read_object_header(e, &tree, &mut r).unwrap();
        for tag in p.read_tags(&mut r).unwrap().into_iter().filter(|t| p.name(t.name) == want) {
            println!("export {i} {} flags={:#x} size={} {:02x?}", p.name(e.object_name), e.object_flags, tag.value.len(), &p.data[tag.value.clone()]);
        }
    }
}

#[test]
#[ignore = "needs the user's installation copy in BL2_INSTALL"]
fn class_defaults() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let packages: Vec<_> = PACKAGES.iter().map(|name| Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()).collect();
    let schemas: Vec<_> = PACKAGES.iter().zip(&packages).map(|(name, p)| Schema::read(name, p).unwrap()).collect();
    let decoder = Decoder::new(&schemas).unwrap();
    let sources: Vec<_> = schemas.iter().zip(&packages).collect();
    let defaults = bl2_formats::defaults::Defaults::read(&sources).unwrap();
    let (mut classes, mut without_cdo, mut effective, mut inherited, mut untyped) = (0, 0, 0, 0, 0);
    for (&class, own) in &defaults.classes {
        classes += 1;
        if own.cdo_path.is_none() { without_cdo += 1; }
        for ((name, _), source) in defaults.effective(class).unwrap() {
            effective += 1;
            if source.source_class != class { inherited += 1; }
            assert_eq!(source.package.name(source.tag.name), name);
            if matches!(decoder.read_tag(source.package, source.source_class, &source.tag, true).unwrap(), Value::Untyped {..}) { untyped += 1; }
        }
    }
    println!("classes={classes} without_cdo={without_cdo} effective_tags={effective} inherited={inherited} untyped={untyped}");
    // orig: WillowGame.WillowPlayerPawn derives from Engine.Pawn; inherited values must come from the nearest definer.
    let pawn = defaults.effective("WillowGame.WillowPlayerPawn").unwrap();
    for ((name, index), source) in &pawn {
        let mut c = Some("WillowGame.WillowPlayerPawn");
        while let Some(cls) = c {
            if defaults.classes[cls].tags.contains_key(&(name.clone(), *index)) { assert_eq!(cls, source.source_class); break; }
            c = defaults.classes[cls].parent;
        }
    }
    // orig: Default__CoverLink.Slots (E-064) serializes transient CoverSlot members; only the class-default rule consumes it.
    let slots = &defaults.effective("Engine.CoverLink").unwrap()[&("Slots".to_owned(), 0)];
    assert!(decoder.read_tag(slots.package, slots.source_class, &slots.tag, true).is_ok());
    assert!(decoder.read_tag(slots.package, slots.source_class, &slots.tag, false).is_err());
    assert!(classes > 3000 && inherited > 0);
}

#[test]
#[ignore = "local investigation: DUMP_PKG, DUMP_NAME"]
fn dump_exports_named() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let p = Package::open(&root.join(std::env::var("DUMP_PKG").unwrap())).unwrap();
    let want = std::env::var("DUMP_NAME").unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.name(e.object_name) == want) {
        println!("export {i} class={} outer={} flags={:#x} size={} export_flags={:#x}", p.object_name(e.class), e.outer, e.object_flags, e.serial_size, e.export_flags);
    }
}

#[test]
#[ignore = "local investigation: DUMP_PKG, DUMP_CLASS; writes the first such export body to DUMP_OUT"]
fn dump_export_of_class() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let p = Package::open(&root.join(std::env::var("DUMP_PKG").unwrap())).unwrap();
    let want = std::env::var("DUMP_CLASS").unwrap();
    let e = p.exports.iter().find(|e| p.object_name(e.class) == want).unwrap();
    println!("{} flags={:#x} offset={} size={}", p.name(e.object_name), e.object_flags, e.serial_offset, e.serial_size);
    std::fs::write(std::env::var("DUMP_OUT").unwrap(), &p.data[e.serial_offset..e.serial_offset + e.serial_size]).unwrap();
}

#[test]
#[ignore = "local investigation: PREFIX lists schema properties under a path prefix"]
fn schema_fields() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let prefix = std::env::var("PREFIX").unwrap();
    for name in PACKAGES {
        let s = Schema::read(name, &Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()).unwrap();
        for (path, f) in s.fields.range(prefix.clone()..).take_while(|(p, _)| p.starts_with(&prefix)) {
            if let Definition::Property {flags, ..} = f.definition { println!("{path} {} flags={flags:#x}", f.class); }
        }
    }
}

#[test]
#[ignore = "local investigation: FUNCS=Core.Object.FRand,... prints iNative"]
fn function_natives() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let want: Vec<String> = std::env::var("FUNCS").unwrap().split(',').map(str::to_owned).collect();
    for name in PACKAGES {
        let s = Schema::read(name, &Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()).unwrap();
        for w in &want { if let Some(Field { definition: Definition::Function {native, flags, ..}, .. }) = s.fields.get(w) { println!("{w} iNative={native} flags={flags:#x}"); } }
    }
}

#[test]
#[ignore = "local investigation: ENUM=Engine.Actor.ETickingGroup prints enum value names"]
fn enum_values() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let want = std::env::var("ENUM").unwrap();
    for name in PACKAGES {
        let p = Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap();
        let s = Schema::read(name, &p).unwrap();
        for (path, f) in s.fields.iter().filter(|(k, _)| k.ends_with(&want)) {
            if let Definition::Enum {names} = &f.definition { println!("{path}: {}", names.iter().map(|n| p.name(*n)).collect::<Vec<_>>().join(", ")); }
        }
    }
}
