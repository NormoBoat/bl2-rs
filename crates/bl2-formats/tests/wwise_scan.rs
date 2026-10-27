//! Wwise package/bank/media inventory on the user's copy (T-020); prints counts only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bl2_formats::wwise::{Bank, FilePackage, wave_format};

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { files(&p, ext, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) { out.push(p) }
    }
}

fn kind(media: &[u8]) -> String {
    match wave_format(media) {
        Some(tag) => format!("RIFF fmt {tag:#06x}"),
        None => format!("non-RIFF {:02x?}", &media[..media.len().min(4)]),
    }
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn all_packages() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut paths = Vec::new(); files(&root, "pck", &mut paths); paths.sort();
    let (mut kinds, mut versions, mut chunks, mut langs) = (BTreeMap::<String, usize>::new(), BTreeMap::<u32, usize>::new(), BTreeMap::<String, usize>::new(), BTreeSet::new());
    let (mut banks, mut streams, mut embedded, mut externals) = (0, 0, 0, 0);
    let mut media_ids = BTreeMap::<u32, BTreeSet<String>>::new(); // media id -> kinds seen
    let mut bank_ids = BTreeSet::new();
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let pkg = FilePackage::read(&data).unwrap();
        langs.extend(pkg.languages.iter().map(|(_, l)| l.clone()));
        externals += pkg.externals.len();
        for e in &pkg.streams {
            streams += 1;
            let k = format!("stream {}", kind(&data[e.data.clone()]));
            media_ids.entry(e.id).or_default().insert(k.clone());
            *kinds.entry(k).or_default() += 1;
        }
        for e in &pkg.banks {
            banks += 1;
            bank_ids.insert(e.id);
            let bank = Bank::read(&data, e.data.clone()).unwrap_or_else(|err| panic!("{} bank {:#x}: {err}", path.display(), e.id));
            assert_eq!(bank.id, e.id, "BKHD id differs from the package table");
            *versions.entry(bank.version).or_default() += 1;
            for (tag, _) in &bank.chunks { *chunks.entry(String::from_utf8_lossy(tag).into_owned()).or_default() += 1; }
            for (id, m) in &bank.media {
                embedded += 1;
                let k = format!("embedded {}", kind(&data[m.clone()]));
                media_ids.entry(*id).or_default().insert(k.clone());
                *kinds.entry(k).or_default() += 1;
            }
        }
    }
    println!("pck={} languages={langs:?} banks={banks} (distinct {}) streams={streams} embedded={embedded} externals={externals}", paths.len(), bank_ids.len());
    println!("bank versions {versions:?}");
    println!("chunks {chunks:?}");
    for (k, n) in &kinds { println!("  {k}: {n}"); }
    println!("distinct media ids={}", media_ids.len());
    let empty_streamed = media_ids.values().filter(|k| k.contains("embedded non-RIFF []")).filter(|k| k.iter().any(|x| x.starts_with("stream"))).count();
    let empty_embedded_elsewhere = media_ids.values().filter(|k| k.contains("embedded non-RIFF []") && k.contains("embedded RIFF fmt 0xffff")).count();
    println!("empty media ids: also streamed={empty_streamed}, also embedded with data={empty_embedded_elsewhere}");
}

/// HIRC object types (u8 type, u32 size, u32 id) that mention the ids of non-RIFF or empty embedded media.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL"]
fn odd_media_owners() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut paths = Vec::new(); files(&root, "pck", &mut paths); paths.sort();
    let mut owners = BTreeMap::<String, usize>::new();
    let mut sizes = BTreeMap::<String, BTreeSet<usize>>::new();
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        for e in &FilePackage::read(&data).unwrap().banks {
            let bank = Bank::read(&data, e.data.clone()).unwrap();
            let odd: Vec<_> = bank.media.iter().filter(|(_, m)| wave_format(&data[m.clone()]).is_none()).collect();
            if odd.is_empty() { continue; }
            let Some((_, hirc)) = bank.chunks.iter().find(|(t, _)| t == b"HIRC") else { continue };
            let h = &data[hirc.clone()];
            let n = u32::from_le_bytes(h[..4].try_into().unwrap()) as usize;
            let mut pos = 4;
            for _ in 0..n {
                let (t, size) = (h[pos], u32::from_le_bytes(h[pos + 1..pos + 5].try_into().unwrap()) as usize);
                let body = &h[pos + 5..pos + 5 + size];
                for (id, m) in &odd {
                    if body.windows(4).skip(4).any(|w| w == id.to_le_bytes()) {
                        let k = format!("hirc type {t} -> {}", if m.is_empty() { "empty" } else { "non-RIFF" });
                        *owners.entry(k.clone()).or_default() += 1;
                        // plugin id follows the object id for sources/effects in this bank version
                        sizes.entry(format!("{k} plugin {:02x?}", &body[4..body.len().min(12)])).or_default().insert(m.len());
                    }
                }
                pos += 5 + size;
            }
        }
    }
    println!("{owners:?}");
    for (k, s) in &sizes { println!("{k} sizes={:?}", s.iter().take(6).collect::<Vec<_>>()); }
}

/// AkBank / AkEvent objects of the packages against bank ids and HIRC event objects (type 4) of the .pck files.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn objects_link_to_banks_and_events() {
    use bl2_formats::package::Package;
    use bl2_formats::wwise::short_id;
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let (mut bank_ids, mut event_ids, mut stid_names) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut pcks = Vec::new(); files(&root, "pck", &mut pcks);
    for path in &pcks {
        let data = std::fs::read(path).unwrap();
        for e in &FilePackage::read(&data).unwrap().banks {
            bank_ids.insert(e.id);
            let bank = Bank::read(&data, e.data.clone()).unwrap();
            if let Some((_, stid)) = bank.chunks.iter().find(|(t, _)| t == b"STID") {
                let s = &data[stid.clone()];
                let mut pos = 8;
                for _ in 0..u32::from_le_bytes(s[4..8].try_into().unwrap()) {
                    let id = u32::from_le_bytes(s[pos..pos + 4].try_into().unwrap());
                    let len = s[pos + 4] as usize;
                    let name = String::from_utf8_lossy(&s[pos + 5..pos + 5 + len]).into_owned();
                    assert_eq!(short_id(&name), id, "STID name hashes to its id");
                    stid_names.insert(name.to_lowercase());
                    pos += 5 + len;
                }
                assert_eq!(pos, s.len());
            }
            let Some((_, hirc)) = bank.chunks.iter().find(|(t, _)| t == b"HIRC") else { continue };
            let h = &data[hirc.clone()];
            let mut pos = 4;
            for _ in 0..u32::from_le_bytes(h[..4].try_into().unwrap()) {
                let size = u32::from_le_bytes(h[pos + 1..pos + 5].try_into().unwrap()) as usize;
                if h[pos] == 4 { event_ids.insert(u32::from_le_bytes(h[pos + 5..pos + 9].try_into().unwrap())); }
                pos += 5 + size;
            }
            assert_eq!(pos, h.len(), "HIRC objects tile the chunk");
        }
    }
    let mut upks = Vec::new(); files(&root, "upk", &mut upks);
    let (mut banks, mut events) = (BTreeMap::<String, bool>::new(), BTreeMap::<String, bool>::new());
    for path in upks.iter().filter(|p| !p.to_string_lossy().contains("Mancana_Textures")) {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0) {
            let name = p.name(e.object_name);
            if name.starts_with("Default__") { continue; }
            match p.object_name(e.class).as_str() {
                "AkBank" => { banks.insert(name.clone(), bank_ids.contains(&short_id(&format!("{}_PC", name.strip_prefix("AkBank_").unwrap_or(&name))))); }
                "AkEvent" => { events.insert(name.clone(), event_ids.contains(&short_id(name.strip_prefix("Ak_").unwrap_or(&name)))); }
                _ => {}
            }
        }
    }
    let count = |m: &BTreeMap<String, bool>| m.values().filter(|&&v| v).count();
    println!("AkBank objects={} matching bank ids={} (pck banks {}); AkEvent objects={} matching HIRC events={} (HIRC events {})",
        banks.len(), count(&banks), bank_ids.len(), events.len(), count(&events), event_ids.len());
    for (n, _) in banks.iter().filter(|(_, v)| !**v) { println!("  bank without pck: {n}"); }
    println!("STID bank names={} e.g. {:?}", stid_names.len(), stid_names.iter().take(8).collect::<Vec<_>>());
    for (n, _) in events.iter().filter(|(_, v)| !**v).take(10) { println!("  event without HIRC: {n}"); }
}
