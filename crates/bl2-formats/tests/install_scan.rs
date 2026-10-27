//! Parses every package of the user's installation (plan/12-tasks.md T-031).
//! Run: $env:BL2_INSTALL="D:\Projects\bl2-rs\Borderlands 2"; cargo test -p bl2-formats --release -- --ignored --nocapture

use std::path::{Path, PathBuf};

use bl2_formats::package::Package;

/// Not UE3 packages: Mancana remaster texture store (plan/03-evidence.md E-006, task T-034).
const NOT_PACKAGES: [&str; 2] = ["Mancana_Textures_Data.upk", "Mancana_Textures_Meta.upk"];

fn upk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            upk_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "needs the user's Borderlands 2 install in BL2_INSTALL"]
fn every_package_parses() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut files = Vec::new();
    upk_files(&root, &mut files);
    files.sort();
    let (mut ok, mut skipped, mut chunked, mut full) = (0, 0, 0, 0);
    let (mut names, mut imports, mut exports, mut bytes) = (0usize, 0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    for f in &files {
        let file_name = f.file_name().unwrap().to_string_lossy();
        if NOT_PACKAGES.contains(&file_name.as_ref()) {
            skipped += 1;
            continue;
        }
        match Package::open(f) {
            Ok(p) => {
                ok += 1;
                if !p.summary.chunks.is_empty() {
                    chunked += 1;
                }
                if f.with_extension("upk.uncompressed_size").exists() {
                    full += 1;
                }
                names += p.names.len();
                imports += p.imports.len();
                exports += p.exports.len();
                bytes += p.data.len();
            }
            Err(e) => failures.push(format!("{}: {e}", f.strip_prefix(&root).unwrap().display())),
        }
    }
    println!(
        "packages={} parsed={ok} skipped_non_ue3={skipped} chunk_compressed={chunked} fully_compressed={full} \
         names={names} imports={imports} exports={exports} uncompressed_bytes={bytes} failures={}",
        files.len(),
        failures.len()
    );
    for f in failures.iter().take(20) {
        println!("FAIL {f}");
    }
    assert!(failures.is_empty(), "{} packages failed", failures.len());
}
