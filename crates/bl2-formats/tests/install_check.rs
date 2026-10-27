//! Installation check on the user's copy (T-033); prints timings for plan/06-assets.md §5.

use std::path::PathBuf;
use std::time::Instant;

use bl2_formats::install::Install;

fn root() -> PathBuf { PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL")) }

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn target_build_opens() {
    let t = Instant::now();
    let install = Install::open(&root()).unwrap();
    println!("open (exe SHA-256 + manifest sizes): {:?}, missing={}", t.elapsed(), install.missing.len());
    let ini = install.resolve("Binaries/Win32", r"..\..\WILLOWGAME\config\defaultengine.ini").expect("resolve");
    assert!(ini.ends_with("WillowGame/Config/DefaultEngine.ini"));
}

#[test]
#[ignore = "hashes the whole installation (~26 GB)"]
fn full_verification() {
    let install = Install::open(&root()).unwrap();
    let t = Instant::now();
    let bad = install.verify_all().unwrap();
    println!("verify_all: {:?}, mismatched={bad:?}", t.elapsed());
    assert!(bad.is_empty());
}

/// Loads a persistent map and every streaming level it names, parsing all tagged properties;
/// run twice to compare a first (possibly cold OS cache) and a warm pass. MAP defaults to Sanctuary_Hole_P.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn map_load_timing() {
    use bl2_formats::package::{Package, Reader};
    use bl2_formats::properties::{ClassTree, is_script_definition};
    let install = Install::open(&root()).unwrap();
    let cooked = "WillowGame/CookedPCConsole";
    let mut tree = ClassTree::default();
    for name in ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"] {
        tree.add_package(&Package::open(&install.resolve(cooked, &format!("{name}.upk")).unwrap()).unwrap());
    }
    tree.add_editor_classes();
    let map = std::env::var("MAP").unwrap_or("Sanctuary_Hole_P".into());
    for pass in ["first", "warm"] {
        let t = Instant::now();
        let mut queue = vec![map.clone()];
        let (mut done, mut bytes, mut tags) = (Vec::new(), 0usize, 0usize);
        while let Some(name) = queue.pop() {
            if done.contains(&name) { continue; }
            let Some(path) = install.resolve(cooked, &format!("{name}.upk")) else { continue };
            let p = Package::open(&path).unwrap();
            bytes += p.data.len();
            for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && !is_script_definition(&p.object_name(e.class))) {
                let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
                p.read_object_header(e, &tree, &mut r).unwrap();
                let list = p.read_tags(&mut r).unwrap();
                tags += list.len();
                if p.object_name(e.class).starts_with("LevelStreaming") {
                    for tag in list.iter().filter(|t| p.name(t.name) == "PackageName") {
                        if let Some(bl2_formats::properties::ScalarValue::Name(n)) = p.read_scalar_tag(tag).unwrap() { queue.push(p.name(n)); }
                    }
                }
            }
            done.push(name);
        }
        println!("{pass}: {map} packages={} uncompressed={} MB tags={tags} time={:?}", done.len(), bytes >> 20, t.elapsed());
    }
}
