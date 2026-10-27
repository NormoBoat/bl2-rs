//! Research helper: prints a function's bytecode as hex plus decompiled statements.
//! cargo run -p bl2-formats --example bytecode_dump -- <install root> <package.upk> <function name> [occurrence]

use std::path::Path;

use bl2_formats::bytecode::Walker;
use bl2_formats::decompile::{Natives, statement};
use bl2_formats::package::{Package, Reader};
use bl2_formats::script::Definition;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let p = Package::open(args[2].as_ref()).expect("open package");
    let nth: usize = args.get(4).map_or(0, |s| s.parse().unwrap());
    let mut natives = Natives::default();
    for name in ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"] {
        natives.add_package(&Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap());
    }
    let e = p
        .exports
        .iter()
        .filter(|e| e.class != 0 && p.name(e.object_name) == args[3] && matches!(p.object_name(e.class).as_str(), "Function" | "State"))
        .nth(nth)
        .expect("function not found");
    let class = p.object_name(e.class);
    let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
    let (Definition::Function { header, .. } | Definition::State { header, .. }) = p.read_definition(e, &class, &mut r).unwrap() else { unreachable!() };
    let code = &p.data[header.script.clone()];
    println!("{} {} outer={} disk={} mem={}", class, p.name(e.object_name), p.object_name(e.outer), code.len(), header.bytecode_size);
    for (i, row) in code.chunks(32).enumerate() {
        println!("{:5}: {}", i * 32, row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
    }
    let mut w = Walker::new(code, p.none_name());
    match w.walk_function() {
        Ok(statements) => statements.iter().for_each(|s| println!("{}", statement(&p, &natives, s))),
        Err(err) => println!("error: {err}"),
    }
    println!("pos={} mem={}", w.pos, w.mem);
}
