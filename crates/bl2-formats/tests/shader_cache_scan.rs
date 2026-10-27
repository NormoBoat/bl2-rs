//! Shader cache inventory on the user's copy (T-019); prints counts only, writes no derived data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use bl2_formats::package::{Package, Reader};
use bl2_formats::shader_cache::{ShaderCache, d3d9_version};

fn inventory(name: &str, cache: &ShaderCache, data: &[u8]) -> BTreeSet<[u8; 16]> {
    let mut kinds = BTreeMap::<String, usize>::new();
    let mut bytes = 0;
    for s in &cache.shaders {
        let v = d3d9_version(&data[s.code.clone()]).map(|(k, ma, mi)| format!("{}_{ma}_{mi}", if k == 0xfffe { "vs" } else if k == 0xffff { "ps" } else { "?" }));
        *kinds.entry(format!("platform={} frequency={} {}", s.platform, s.frequency, v.as_deref().unwrap_or("not-d3d9"))).or_default() += 1;
        bytes += s.code.len();
    }
    let types: BTreeSet<_> = cache.shaders.iter().map(|s| s.type_name.as_str()).collect();
    println!("{name}: platform={} shaders={} code_bytes={bytes} types_used={} type_crcs={} vf_crcs={} materials={} global_refs={}",
        cache.platform, cache.shaders.len(), types.len(), cache.type_crcs.len(), cache.vertex_factory_crcs.len(), cache.materials.len(), cache.global.len());
    for (k, n) in &kinds { println!("  {k}: {n}"); }
    for s in cache.shaders.iter().filter(|s| d3d9_version(&data[s.code.clone()]).is_none()) {
        println!("  not D3D9 bytecode: {} code={} bytes", s.type_name, s.code.len());
    }
    let ids: BTreeSet<_> = cache.shaders.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), cache.shaders.len(), "duplicate shader ids");
    ids
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn shader_caches() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL")).join("WillowGame/CookedPCConsole");
    let global_data = std::fs::read(root.join("GlobalShaderCache-PC-D3D-SM3.bin")).unwrap();
    let global = ShaderCache::read_global(&global_data).unwrap();
    let ids = inventory("global", &global, &global_data);
    let by_id: BTreeMap<_, _> = global.shaders.iter().map(|s| (s.id, s)).collect();
    for r in &global.global { assert_eq!(by_id[&r.id].type_name, r.type_name); }
    assert_eq!(global.global.iter().map(|r| r.id).collect::<BTreeSet<_>>(), ids, "global map covers every global shader");

    let package = Package::open(&root.join("RefShaderCache-PC-D3D-SM3.upk")).unwrap();
    let cache = ShaderCache::read_package(&package).unwrap();
    let ids = inventory("RefShaderCache", &cache, &package.data);
    let by_id: BTreeMap<_, _> = cache.shaders.iter().map(|s| (s.id, s)).collect();
    let (mut referenced, mut refs, mut vfs) = (BTreeSet::new(), 0, BTreeMap::<&str, usize>::new());
    let mut ids_per_material = BTreeMap::<[u8; 16], usize>::new();
    for m in &cache.materials {
        *ids_per_material.entry(m.material_id).or_default() += 1;
        for r in m.shaders.iter().chain(m.mesh.iter().flat_map(|v| &v.shaders)) {
            assert_eq!(by_id.get(&r.id).map(|s| s.type_name.as_str()), Some(r.type_name.as_str()), "{}", m.friendly_name);
            referenced.insert(r.id);
            refs += 1;
        }
        for v in &m.mesh { *vfs.entry(&v.vertex_factory).or_default() += 1; }
    }
    println!("material maps={} distinct material ids={} shader refs={refs} distinct referenced={} unreferenced={}",
        cache.materials.len(), ids_per_material.len(), referenced.len(), ids.difference(&referenced).count());
    println!("vertex factories: {vfs:?}");
}

/// Every material resource (T-038, E-068) names a cached shader map; static permutations match its key byte for byte.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn materials_link_to_shader_maps() {
    use bl2_formats::level::Terrain;
    use bl2_formats::material::{MaterialResource, StaticParameterSet};
    use bl2_formats::properties::{ClassTree, is_script_definition};
    let install = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let root = install.join("WillowGame/CookedPCConsole");
    let package = Package::open(&root.join("RefShaderCache-PC-D3D-SM3.upk")).unwrap();
    let cache = ShaderCache::read_package(&package).unwrap();
    // Static parameter sets compared with names resolved (each package has its own name table); a value whose
    // override flag is 0 comes from the parent material, so the cache key may hold the parent's value.
    let named = |p: &Package, s: &StaticParameterSet| {
        let v = |o: u32, x: String| if o == 0 { "parent".to_owned() } else { x };
        format!("{:?} {:?} {:?} {:?} {:?}", s.base_material_id,
            s.switches.iter().map(|x| (p.name(x.0), v(x.2, format!("{}", x.1)), x.3)).collect::<Vec<_>>(),
            s.component_masks.iter().map(|x| (p.name(x.0), v(x.2, format!("{:?}", x.1)), x.3)).collect::<Vec<_>>(),
            s.normals.iter().map(|x| (p.name(x.0), v(x.2, format!("{}", x.1)), x.3)).collect::<Vec<_>>(),
            s.terrain_layer_weights.iter().map(|x| (p.name(x.0), v(x.2, format!("{}", x.1)), x.3)).collect::<Vec<_>>())
    };
    let overrides = |s: &StaticParameterSet| s.switches.iter().any(|x| x.2 != 0) || s.component_masks.iter().any(|x| x.2 != 0) || s.normals.iter().any(|x| x.2 != 0) || s.terrain_layer_weights.iter().any(|x| x.2 != 0);
    let mut maps = BTreeMap::<[u8; 16], Vec<String>>::new();
    for m in &cache.materials {
        let mut r = Reader::new(&package.data[..m.static_parameters.end], m.static_parameters.start);
        let set = StaticParameterSet::read(&mut r).unwrap();
        assert_eq!(r.pos, m.static_parameters.end);
        maps.entry(m.material_id).or_default().push(named(&package, &set));
    }
    let all_sets: BTreeSet<&String> = maps.values().flatten().collect();
    let mut tree = ClassTree::default();
    for name in ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"] {
        tree.add_package(&Package::open(&root.join(format!("{name}.upk"))).unwrap());
    }
    tree.add_editor_classes();
    fn files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { files(&p, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk"))
                && !matches!(p.file_name().unwrap().to_str().unwrap(), "Mancana_Textures_Data.upk" | "Mancana_Textures_Meta.upk") { out.push(p) }
        }
    }
    let mut paths = Vec::new(); files(&install, &mut paths);
    let mut stats = BTreeMap::<String, usize>::new();
    let mut found = BTreeSet::<[u8; 16]>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if is_script_definition(&class) || !(tree.is_a(&class, "MaterialInterface") || class == "Terrain") { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            p.read_tags(&mut r).unwrap();
            if r.pos == end { *stats.entry(format!("{class}: no resource (uses its parent's)")).or_default() += 1; continue; }
            let mut ids = Vec::new();
            if class == "Terrain" {
                ids.extend(Terrain::read(&mut r, end).unwrap().unwrap().materials.iter().map(|m| (m.resource.id, None)));
            } else {
                let res = MaterialResource::read(&mut r).unwrap();
                let set = if class.starts_with("MaterialInstance") { let s = StaticParameterSet::read(&mut r).unwrap(); Some((named(&p, &s), overrides(&s))) } else { None };
                assert_eq!(r.pos, end, "{class} resource ends the object");
                ids.push((res.id, set));
            }
            for (id, set) in ids {
                let key = maps.get(&id);
                *stats.entry(format!("{class}: resource {}{}", if key.is_some() { "with cached shader map" } else { "WITHOUT shader map" }, match &set { Some((_, true)) => ", overrides static parameters", Some((_, false)) => ", no static overrides", None => "" })).or_default() += 1;
                if let (Some(keys), Some((set, _))) = (key, &set) {
                    let ok = keys.contains(set);
                    *stats.entry(format!("{class}: static parameters {}", if ok { "equal a key of the id" } else { "MISMATCH" })).or_default() += 1;
                }
                if let Some((set, _)) = &set { *stats.entry(format!("{class}: static parameter set {} key of the shader cache", if all_sets.contains(set) { "is a" } else { "is NOT a" })).or_default() += 1; }
                found.insert(id);
            }
        }
    }
    for (k, n) in &stats { println!("{n} {k}"); }
    println!("cached material ids used by objects: {}/{}", found.iter().filter(|id| maps.contains_key(*id)).count(), maps.len());
    // Established (E-068): materials, decal materials and terrain materials always have a cached map; time-varying
    // instances with a map match its key. Constant instances without a map or with a different key are open (T-038).
    assert!(stats.keys().filter(|k| !k.starts_with("MaterialInstanceConstant")).all(|k| !k.contains("WITHOUT shader map, overrides") && !k.contains("MISMATCH")));
}
