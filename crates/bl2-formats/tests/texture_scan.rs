//! Parses the native mip data of every Texture2D-family export and reads every TFC mip (plan/12-tasks.md T-034).
//! Run: $env:BL2_INSTALL="..."; cargo test -p bl2-formats --release --test texture_scan -- --ignored --nocapture
//! BL2_TFC_CHECK=0 skips reading mip payloads from .tfc files.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::ClassTree;
use bl2_formats::texture::{BULK_STORE_IN_SEPARATE_FILE, BULK_UNUSED, Texture2DData, mip_bytes, read_tfc_mip};

const NOT_PACKAGES: [&str; 2] = ["Mancana_Textures_Data.upk", "Mancana_Textures_Meta.upk"];

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))
            && !NOT_PACKAGES.contains(&p.file_name().unwrap().to_string_lossy().as_ref())
        {
            out.push(p);
        }
    }
}

#[derive(Default, Debug)]
struct Stats {
    total: usize,
    ok: usize,
    first_error: Option<String>,
}

#[test]
#[ignore = "needs the user's Borderlands 2 install in BL2_INSTALL"]
fn every_texture_mip_is_consistent() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let check_tfc = std::env::var("BL2_TFC_CHECK").map_or(true, |v| v != "0");
    let mut packages = Vec::new();
    files(&root, "upk", &mut packages);
    packages.sort();
    let mut tfcs = Vec::new();
    files(&root, "tfc", &mut tfcs);
    let mut tfc_by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for t in &tfcs {
        tfc_by_name.entry(t.file_stem().unwrap().to_string_lossy().to_lowercase()).or_default().push(t.clone());
    }
    let ambiguous: Vec<_> = tfc_by_name.iter().filter(|(_, v)| v.len() > 1).map(|(k, _)| k.clone()).collect();
    println!("tfc_files={} names={} ambiguous_names={ambiguous:?}", tfcs.len(), tfc_by_name.len());

    let mut tree = ClassTree::default();
    for f in packages.iter().filter(|f| f.with_extension("upk.uncompressed_size").exists()) {
        tree.add_package(&Package::open(f).unwrap());
    }
    tree.add_editor_classes();

    let mut stats: BTreeMap<String, Stats> = BTreeMap::new();
    let (mut mips, mut inline_mips, mut tfc_mips, mut tfc_bytes, mut formats) = (0usize, 0usize, 0usize, 0usize, BTreeMap::<String, usize>::new());
    let mut open_tfc: Option<(PathBuf, std::fs::File)> = None;
    for f in &packages {
        let p = Package::open(f).unwrap();
        for e in &p.exports {
            let class = p.object_name(e.class);
            // Class default objects carry no native mip data.
            if e.class == 0 || e.object_flags & 0x200 != 0 || !tree.is_a(&class, "Texture2D") {
                continue;
            }
            let s = stats.entry(class).or_default();
            s.total += 1;
            let end = e.serial_offset + e.serial_size;
            let result = (|| -> Result<(), String> {
                let mut r = Reader::new(&p.data[..end], e.serial_offset);
                p.read_object_header(e, &tree, &mut r).map_err(|x| x.to_string())?;
                let tags = p.read_tags(&mut r).map_err(|x| x.to_string())?;
                let tag = |n: &str| tags.iter().find(|t| p.name(t.name) == n);
                let format = tag("Format").map(|t| p.tag_name_value(t)).transpose().map_err(|x| x.to_string())?.unwrap_or_else(|| "PF_DXT1".into());
                let tfc_name = tag("TextureFileCacheName").map(|t| p.tag_name_value(t)).transpose().map_err(|x| x.to_string())?;
                let t = Texture2DData::read(&mut r, tree.is_a(&p.object_name(e.class), "LightMapTexture2D")).map_err(|x| x.to_string())?;
                if r.pos != end {
                    return Err(format!("{} bytes left after native data", end - r.pos));
                }
                *formats.entry(format.clone()).or_default() += 1;
                for m in &t.mips {
                    mips += 1;
                    if m.data.flags & BULK_UNUSED != 0 {
                        continue;
                    }
                    let want = mip_bytes(&format, m.size_x, m.size_y).ok_or(format!("unknown format {format}"))?;
                    if m.data.element_count != want {
                        return Err(format!("{format} {}x{} mip has {} bytes, expected {want}", m.size_x, m.size_y, m.data.element_count));
                    }
                    if m.data.flags & BULK_STORE_IN_SEPARATE_FILE == 0 {
                        inline_mips += 1;
                        if m.data.inline.len() != m.data.element_count {
                            return Err(format!("inline mip flags {:#x}: {} bytes on disk, {} expected", m.data.flags, m.data.inline.len(), m.data.element_count));
                        }
                        continue;
                    }
                    tfc_mips += 1;
                    if !check_tfc {
                        continue;
                    }
                    let name = tfc_name.as_deref().ok_or("separate-file mip without TextureFileCacheName")?.to_lowercase();
                    let path = tfc_by_name.get(&name).and_then(|v| v.first()).ok_or(format!("no TFC named {name}"))?;
                    if open_tfc.as_ref().is_none_or(|(p0, _)| p0 != path) {
                        open_tfc = Some((path.clone(), std::fs::File::open(path).map_err(|x| x.to_string())?));
                    }
                    let file = &mut open_tfc.as_mut().unwrap().1;
                    let mut buf = vec![0u8; usize::try_from(m.data.size_on_disk).map_err(|_| "separate-file mip without size".to_string())?];
                    let offset = u64::try_from(m.data.offset_in_file).map_err(|_| "separate-file mip without offset".to_string())?;
                    file.seek(SeekFrom::Start(offset)).map_err(|x| x.to_string())?;
                    file.read_exact(&mut buf).map_err(|x| x.to_string())?;
                    let mut d = m.data.clone();
                    d.offset_in_file = 0;
                    tfc_bytes += read_tfc_mip(&buf, &d).map_err(|x| format!("{name}: {x}"))?.len();
                }
                Ok(())
            })();
            match result {
                Ok(()) => s.ok += 1,
                Err(err) => {
                    s.first_error.get_or_insert_with(|| format!("{} {}: {err}", f.file_name().unwrap().to_string_lossy(), p.name(e.object_name)));
                }
            }
        }
    }
    println!("mips={mips} inline={inline_mips} tfc={tfc_mips} tfc_bytes_decoded={tfc_bytes} formats={formats:?}");
    let mut failed = 0;
    for (c, s) in &stats {
        println!("CLASS {c}: {}/{} ok {}", s.ok, s.total, s.first_error.as_deref().unwrap_or(""));
        failed += s.total - s.ok;
    }
    assert_eq!(failed, 0);
}

/// Mancana Remaster textures (E-075): every index entry parses as a record to its last byte, and every mip has the
/// size its format and dimensions give — inline, or read (and LZO-decompressed) from the named `.tfc`.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn mancana_remaster_textures() {
    use bl2_formats::mancana::{Index, Record};
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let dir = root.join("DLC/Mancana/Lic/Content/Remaster");
    let index = Index::read(&std::fs::read(dir.join("Mancana_Textures_Meta.upk")).unwrap()).unwrap();
    let data = std::fs::read(dir.join(&index.data_file)).unwrap();
    let mut tfc_paths = Vec::new(); files(&root, "tfc", &mut tfc_paths);
    let tfc_path = |name: &str| tfc_paths.iter().find(|p| p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(name))).cloned();
    let mut tfcs = HashMap::<String, Vec<u8>>::new();
    let mut stats = BTreeMap::<String, usize>::new();
    for e in &index.entries {
        let rec = Record::read(&data, e.data.clone()).unwrap_or_else(|err| panic!("{}: {err}", e.path));
        let format = rec.string(&data, "Format").unwrap_or_default();
        *stats.entry(format!("format {format}")).or_default() += 1;
        let tfc_name = rec.string(&data, "TextureFileCacheName");
        for m in &rec.texture.mips {
            let want = mip_bytes(&format, m.size_x, m.size_y);
            let outcome = if m.data.flags & BULK_UNUSED != 0 { "unused".to_string() }
                else if m.data.flags & BULK_STORE_IN_SEPARATE_FILE != 0 {
                    let name = tfc_name.clone().expect("separate mip without TextureFileCacheName");
                    match tfc_path(&name) {
                        None => format!("tfc {name} not in the installation"),
                        Some(path) => {
                            *stats.entry(format!("tfc {}", path.strip_prefix(&root).unwrap().display())).or_default() += 1;
                            let tfc = tfcs.entry(name.clone()).or_insert_with(|| std::fs::read(&path).unwrap());
                            match read_tfc_mip(tfc, &m.data) { Ok(b) => format!("tfc size matches format: {}", Some(b.len()) == want), Err(err) => format!("tfc error {err}") }
                        }
                    }
                } else { format!("inline size matches format: {}", Some(m.data.inline.len()) == want) };
            *stats.entry(outcome).or_default() += 1;
        }
    }
    for (k, v) in &stats { println!("{v:7} {k}"); }
    println!("{} records", index.entries.len());
}
