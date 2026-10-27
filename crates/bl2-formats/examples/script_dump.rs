//! Research helper: writes decompiled functions and states of the script packages to text files
//! (one file per package, local only — derived from the original, never committed).
//! cargo run -p bl2-formats --release --example script_dump -- <install root> <out dir>

use std::fmt::Write as _;
use std::path::Path;

use bl2_formats::bytecode::Walker;
use bl2_formats::decompile::{Natives, statement};
use bl2_formats::package::{Package, Reader};
use bl2_formats::script::Definition;

const SCRIPT_PACKAGES: [&str; 9] = ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out).unwrap();
    let packages: Vec<Package> = SCRIPT_PACKAGES.iter().map(|n| Package::open(&root.join(format!("WillowGame/CookedPCConsole/{n}.upk"))).unwrap()).collect();
    let mut natives = Natives::default();
    packages.iter().for_each(|p| natives.add_package(p));
    for (pname, p) in SCRIPT_PACKAGES.iter().zip(&packages) {
        let mut text = String::new();
        let (mut functions, mut lines) = (0, 0);
        for e in &p.exports {
            let class = if e.class == 0 { "Class".to_string() } else { p.object_name(e.class) };
            if !matches!(class.as_str(), "Function" | "State") {
                continue;
            }
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            let header = match p.read_definition(e, &class, &mut r).unwrap() {
                Definition::Function { header, native, flags, .. } => {
                    writeln!(text, "\n// {class} {}.{} native={native} flags={flags:#010x}", p.object_name(e.outer), p.name(e.object_name)).unwrap();
                    header
                }
                Definition::State { header, .. } => {
                    writeln!(text, "\n// state {}.{}", p.object_name(e.outer), p.name(e.object_name)).unwrap();
                    header
                }
                _ => unreachable!(),
            };
            functions += 1;
            for s in Walker::new(&p.data[header.script.clone()], p.none_name()).walk_function().unwrap() {
                let line = statement(p, &natives, &s);
                if !line.ends_with("*/ ") {
                    writeln!(text, "{line}").unwrap();
                    lines += 1;
                }
            }
        }
        std::fs::write(out.join(format!("{pname}.txt")), &text).unwrap();
        println!("{pname}: functions/states={functions} statements={lines}");
    }
}
