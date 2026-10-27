//! Levels in the user's packages (T-037); prints counts only, writes no derived data.

use std::path::{Path, PathBuf};

use bl2_formats::package::{Export, Package, Reader};
use bl2_formats::properties::{ClassTree, is_script_definition};

const PACKAGES: [&str; 9] = ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"];

fn root() -> PathBuf { PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL")) }

fn tree(root: &Path) -> ClassTree {
    let mut tree = ClassTree::default();
    for name in PACKAGES { tree.add_package(&Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()); }
    tree.add_editor_classes();
    tree
}

fn upks(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { upks(&p, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) && !p.to_string_lossy().contains("Mancana_Textures") { out.push(p) }
    }
}

/// Byte range of an export's native data after its tagged properties.
fn tail(p: &Package, e: &Export, tree: &ClassTree) -> std::ops::Range<usize> {
    let end = e.serial_offset + e.serial_size;
    let mut r = Reader::new(&p.data[..end], e.serial_offset);
    p.read_object_header(e, tree, &mut r).unwrap();
    p.read_tags(&mut r).unwrap();
    r.pos..end
}

#[test]
#[ignore = "local investigation: DUMP_PKG, DUMP_CLASS (default Level), DUMP_INDEX, DUMP_OUT"]
fn dump_native_tail() {
    let root = root();
    let tree = tree(&root);
    let p = Package::open(&root.join(std::env::var("DUMP_PKG").unwrap())).unwrap();
    let class = std::env::var("DUMP_CLASS").unwrap_or("Level".into());
    let only: Option<usize> = std::env::var("DUMP_INDEX").ok().map(|s| s.parse().unwrap());
    for (_, e) in p.exports.iter().enumerate().filter(|(i, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == class && only.is_none_or(|o| o == *i)).take(2) {
        let t = tail(&p, e, &tree);
        println!("{} tail {} bytes at {}", p.name(e.object_name), t.len(), t.start);
        if let Ok(out) = std::env::var("DUMP_OUT") { std::fs::write(out, &p.data[t.clone()]).unwrap(); }
        for (i, chunk) in p.data[t.start..t.end.min(t.start + 512)].chunks(16).enumerate() { println!("{:04x}: {}", i * 16, chunk.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")); }
    }
    println!("imports={} exports={}", p.imports.len(), p.exports.len());
    let _ = is_script_definition;
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn all_levels() {
    use bl2_formats::level::Level;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let (mut levels, mut actors, mut rest_sizes, mut heads) = (0, 0, BTreeMap::<usize, usize>::new(), BTreeMap::<String, usize>::new());
    let class_of = |p: &Package, i: i32| if i == 0 { "null".to_owned() } else { p.object_name(if i > 0 { p.exports[i as usize - 1].class } else { 0 }) };
    let mut ref_classes = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && p.object_name(e.class) == "Level") {
            let t = tail(&p, e, &tree);
            let mut r = Reader::new(&p.data[..t.end], t.start);
            let l = Level::read(&mut r, t.end).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            levels += 1;
            assert_eq!(l.owner, i as i32 + 1, "owner is the level itself");
            for &a in &l.actors { assert!(a >= 0 && (a as usize) <= p.exports.len(), "actor ref {a}"); }
            actors += l.actors.iter().filter(|&&a| a != 0).count();
            *ref_classes.entry(format!("model {}", class_of(&p, l.model))).or_default() += 1;
            for &c in &l.model_components { *ref_classes.entry(format!("modelcomponent {}", class_of(&p, c))).or_default() += 1; }
            for &s in &l.game_sequences { *ref_classes.entry(format!("sequence {}", class_of(&p, s))).or_default() += 1; }
            *rest_sizes.entry(l.trailer.len()).or_default() += 1;
            if p.data[l.trailer.clone()].iter().any(|&b| b != 0) { *heads.entry(format!("nonzero trailer {}", path.file_name().unwrap().to_string_lossy())).or_default() += 1; }
            for &a in l.gbx_actors.iter().filter(|&&a| a != 0) { *ref_classes.entry(format!("gbx actor {}", if tree.is_a(&class_of(&p, a), "Actor") { "Actor" } else { "other" })).or_default() += 1; }
        }
    }
    println!("levels={levels} actors={actors}");
    println!("{ref_classes:?}");
    println!("trailer sizes: {rest_sizes:?}");
    println!("{heads:?}");
}


#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn all_models() {
    use bl2_formats::model::Model;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let (mut models, mut surfs, mut nodes, mut errors) = (0, 0, 0, BTreeMap::<String, (usize, String)>::new());
    for path in &paths {
        let p = Package::open(path).unwrap();
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && p.object_name(e.class) == "Model") {
            let t = tail(&p, e, &tree);
            let mut r = Reader::new(&p.data[..t.end], t.start);
            match Model::read(&mut r, t.end, i as i32 + 1) {
                Ok(m) => {
                    models += 1; surfs += m.surfs.len(); nodes += m.nodes.len() / 64;
                    let polys = if m.polys > 0 { p.object_name(p.exports[m.polys as usize - 1].class) } else { m.polys.to_string() };
                    assert!(m.polys == 0 || polys == "Polys", "{polys}");
                }
                Err(err) => { let v = errors.entry(err.to_string()).or_insert((0, path.display().to_string())); v.0 += 1; }
            }
        }
    }
    println!("models={models} surfs={surfs} nodes={nodes} errors={errors:#?}");
    assert!(errors.is_empty());
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn all_worlds_and_polys() {
    use bl2_formats::{level::World, model::read_polys};
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let (mut worlds, mut polys, mut polygons) = (0, 0, 0);
    for path in &paths {
        let p = Package::open(path).unwrap();
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if class != "World" && class != "Polys" { continue; }
            let t = tail(&p, e, &tree);
            let mut r = Reader::new(&p.data[..t.end], t.start);
            if class == "World" {
                let w = World::read(&mut r, t.end).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                assert_eq!(p.object_name(p.exports[w.persistent_level as usize - 1].class), "Level");
                worlds += 1;
            } else {
                polygons += read_polys(&mut r, t.end, i as i32 + 1).unwrap_or_else(|err| panic!("{}: {err}", path.display())).len();
                polys += 1;
            }
        }
    }
    println!("worlds={worlds} polys objects={polys} polygons={polygons}");
}

/// Level.Actors against the package's actor exports, positions, and streaming-level references.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn actors_and_streaming() {
    use bl2_formats::level::Level;
    use bl2_formats::properties::{ObjectReference, ScalarValue};
    use std::collections::{BTreeMap, BTreeSet};
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let names: BTreeSet<String> = paths.iter().map(|p| p.file_stem().unwrap().to_string_lossy().to_lowercase()).collect();
    let mut stats = BTreeMap::<String, usize>::new();
    let mut streamed = BTreeSet::<String>::new();
    let mut with_world = BTreeSet::<String>::new();
    let mut missing_streaming = BTreeSet::<String>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().to_lowercase();
        let tags = |e: &Export| { let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset); p.read_object_header(e, &tree, &mut r).unwrap(); p.read_tags(&mut r).unwrap() };
        for (li, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && p.object_name(e.class) == "Level") {
            with_world.insert(stem.clone());
            let t = tail(&p, e, &tree);
            let l = Level::read(&mut Reader::new(&p.data[..t.end], t.start), t.end).unwrap();
            let listed: BTreeSet<i32> = l.actors.iter().copied().filter(|&a| a != 0).collect();
            *stats.entry("listed actors".into()).or_default() += listed.len();
            if listed.len() != l.actors.iter().filter(|&&a| a != 0).count() { *stats.entry("duplicate entries".into()).or_default() += 1; }
            // Actors are exports of an Actor class directly inside this level, excluding templates.
            let owned: BTreeSet<i32> = p.exports.iter().enumerate().filter(|(_, x)| x.outer == li as i32 + 1 && x.class != 0
                && tree.is_a(&p.object_name(x.class), "Actor") && x.object_flags & 0x600 == 0).map(|(i, _)| i as i32 + 1).collect();
            *stats.entry("listed but not an owned actor".into()).or_default() += listed.difference(&owned).count();
            for &a in owned.difference(&listed) { let x = &p.exports[a as usize - 1]; *stats.entry(format!("unlisted {} flags {:#x}", p.object_name(x.class), x.object_flags & 0xffff_ffff)).or_default() += 1; }
            for &a in &listed {
                let x = &p.exports[a as usize - 1];
                let has_location = tags(x).iter().any(|t| p.name(t.name) == "Location");
                let loc = bl2_formats::level::actor_location(&p, &tree, a as usize - 1).unwrap();
                *stats.entry(format!("location {} {}", if has_location { "tagged" } else { "via archetype" }, if loc.is_some() { "found" } else { "class default" })).or_default() += 1;
                if loc.is_some_and(|v| v.iter().any(|c| !c.is_finite())) { *stats.entry(format!("non-finite location {} {:?} in {}", p.object_name(x.class), loc, stem)).or_default() += 1; }
                if loc.is_none() { *stats.entry(format!("default location {}", p.object_name(x.class))).or_default() += 1; }
            }
            // WorldInfo (first actor) streaming levels: LevelStreaming objects with PackageName.
            let infos: Vec<&Export> = l.actors.iter().filter(|&&a| a > 0).map(|&a| &p.exports[a as usize - 1]).filter(|x| tree.is_a(&p.object_name(x.class), "WorldInfo")).collect();
            *stats.entry(format!("worldinfo count {} first slot {}", infos.len(), if l.actors[0] == 0 { "null" } else if infos.first().is_some_and(|i| std::ptr::eq(*i, &p.exports[l.actors[0] as usize - 1])) { "worldinfo" } else { "other" })).or_default() += 1;
            let Some(info) = infos.first() else { continue };
            for tag in tags(info).iter().filter(|t| p.name(t.name) == "StreamingLevels") {
                let mut r = Reader::new(&p.data[..tag.value.end], tag.value.start);
                for _ in 0..r.count().unwrap() {
                    let Some(i) = Some(r.i32().unwrap()).filter(|&i| i > 0) else { continue };
                    for st in tags(&p.exports[i as usize - 1]).iter().filter(|t| p.name(t.name) == "PackageName") {
                        if let Some(ScalarValue::Name(n)) = p.read_scalar_tag(st).unwrap() {
                            let n = p.name(n).to_lowercase();
                            if !names.contains(&n) { missing_streaming.insert(format!("{stem} -> {n}")); }
                            streamed.insert(n);
                        }
                    }
                }
            }
            let _ = ObjectReference::Null;
        }
    }
    let persistent: Vec<_> = with_world.iter().filter(|w| !streamed.contains(*w)).collect();
    println!("{stats:#?}");
    println!("packages with a level={} streamed={} persistent (never streamed)={}", with_world.len(), streamed.len(), persistent.len());
    println!("persistent: {persistent:?}");
    println!("missing streaming packages: {missing_streaming:?}");
}

/// Collection actors store one LocalToWorld matrix per component of their component array (UE3 cooker merge).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn collection_actors() {
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if !class.ends_with("CollectionActor") { continue; }
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let tags = p.read_tags(&mut r).unwrap();
            let counts: Vec<String> = tags.iter().filter(|t| p.name(t.type_name) == "ArrayProperty").map(|t| {
                let n = i32::from_le_bytes(p.data[t.value.start..t.value.start + 4].try_into().unwrap());
                format!("{}={n}", p.name(t.name))
            }).collect();
            let comps: usize = tags.iter().filter(|t| p.name(t.type_name) == "ArrayProperty" && p.name(t.name).ends_with("Components"))
                .map(|t| i32::from_le_bytes(p.data[t.value.start..t.value.start + 4].try_into().unwrap()) as usize).sum();
            let tail = e.serial_offset + e.serial_size - r.pos;
            let entries = bl2_formats::level::read_collection(&mut r, e.serial_offset + e.serial_size, comps, class != "StaticLightCollectionActor").unwrap();
            assert!(entries.iter().all(|(m, _)| m[15] == 1.0 && m[3] == 0.0 && m[7] == 0.0 && m[11] == 0.0), "affine matrices");
            *stats.entry(format!("{class} arrays={:?} bytes per component: {}", counts.iter().map(|c| c.split('=').next().unwrap().to_owned()).collect::<Vec<_>>(), if comps == 0 { format!("no components, tail {tail}") } else if tail.is_multiple_of(comps) { (tail / comps).to_string() } else { format!("not divisible ({tail}/{comps})") })).or_default() += 1;
        }
    }
    for (k, n) in &stats { println!("{n} {k}"); }
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn model_and_brush_components() {
    use bl2_formats::model::{LightMap, read_model_component};
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if class != "ModelComponent" && class != "BrushComponent" { continue; }
            let t = tail(&p, e, &tree);
            let mut r = Reader::new(&p.data[..t.end], t.start);
            if class == "BrushComponent" {
                // CachedPhysBrushData: TArray<FKCachedConvexDataElement> of bulk bytes.
                let n = bl2_formats::model::read_brush_component(&mut r, t.end).unwrap_or_else(|err| panic!("{} {i}: {err}", path.display())).len();
                *stats.entry(format!("brush convex elements={}", n.min(2))).or_default() += 1;
                continue;
            }
            if t.is_empty() { *stats.entry(format!("model component without native data (flags {:#x})", e.object_flags & 0xffff_ffff)).or_default() += 1; continue; }
            let mc = read_model_component(&mut r, t.end).unwrap_or_else(|err| panic!("{} {i}: {err}", path.display()));
            assert_eq!(p.object_name(p.exports[mc.model as usize - 1].class), "Model");
            for el in &mc.elements {
                *stats.entry(format!("lightmap {}", match el.light_map { LightMap::None => "none", LightMap::Vertex {..} => "vertex", LightMap::Texture {..} => "texture" })).or_default() += 1;
                assert_eq!(el.component, i as i32 + 1, "element component is the model component");
            }
            *stats.entry("model components".into()).or_default() += 1;
        }
    }
    println!("{stats:#?}");
}

/// StaticMeshComponent (and subclasses) LOD info: shadow maps, light maps, override vertex colors.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn static_mesh_components() {
    use bl2_formats::model::{LightMap, read_static_mesh_component};
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let class_of = |i: i32| if i > 0 { p.object_name(p.exports[i as usize - 1].class) } else if i < 0 { format!("import {}", p.object_name(i)) } else { "null".into() };
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if !tree.is_a(&class, "StaticMeshComponent") { continue; }
            let t = tail(&p, e, &tree);
            let lods = read_static_mesh_component(&mut Reader::new(&p.data[..t.end], t.start), t.end)
                .unwrap_or_else(|err| panic!("{} {i}: {err}", path.display()));
            *stats.entry(format!("{class} lods={}", lods.len().min(3))).or_default() += 1;
            for lod in &lods {
                for &s in &lod.shadow_maps { *stats.entry(format!("shadow map {}", class_of(s))).or_default() += 1; }
                for &s in &lod.shadow_vertex_buffers { *stats.entry(format!("shadow vertex buffer {}", class_of(s))).or_default() += 1; }
                match &lod.light_map {
                    LightMap::None => *stats.entry("lightmap none".into()).or_default() += 1,
                    LightMap::Texture { textures, .. } => {
                        *stats.entry("lightmap texture".into()).or_default() += 1;
                        for (tx, _) in textures { *stats.entry(format!("lightmap texture ref {}", class_of(*tx))).or_default() += 1; }
                    }
                    LightMap::Vertex { owner, directional, simple, .. } => {
                        *stats.entry("lightmap vertex".into()).or_default() += 1;
                        *stats.entry(format!("vertex owner {}", if *owner == i as i32 + 1 { "self".into() } else { format!("{} outer={}", class_of(*owner), *owner == e.outer) })).or_default() += 1;
                        for (kind, d) in [("directional", directional), ("simple", simple)] {
                            assert_eq!(d.inline.len() as i64, d.size_on_disk, "inline payload");
                            assert!(d.element_count == 0 || d.offset_in_file as usize == d.inline.start, "offset is own position");
                            let per = (d.size_on_disk as usize).checked_div(d.element_count).unwrap_or(0);
                            assert!(d.element_count == 0 || per * d.element_count == d.size_on_disk as usize);
                            *stats.entry(format!("vertex {kind} flags={:#x} bytes/sample={per}", d.flags)).or_default() += 1;
                        }
                    }
                }
                if let Some((stride, count, data)) = &lod.vertex_colors {
                    assert_eq!(data.len(), *stride as usize * *count as usize);
                    *stats.entry(format!("override vertex colors stride={stride}")).or_default() += 1;
                }
            }
        }
    }
    println!("{stats:#?}");
}



#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn decal_components() {
    use bl2_formats::model::{LightMap, read_decal_component};
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let class_of = |i: i32| if i > 0 { p.object_name(p.exports[i as usize - 1].class) } else if i < 0 { format!("import {}", p.object_name(i)) } else { "null".into() };
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "DecalComponent") {
            let t = tail(&p, e, &tree);
            let receivers = read_decal_component(&mut Reader::new(&p.data[..t.end], t.start), t.end)
                .unwrap_or_else(|err| panic!("{} {i}: {err}", path.display()));
            *stats.entry("decal components".into()).or_default() += 1;
            for d in &receivers {
                *stats.entry(format!("receiver {}", class_of(d.component))).or_default() += 1;
                if d.indices.len() / 2 != d.num_triangles as usize * 3 { *stats.entry(format!("no baked geometry ({} indices, {} vertices), {} triangles: {}", d.indices.len() / 2, d.vertices.len() / 28, if d.num_triangles > 0 { "some" } else { "zero" }, class_of(d.component))).or_default() += 1; }
                // Terrain receivers keep only the triangle count; their decal geometry is not baked.
                assert!(d.indices.len() / 2 == d.num_triangles as usize * 3 || (d.indices.is_empty() && d.vertices.is_empty() && class_of(d.component) == "TerrainComponent"));
                let verts = d.vertices.len() / 28;
                assert!(p.data[d.indices.clone()].chunks(2).all(|c| (u16::from_le_bytes([c[0], c[1]]) as usize) < verts), "indices in range");
                *stats.entry(format!("lightmap {}", match d.light_map { LightMap::None => "none", LightMap::Vertex { .. } => "vertex", LightMap::Texture { .. } => "texture" })).or_default() += 1;
                for &s in &d.shadow_maps { *stats.entry(format!("shadow map {}", class_of(s))).or_default() += 1; }
                *stats.entry(format!("unknown {:?}", [d.unknown[0].clamp(-2, 8), d.unknown[1].clamp(-2, 8)])).or_default() += 1;
            }
        }
    }
    println!("{stats:#?}");
}

#[test]
#[ignore = "local investigation: DUMP_PKG, DUMP_CLASS prints the tags of the first two exports of the class"]
fn tag_list() {
    let root = root();
    let tree = tree(&root);
    let p = Package::open(&root.join(std::env::var("DUMP_PKG").unwrap())).unwrap();
    let class = std::env::var("DUMP_CLASS").unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == class).take(2) {
        let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
        p.read_object_header(e, &tree, &mut r).unwrap();
        println!("export {} {} index {}", i + 1, p.name(e.object_name), i + 1);
        for t in p.read_tags(&mut r).unwrap() {
            println!("  {}[{}] {} size={} {:?} {:02x?}", p.name(t.name), t.array_index, p.name(t.type_name), t.value.len(), p.read_scalar_tag(&t).ok().flatten(), &p.data[t.value.start..t.value.end.min(t.value.start + 24)]);
        }
    }
}



/// WiringMesh (Gearbox, no script definition) stores StaticMesh native data followed by its own fields.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn wiring_meshes() {
    use bl2_formats::static_mesh::StaticMeshData;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let mut shown = 0;
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "WiringMesh") {
            let t = tail(&p, e, &tree);
            let mut r = Reader::new(&p.data[..t.end], t.start);
            let m = StaticMeshData::read(&mut r).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let rest = t.end - r.pos;
            *stats.entry(format!("lods={} rest={}", m.lod_count, rest.min(64))).or_default() += 1;
            if rest > 0 && shown < 3 { shown += 1; println!("REST {rest}: {}", p.data[r.pos..t.end.min(r.pos + 96)].iter().map(|b| format!("{b:02x}")).collect::<String>()); }
        }
    }
    println!("{stats:#?}");
}

#[test]
#[ignore = "local investigation: DUMP_CLASS, DUMP_OUT (DUMP_MIN) writes the smallest native tail of the class"]
fn smallest_tail() {
    let root = root();
    let tree = tree(&root);
    let class = std::env::var("DUMP_CLASS").unwrap();
    let min: usize = std::env::var("DUMP_MIN").ok().map(|s| s.parse().unwrap()).unwrap_or(1);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut best: Option<(usize, String, Vec<u8>)> = None;
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == class) {
            let t = tail(&p, e, &tree);
            if t.len() >= min && best.as_ref().is_none_or(|b| t.len() < b.0) { best = Some((t.len(), format!("{} {}", path.display(), p.name(e.object_name)), p.data[t].to_vec())); }
        }
    }
    let (n, at, bytes) = best.unwrap();
    println!("smallest {n} at {at}");
    std::fs::write(std::env::var("DUMP_OUT").unwrap(), bytes).unwrap();
}


/// Terrain heights, layer alpha maps, weight maps and cached terrain materials against the terrain's own properties.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn terrains() {
    use bl2_formats::level::Terrain;
    use bl2_formats::properties::ScalarValue;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let class_of = |i: i32| if i > 0 { p.object_name(p.exports[i as usize - 1].class) } else if i < 0 { "import".into() } else { "null".into() };
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "Terrain") {
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let mut ints = BTreeMap::new();
            for t in p.read_tags(&mut r).unwrap() { if let Ok(Some(ScalarValue::Int(v))) = p.read_scalar_tag(&t) { ints.insert(p.name(t.name), v as usize); } }
            let Some(t) = Terrain::read(&mut r, end).unwrap_or_else(|err| panic!("{} {i}: {err}", path.display())) else { *stats.entry("class default".into()).or_default() += 1; continue };
            let int = |k: &str, d: usize| ints.get(k).copied().unwrap_or(d);
            // orig: Engine.Terrain defaults NumPatchesX/Y, MaxTesselationLevel = 4/4/4? only checked when serialized.
            let (px, py) = (int("NumPatchesX", 0), int("NumPatchesY", 0));
            assert_eq!(t.heights.len() / 2, (px + 1) * (py + 1), "one height per vertex");
            assert_eq!(t.info_data.len(), (px + 1) * (py + 1), "one info byte per vertex");
            let w = int("WeightmapTesselationLevel", 1);
            for a in &t.alpha_maps { assert!(a.is_empty() || a.len() == (px * w + 1) * (py * w + 1), "alpha map size"); }
            for (d, sx, sy, owner, tex) in &t.weight_maps {
                assert_eq!(d.len(), (*sx * *sy) as usize);
                assert_eq!(*owner, i as i32 + 1);
                *stats.entry(format!("weight map texture {}", class_of(*tex))).or_default() += 1;
            }
            for &x in &t.weighted_texture_maps { *stats.entry(format!("weighted texture {}", class_of(x))).or_default() += 1; }
            for m in &t.materials {
                assert_eq!(m.terrain, i as i32 + 1);
                *stats.entry(format!("material ids={} unknown={} records={}", m.material_ids.len().min(4), m.unknown, m.unknown_records.len())).or_default() += 1;
            }
            *stats.entry("terrains".into()).or_default() += 1;
        }
    }
    println!("{stats:#?}");
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn nav_meshes() {
    use bl2_formats::level::NavMesh;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let class_of = |i: i32| if i > 0 { p.object_name(p.exports[i as usize - 1].class) } else if i < 0 { "import".into() } else { "null".into() };
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "GBXNavMesh") {
            let t = tail(&p, e, &tree);
            if t.is_empty() { *stats.entry("class default".into()).or_default() += 1; continue; }
            let m = NavMesh::read(&mut Reader::new(&p.data[..t.end], t.start), t.end).unwrap_or_else(|err| panic!("{} {i}: {err}", path.display()));
            *stats.entry("nav meshes".into()).or_default() += 1;
            *stats.entry("polys".into()).or_default() += m.polys.len();
            for (v, n, _) in &m.polys {
                assert!(v.iter().all(|&x| (x as usize) < m.vertices.len()), "poly vertices in range");
                assert!(n.iter().all(|&x| x == 0xffff || (x as usize) < m.polys.len()), "neighbours in range");
            }
            for d in &m.poly_data {
                for sm in &d.special_moves { *stats.entry(format!("special move mover {}", class_of(sm.mover))).or_default() += 1; }
                *stats.entry("cross-level connections".into()).or_default() += d.cross_level_connections.len();
            }
            for x in &m.polys { *stats.entry(format!("PolyDataIdx {}", if x.2 == 0xffff { "none" } else if (x.2 as usize) < m.poly_data.len() { "in range" } else { "OUT OF RANGE" })).or_default() += 1; }
            for (o, _) in &m.obstacles { *stats.entry(format!("obstacle {}", class_of(*o))).or_default() += 1; }
            for &o in &m.special_movers { *stats.entry(format!("special mover {}", class_of(o))).or_default() += 1; }
            *stats.entry(format!("mesh base {}", class_of(m.mesh_base))).or_default() += 1;
        }
    }
    println!("{stats:#?}");
    assert!(!stats.keys().any(|k| k.contains("OUT OF RANGE")));
}


/// Cross-level object references (GBXCrossLevelReferenceContainer.CrossLevelObjectRef) through the packages' GUID tables.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn cross_level_references() {
    use std::collections::{BTreeMap, HashMap};
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let by_stem: HashMap<String, &PathBuf> = paths.iter().map(|p| (p.file_stem().unwrap().to_string_lossy().to_lowercase(), p)).collect();
    let mut export_guids = HashMap::<String, HashMap<[u8; 16], (i32, String)>>::new();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        let guids = p.level_guids().unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        if !guids.exports.is_empty() { *stats.entry("packages with export GUIDs".into()).or_default() += 1; }
        if !guids.imports.is_empty() { *stats.entry("packages with import GUIDs".into()).or_default() += 1; }
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "GBXCrossLevelReferenceContainer") {
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            for t in p.read_tags(&mut r).unwrap().iter().filter(|t| p.name(t.name) == "CrossLevelObjectRef") {
                let v = u32::from_le_bytes(p.data[t.value.start..t.value.start + 4].try_into().unwrap());
                *stats.entry("references".into()).or_default() += 1;
                if v == 0 { *stats.entry("null reference".into()).or_default() += 1; continue; }
                if v >> 28 != 0xf { let i = v as i32; *stats.entry(format!("ordinary reference to {}", if i > 0 && (i as usize) <= p.exports.len() { p.object_name(p.exports[i as usize - 1].class) } else { format!("raw {v:#x}") })).or_default() += 1; continue; }
                let (name, guid) = guids.import_guid(v).expect("reference inside the import GUID tables");
                let target = name.to_lowercase();
                let map = export_guids.entry(target.clone()).or_insert_with(|| {
                    let Some(tp) = by_stem.get(&target) else { return HashMap::new() };
                    let tp = Package::open(tp).unwrap();
                    tp.level_guids().unwrap().exports.into_iter().map(|(g, i)| (g, (i, if i > 0 { tp.object_name(tp.exports[i as usize - 1].class) } else { "not an export".into() }))).collect()
                });
                match map.get(&guid) {
                    Some((_, class)) => *stats.entry(format!("resolved to {class}")).or_default() += 1,
                    None => *stats.entry(format!("UNRESOLVED (target package {})", if by_stem.contains_key(&target) { "present" } else { "missing" })).or_default() += 1,
                }
            }
        }
    }
    println!("{stats:#?}");
    assert!(!stats.keys().any(|k| k.starts_with("UNRESOLVED")));
}


#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn terrain_components() {
    use bl2_formats::level::TerrainComponent;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| e.serial_size > 0 && e.class != 0 && p.object_name(e.class) == "TerrainComponent") {
            let t = tail(&p, e, &tree);
            let Some(c) = TerrainComponent::read(&mut Reader::new(&p.data[..t.end], t.start), t.end).unwrap_or_else(|err| panic!("{} {i}: {err}", path.display())) else {
                *stats.entry("class default".into()).or_default() += 1; continue
            };
            *stats.entry("terrain components".into()).or_default() += 1;
            assert!(c.bv_tree.iter().all(|n| n.bounds.valid == 1 && n.is_leaf <= 1), "valid boxes, boolean leaf flag");
            if let Some(d) = &c.platform_data {
                assert_eq!(d.owner, i as i32 + 1, "platform data owner is the component");
                let verts = d.vertices.len() / 8;
                for b in &d.batches { assert!(p.data[b.indices.clone()].chunks(2).all(|c| (u16::from_le_bytes([c[0], c[1]]) as usize) < verts), "batch indices in range"); }
                *stats.entry(format!("platform data batches={}", d.batches.len().min(9))).or_default() += 1;
            } else { *stats.entry("no platform data".into()).or_default() += 1; }
        }
    }
    println!("{stats:#?}");
}

/// Shadow-map samples, and level classes whose native data is a fixed zero-filled field (E-067).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn shadow_maps_and_zero_tails() {
    use bl2_formats::model::read_shadow_map_1d;
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    // Bytes after the tags of non-default objects; all zero in the installation.
    let zero: BTreeMap<&str, usize> = [("Pylon", 8), ("DynamicPylon", 8), ("SeqAct_Interp", 4), ("WillowSeqAct_DayNightCycle", 4), ("WillowSeqAct_InterpMenu", 4), ("FluidSurfaceComponent", 4)].into();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if class == "ShadowMap1D" {
                let t = tail(&p, e, &tree);
                let (samples, _) = read_shadow_map_1d(&mut Reader::new(&p.data[..t.end], t.start), t.end).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                for s in samples { *stats.entry(format!("ShadowMap1D sample {}", if !s.is_finite() { "non-finite" } else if s < 0.0 { "< 0" } else if s > 1.0 { "> 1" } else { "in [0, 1]" })).or_default() += 1; }
                *stats.entry(class).or_default() += 1;
            } else if let Some(&n) = zero.get(class.as_str()) {
                let t = tail(&p, e, &tree);
                let cdo = e.object_flags & 0x200 != 0;
                assert!(if cdo { t.is_empty() } else { t.len() == n && p.data[t].iter().all(|&b| b == 0) }, "{class} tail");
                *stats.entry(class).or_default() += 1;
            }
        }
    }
    println!("{stats:#?}");
}


/// FaceFX data (T-021a): actor archives in FaceFXAsset, empty anim-set bytes, and the objects that play FaceFX.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn facefx_objects() {
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let (mut sizes, mut classes) = (Vec::new(), BTreeMap::<String, usize>::new());
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            let users = ["SeqAct_PlayFaceFXAnim", "AnimNotify_PlayFaceFXAnim", "SoundCue", "SkeletalMesh"];
            if !class.starts_with("FaceFX") && !users.contains(&class.as_str()) { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let tags = p.read_tags(&mut r).unwrap();
            if users.contains(&class.as_str()) {
                let names: Vec<String> = tags.iter().map(|t| p.name(t.name)).filter(|n| n.contains("FaceFX")).collect();
                if !names.is_empty() { *stats.entry(format!("{class} with {names:?}")).or_default() += 1; }
                continue;
            }
            if e.object_flags & 0x200 != 0 { continue; }
            // Native data: TArray<u8> raw FaceFX bytes, then a second TArray<u8> (session / mini session bytes).
            let a = r.count().unwrap(); let blob = r.bytes(a).unwrap();
            let b = r.count().unwrap(); r.bytes(b).unwrap();
            assert_eq!(r.pos, end, "{class}: two byte arrays end the object");
            *stats.entry(format!("{class}: first array {}, second {}", if a == 0 { "empty" } else { "FaceFX archive" }, if b == 0 { "empty" } else { "data" })).or_default() += 1;
            if a > 0 {
                assert_eq!(&blob[..4], b"FACE");
                *stats.entry(format!("archive version {}", u32::from_le_bytes(blob[4..8].try_into().unwrap()))).or_default() += 1;
                sizes.push(a);
                // Class names in the archive's class table (length-prefixed "Fx…" strings).
                for i in 0..blob.len().saturating_sub(6) {
                    let n = u32::from_le_bytes(blob[i..i + 4].try_into().unwrap()) as usize;
                    if (4..40).contains(&n) && i + 4 + n <= blob.len() && blob[i + 4..i + 6] == *b"Fx" && blob[i + 4..i + 4 + n].iter().all(|c| c.is_ascii_alphanumeric()) {
                        *classes.entry(String::from_utf8_lossy(&blob[i + 4..i + 4 + n]).into_owned()).or_default() += 1;
                    }
                }
            }
        }
    }
    sizes.sort();
    for (k, c) in &stats { println!("{c} {k}"); }
    println!("archive sizes: min {:?} median {:?} max {:?} total {}", sizes.first(), sizes.get(sizes.len() / 2), sizes.last(), sizes.iter().sum::<usize>());
    println!("FaceFX classes named in archives (archives naming each): {classes:?}");
}

/// Objects of physics-related classes in the packages (T-022, E-033).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn physics_objects() {
    use std::collections::BTreeMap;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let kinds = ["KActor", "KAsset", "RB_", "PhysicsAsset", "Apex", "Vehicle", "Fluid", "Cloth", "PhysX", "Ragdoll", "Constraint", "Pickup", "Pizazz", "RigidBody"];
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.class != 0) {
            let class = p.object_name(e.class);
            if e.object_flags & 0x200 != 0 || is_script_definition(&class) { continue; }
            if kinds.iter().any(|k| class.contains(k)) { *stats.entry(class.clone()).or_default() += 1; }
            if class == "SkeletalMesh" && e.serial_size > 0 {
                let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
                p.read_object_header(e, &tree, &mut r).unwrap();
                for t in p.read_tags(&mut r).unwrap() {
                    let n = p.name(t.name);
                    if n.contains("Cloth") || n.contains("Apex") || n == "PhysicsAsset" { *stats.entry(format!("SkeletalMesh tag {n}")).or_default() += 1; }
                }
            }
        }
    }
    for (k, c) in &stats { println!("{c} {k}"); }
}

/// FaceFX archives of `FaceFXAsset`/`FaceFXAnimSet` (T-021b): header and tables; FACEFX_OUT_DIR gets the smallest
/// archive of each class.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn facefx_archives() {
    use bl2_formats::facefx::Archive;
    use std::collections::{BTreeMap, HashSet};
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let (mut stats, mut seen, mut smallest) = (BTreeMap::<String, usize>::new(), HashSet::new(), BTreeMap::<String, Vec<u8>>::new());
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            let class = p.object_name(e.class);
            if !class.starts_with("FaceFX") { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            p.read_tags(&mut r).unwrap();
            let a = r.count().unwrap(); let blob = r.bytes(a).unwrap();
            if a == 0 || !seen.insert(blob.to_vec()) { continue; }
            let ar = Archive::read(blob).unwrap_or_else(|err| panic!("{} {}: {err}", path.display(), p.name(e.object_name)));
            *stats.entry(format!("{class} classes {:?}", ar.classes.iter().map(|c| format!("{}:{}", c.name, c.version)).collect::<Vec<_>>())).or_default() += 1;
            *stats.entry(format!("{class} header {} {}", ar.licensee_version, ar.flags)).or_default() += 1;
            let actor = ar.actor().unwrap_or_else(|err| { std::fs::write(std::env::var("FACEFX_OUT").unwrap(), blob).unwrap(); panic!("{} {}: {err}", path.display(), p.name(e.object_name)) });
            println!("{:28} bones {:3} nodes {:4} inputs {:5} props {:3} tables {:2} groups {:?} phonemes {:3}/{:2}", actor.name, actor.bones.len(), actor.nodes.len(),
                actor.nodes.iter().map(|n| n.inputs.len()).sum::<usize>(), actor.nodes.iter().map(|n| n.properties.len()).sum::<usize>(), actor.tables.len(),
                actor.groups.iter().map(|g| (g.name.as_str(), g.anims.len())).collect::<Vec<_>>(), actor.phonemes.len(), actor.phoneme_targets.len());
            for n in &actor.nodes {
                *stats.entry(format!("node kind {}", n.kind)).or_default() += 1;
                *stats.entry(format!("node input op {}", n.input_op)).or_default() += 1;
                let recip = |a: f32, b: f32| if a == 0.0 { b == 0.0 } else { (a * b - 1.0).abs() < 1e-5 };
                *stats.entry(format!("node values[1] = 1/values[0] {}, values[3] = 1/values[2] {}", recip(n.values[0], n.values[1]), recip(n.values[2], n.values[3]))).or_default() += 1;
                if !recip(n.values[0], n.values[1]) { *stats.entry(format!("node values {:?}", n.values)).or_default() += 1; }
                for l in &n.inputs { *stats.entry(format!("link fn {} params {}", l.function, l.params.len())).or_default() += 1; }
            }
            if smallest.get(&class).is_none_or(|b| blob.len() < b.len()) { smallest.insert(class.clone(), blob.to_vec()); }
        }
    }
    for (k, c) in &stats { println!("{c:5} {k}"); }
    if let Ok(dir) = std::env::var("FACEFX_OUT_DIR") { for (c, b) in &smallest { std::fs::write(format!("{dir}/{c}.bin"), b).unwrap(); } }
}
