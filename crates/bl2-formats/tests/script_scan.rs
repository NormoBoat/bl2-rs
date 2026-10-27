//! Parses every script definition export (classes, structs, functions, states, enums, consts, properties)
//! and requires each body to be consumed exactly (plan/12-tasks.md T-015).
//! Run: $env:BL2_INSTALL="..."; cargo test -p bl2-formats --release --test script_scan -- --ignored --nocapture

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bl2_formats::bytecode::Walker;
use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::is_script_definition;
use bl2_formats::script::Definition;

const NOT_PACKAGES: [&str; 2] = ["Mancana_Textures_Data.upk", "Mancana_Textures_Meta.upk"];

fn upk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            upk_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk"))
            && !NOT_PACKAGES.contains(&p.file_name().unwrap().to_string_lossy().as_ref())
        {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Stats {
    total: usize,
    ok: usize,
    first_error: Option<String>,
}

#[test]
#[ignore = "needs the user's Borderlands 2 install in BL2_INSTALL"]
fn every_script_definition_parses() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut files = Vec::new();
    upk_files(&root, &mut files);
    files.sort();
    let mut stats: BTreeMap<String, Stats> = BTreeMap::new();
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    let (mut bytecode_functions, mut bytecode_bytes, mut walked_ok) = (0usize, 0usize, 0usize);
    let mut opcodes = [0usize; 256];
    let mut walk_errors: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for f in &files {
        let p = Package::open(f).unwrap();
        let none_name = p.none_name();
        for e in p.exports.iter().filter(|e| e.serial_size > 0) {
            let class = if e.class == 0 { "Class".to_string() } else { p.object_name(e.class) };
            if !is_script_definition(&class) {
                continue;
            }
            let s = stats.entry(class.clone()).or_default();
            s.total += 1;
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            let res = p.read_definition(e, &class, &mut r).map_err(|x| x.to_string()).and_then(|d| {
                if r.pos != end {
                    return Err(format!("{} bytes left", end as i64 - r.pos as i64));
                }
                let header = match &d {
                    Definition::Function { header, .. } | Definition::State { header, .. } | Definition::Class { header, .. } => Some(header),
                    _ => None,
                };
                if let Some(h) = header.filter(|h| !h.script.is_empty()) {
                    let mut w = Walker::new(&p.data[h.script.clone()], none_name);
                    let walked = w.walk_function();
                    let problem = match &walked {
                        Err(err) => Some(err.to_string()),
                        Ok(_) if w.pos != h.script.len() => Some(format!("EndOfScript at {} of {}", w.pos, h.script.len())),
                        Ok(_) if w.mem != h.bytecode_size as usize => Some(format!("memory size {} != {}", w.mem, h.bytecode_size)),
                        Ok(_) => None,
                    };
                    match problem {
                        None => {
                            walked_ok += 1;
                            for s in walked.as_ref().unwrap() {
                                s.visit(&mut |x| opcodes[x.op as usize] += 1);
                            }
                        }
                        Some(msg) => {
                            let key: String = msg.split(" at ").next().unwrap_or("").chars().take(48).collect();
                            let e2 = walk_errors.entry(key).or_insert((0, format!("{}.{}: {msg}", f.file_name().unwrap().to_string_lossy(), p.name(e.object_name))));
                            e2.0 += 1;
                        }
                    }
                }
                match &d {
                    Definition::Function { header, native, .. } => {
                        if !header.script.is_empty() {
                            bytecode_functions += 1;
                            bytecode_bytes += header.script.len();
                        }
                        *hist.entry(format!("function_native={}", *native != 0)).or_default() += 1;
                    }
                    Definition::State { state, .. } => *hist.entry(format!("state_flags={}", state.state_flags)).or_default() += 1,
                    Definition::Class { class, .. } => {
                        *hist.entry(format!("class_unknown_byte={}", class.unknown_byte)).or_default() += 1;
                        *hist.entry(format!("class_name_list_lens={:?}", class.name_lists.iter().map(Vec::len).collect::<Vec<_>>())).or_default() += 1;
                    }
                    _ => {}
                }
                Ok(())
            });
            match res {
                Ok(()) => s.ok += 1,
                Err(err) => {
                    s.first_error.get_or_insert_with(|| format!("{} {}: {err}", f.file_name().unwrap().to_string_lossy(), p.name(e.object_name)));
                }
            }
        }
    }
    let mut failed = 0;
    for (c, s) in &stats {
        println!("CLASS {c}: {}/{} {}", s.ok, s.total, s.first_error.as_deref().unwrap_or(""));
        failed += s.total - s.ok;
    }
    let shown: Vec<String> = hist.iter().filter(|(k, _)| !k.starts_with("class_name_list_lens") || hist.len() < 40).map(|(k, v)| format!("{k}={v}")).collect();
    println!("HIST {}", shown.join(" | "));
    println!("bytecode_functions={bytecode_functions} bytecode_bytes={bytecode_bytes} walked_ok={walked_ok}");
    let mut errs: Vec<_> = walk_errors.values().collect();
    errs.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    for (n, ex) in errs.iter().take(25) {
        println!("WALKERR x{n}: {ex}");
    }
    let used: Vec<String> = opcodes.iter().enumerate().filter(|(i, n)| **n > 0 && *i < 0x60).map(|(i, n)| format!("{i:02x}:{n}")).collect();
    println!("OPCODES {}", used.join(" "));
    assert_eq!(failed, 0);
    assert!(walk_errors.is_empty(), "{} bytecode walk error kinds", walk_errors.len());
}
