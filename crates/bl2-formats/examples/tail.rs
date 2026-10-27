//! Research helper: prints the native bytes after the tagged properties of exports of one class.
//! cargo run -p bl2-formats --example tail -- <install root> <package.upk> <class> [count] [bytes]

use std::path::Path;

use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::ClassTree;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let p = Package::open(args[2].as_ref()).expect("open package");
    let class = &args[3];
    let count: usize = args.get(4).map_or(5, |s| s.parse().unwrap());
    let nbytes: usize = args.get(5).map_or(96, |s| s.parse().unwrap());
    let mut tree = ClassTree::default();
    for name in ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"] {
        tree.add_package(&Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap());
    }
    tree.add_editor_classes();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name(e.class) == *class).take(count) {
        let end = e.serial_offset + e.serial_size;
        let mut r = Reader::new(&p.data[..end], e.serial_offset);
        p.read_object_header(e, &tree, &mut r).unwrap();
        let tags = p.read_tags(&mut r).unwrap();
        let tag_names: Vec<String> = tags.iter().map(|t| p.name(t.name)).collect();
        let tail = &p.data[r.pos..end];
        let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        println!(
            "#{i} {} tail_at={} tail_len={} tags={:?}\n    head: {}\n    end:  {}",
            p.name(e.object_name), r.pos, tail.len(), tag_names,
            hex(&tail[..tail.len().min(nbytes)]), hex(&tail[tail.len().saturating_sub(48)..])
        );
    }
}
