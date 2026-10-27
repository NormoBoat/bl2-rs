//! Reads the object header + tagged properties of every export in the installation (plan/12-tasks.md T-032).
//! Run: $env:BL2_INSTALL="..."; cargo test -p bl2-formats --release --test property_scan -- --ignored --nocapture
//! Optional: BL2_SCAN_LIMIT=<n> to scan only the first n packages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::{ClassTree, ObjectReference, ScalarValue, is_script_definition};

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
struct ClassStats {
    total: usize,
    failed: usize,
    with_native_tail: usize,
    first_error: Option<String>,
}

#[test]
#[ignore = "needs the user's Borderlands 2 install in BL2_INSTALL"]
fn every_export_has_readable_tagged_properties() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut files = Vec::new();
    upk_files(&root, &mut files);
    files.sort();

    // Script packages are the fully compressed ones (plan/03-evidence.md E-040).
    let mut tree = ClassTree::default();
    for f in files.iter().filter(|f| f.with_extension("upk.uncompressed_size").exists()) {
        tree.add_package(&Package::open(f).unwrap());
    }
    tree.add_editor_classes();

    let limit: usize = std::env::var("BL2_SCAN_LIMIT").ok().map_or(usize::MAX, |s| s.parse().unwrap());
    let mut stats: BTreeMap<String, ClassStats> = BTreeMap::new();
    let (mut exports, mut failed, mut tags, mut script_definitions) = (0usize, 0usize, 0usize, 0usize);
    let mut scalar_counts = BTreeMap::<String, usize>::new();
    let mut unsupported_counts = BTreeMap::<String, usize>::new();
    let mut scalar_errors = BTreeMap::<String, (usize, String)>::new();
    let mut unresolved_refs = BTreeMap::<String, usize>::new();
    for f in files.iter().take(limit) {
        let p = Package::open(f).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0) {
            exports += 1;
            let class = if e.class == 0 { "Class".to_string() } else { p.object_name(e.class) };
            if is_script_definition(&class) {
                script_definitions += 1;
                continue;
            }
            let s = stats.entry(class).or_default();
            s.total += 1;
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            match p.read_object_header(e, &tree, &mut r).and_then(|_| p.read_tags(&mut r)) {
                Ok(t) => {
                    tags += t.len();
                    for tag in &t {
                        let kind = p.name(tag.type_name);
                        match p.read_scalar_tag(tag) {
                            Ok(Some(value)) => {
                                *scalar_counts.entry(kind).or_default() += 1;
                                if let ScalarValue::Object(ObjectReference::Unresolved(raw))
                                    | ScalarValue::Delegate { object: ObjectReference::Unresolved(raw), .. } = value
                                {
                                    *unresolved_refs.entry(format!("{}.{}, raw={:#010x}", p.object_name(e.class), p.name(tag.name), raw as u32)).or_default() += 1;
                                }
                            }
                            Ok(None) => *unsupported_counts.entry(kind).or_default() += 1,
                            Err(err) => {
                                let entry = scalar_errors.entry(kind).or_insert_with(|| (0,
                                    format!("{} {}.{}: {err}", f.display(), p.name(e.object_name), p.name(tag.name))));
                                entry.0 += 1;
                            }
                        }
                    }
                    if r.pos < end {
                        s.with_native_tail += 1;
                    }
                }
                Err(err) => {
                    failed += 1;
                    s.failed += 1;
                    s.first_error.get_or_insert_with(|| {
                        format!("{} #{} {} flags={:#x}: {err}", f.file_name().unwrap().to_string_lossy(),
                            p.exports.iter().position(|x| std::ptr::eq(x, e)).unwrap(), p.name(e.object_name), e.object_flags)
                    });
                }
            }
        }
    }
    let unknown: Vec<&String> = stats.keys().filter(|c| !tree.contains(c) && c.as_str() != "Class").collect();
    println!(
        "exports={exports} script_definitions_skipped={script_definitions} failed={failed} tags={tags} classes={} classes_not_in_tree={}",
        stats.len(),
        unknown.len()
    );
    if !unknown.is_empty() {
        println!("NOT_IN_TREE {:?}", unknown.iter().take(30).collect::<Vec<_>>());
    }
    let mut bad: Vec<_> = stats.iter().filter(|(_, s)| s.failed > 0).collect();
    bad.sort_by_key(|(_, s)| std::cmp::Reverse(s.failed));
    for (c, s) in bad.iter().take(40) {
        println!("FAILCLASS {c}: {}/{} failed; {}", s.failed, s.total, s.first_error.as_deref().unwrap_or(""));
    }
    let mut tails: Vec<_> = stats.iter().filter(|(_, s)| s.with_native_tail > 0).collect();
    tails.sort_by_key(|(_, s)| std::cmp::Reverse(s.with_native_tail));
    let tail_list: Vec<String> = tails.iter().map(|(c, s)| format!("{c}={}", s.with_native_tail)).collect();
    println!("NATIVE_TAIL_CLASSES {}: {}", tails.len(), tail_list.join(" "));
    println!("SCALARS {scalar_counts:?}");
    println!("UNSUPPORTED_VALUES {unsupported_counts:?}");
    println!("SCALAR_ERRORS {scalar_errors:?}");
    println!("UNRESOLVED_OBJECT_REFERENCES {unresolved_refs:?}");
    assert_eq!(failed, 0);
    assert!(scalar_errors.is_empty());
}
