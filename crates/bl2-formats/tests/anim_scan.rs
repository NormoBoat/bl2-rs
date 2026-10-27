//! Skeletal meshes and animations in the user's packages (T-036); prints counts only, writes no derived data.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::{ClassTree, ScalarValue};

const PACKAGES: [&str; 9] = ["Core", "Engine", "GameFramework", "GearboxFramework", "WillowGame", "GFxUI", "IpDrv", "OnlineSubsystemSteamworks", "AkAudio"];

fn root() -> PathBuf { PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL")) }

fn tree(root: &Path) -> ClassTree {
    let mut tree = ClassTree::default();
    for name in PACKAGES { tree.add_package(&Package::open(&root.join(format!("WillowGame/CookedPCConsole/{name}.upk"))).unwrap()); }
    tree.add_editor_classes();
    tree
}

fn upks(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { upks(&p, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) && !p.to_string_lossy().contains("Mancana_Textures") { out.push(p) }
    }
}

/// Classes of the animation system: object counts, AnimSequence compression formats and the layout of its native
/// data (u32 0, then `TArray<u8>` CompressedByteStream).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn anim_survey() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let classes = ["SkeletalMesh", "AnimSequence", "AnimSet", "MorphTarget", "MorphTargetSet", "AnimTree", "PhysicsAsset"];
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0) {
            let class = p.object_name(e.class);
            if !classes.contains(&class.as_str()) { continue; }
            if e.object_flags & 0x200 != 0 { *stats.entry(format!("{class} (class default)")).or_default() += 1; continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let tags = p.read_tags(&mut r).unwrap();
            let tail = end - r.pos;
            *stats.entry(class.clone()).or_default() += 1;
            if class != "AnimSequence" {
                *stats.entry(format!("{class} tail {}", if tail == 0 { "empty".into() } else { format!("{} bytes class", tail.next_power_of_two()) })).or_default() += 1;
                continue;
            }
            let mut fmt = Vec::new();
            for t in &tags {
                let n = p.name(t.name);
                if (n.ends_with("CompressionFormat") || n == "KeyEncodingFormat") && let Ok(Some(ScalarValue::Enum(v))) = p.read_scalar_tag(t) {
                    fmt.push(format!("{n}={}", p.name(v)));
                }
            }
            *stats.entry(format!("AnimSequence formats {fmt:?}")).or_default() += 1;
            // Native data: u32 (empty raw-data array?) then TArray<u8> CompressedByteStream.
            let (lead, n) = (r.u32().unwrap(), r.count().unwrap());
            *stats.entry(format!("AnimSequence tail: lead {lead}, 8 + count {}", tail == 8 + n)).or_default() += 1;
        }
    }
    for (k, v) in &stats { println!("{v:7} {k}"); }
}

/// `anim::Sequence` from an AnimSequence export's tags and native data.
fn sequence(p: &Package, tree: &ClassTree, e: &bl2_formats::package::Export) -> bl2_formats::anim::Sequence {
    use bl2_formats::anim::{COMPRESSION_FORMATS, KEY_FORMATS, Sequence, read_native};
    let end = e.serial_offset + e.serial_size;
    let mut r = Reader::new(&p.data[..end], e.serial_offset);
    p.read_object_header(e, tree, &mut r).unwrap();
    let tags = p.read_tags(&mut r).unwrap();
    let mut s = Sequence { stream: read_native(&mut r, end).unwrap(), ..Default::default() };
    for t in &tags {
        let n = p.name(t.name);
        let v = p.read_scalar_tag(t).ok().flatten();
        let index = |list: &[&str]| match &v { Some(ScalarValue::Enum(e)) => list.iter().position(|f| *f == p.name(*e)).unwrap() as u8, _ => panic!("{n} is not an enum") };
        match n.as_str() {
            "SequenceLength" => if let Some(ScalarValue::Float(f)) = v { s.sequence_length = f },
            "NumFrames" => if let Some(ScalarValue::Int(i)) = v { s.num_frames = i },
            "TranslationCompressionFormat" => s.translation_format = index(&COMPRESSION_FORMATS),
            "RotationCompressionFormat" => s.rotation_format = index(&COMPRESSION_FORMATS),
            "KeyEncodingFormat" => s.key_format = index(&KEY_FORMATS),
            "CompressedTrackOffsets" => {
                let mut a = Reader::new(&p.data[..t.value.end], t.value.start);
                s.track_offsets = (0..a.count().unwrap()).map(|_| a.i32().unwrap()).collect();
            }
            _ => {}
        }
    }
    s
}

/// Rotation decoding of every constant-key sequence: each key (time of key k) decodes, and the angle between
/// consecutive keys of a track, per frame, is small (a wrong bit layout would make keys jump).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn rotations() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let (mut seqs, mut keys) = (0, 0usize);
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "AnimSequence" { continue; }
            let s = sequence(&p, &tree, e);
            if s.key_format != 0 || s.rotation_format != 9 { continue; }
            seqs += 1;
            for track in 0..s.track_offsets.len() / 4 {
                let n = s.track_offsets[track * 4 + 3];
                let at = |k: i32| s.rotation(track, if n > 1 { s.sequence_length * k as f32 / (n - 1) as f32 } else { 0.0 }, false).unwrap();
                let mut prev = at(0);
                for k in 1..n {
                    let q = at(k);
                    let dot = (prev[0] * q[0] + prev[1] * q[1] + prev[2] * q[2] + prev[3] * q[3]).abs().min(1.0);
                    // Per frame: keys are spread evenly over NumFrames - 1 frame steps.
                    let deg = 2.0 * dot.acos().to_degrees() * (n - 1) as f32 / (s.num_frames - 1).max(1) as f32;
                    *stats.entry(format!("step {}", match deg { d if d < 1.0 => "< 1 deg", d if d < 5.0 => "1-5 deg", d if d < 20.0 => "5-20 deg", d if d < 60.0 => "20-60 deg", _ => ">= 60 deg" })).or_default() += 1;
                    prev = q;
                    keys += 1;
                }
            }
        }
    }
    for (k, v) in &stats { println!("{v:9} {k}"); }
    println!("{seqs} sequences, {keys} key steps");
}

/// Key decoders against the original: ANIM_KEYS = JSON lines from tools/research/frida-anim-keys.mjs (inputs and
/// outputs of exe 0x4f92a0 / 0x4f9430 captured in the running game). Reports exact matches and the largest error.
#[test]
#[ignore = "needs a capture from the original (ANIM_KEYS)"]
fn keys_match_original() {
    let text = std::fs::read_to_string(std::env::var("ANIM_KEYS").expect("set ANIM_KEYS")).unwrap();
    let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
    let field = |l: &str, k: &str| { let at = l.find(&format!("\"{k}\":\"")).unwrap() + k.len() + 4; l[at..at + l[at..].find('"').unwrap()].to_string() };
    let mut stats = BTreeMap::<String, (usize, usize, f32)>::new();
    for l in text.lines().filter(|l| l.contains("\"key\"")) {
        let (f, key, out) = (field(l, "f"), hex(&field(l, "key")), hex(&field(l, "out")));
        let want: [f32; 4] = std::array::from_fn(|i| f32::from_le_bytes(out[i * 4..i * 4 + 4].try_into().unwrap()));
        let got = match f.as_str() {
            "polar32" => bl2_formats::anim::polar32(u32::from_le_bytes(key[..4].try_into().unwrap())),
            "polar48" => bl2_formats::anim::polar48(key[..6].try_into().unwrap()),
            _ => continue,
        };
        let e = stats.entry(f).or_default();
        e.0 += 1;
        if got.iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits()) { e.1 += 1; } else if std::env::var("ANIM_KEYS_DIFF").is_ok() { println!("{} {key:02x?} got {got:?} want {want:?}", e.0); }
        e.2 = e.2.max(got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max));
    }
    for (f, (n, exact, max)) in &stats { println!("{f}: {n} keys, {exact} bit-exact, max abs error {max:e}"); }
    assert!(stats.values().all(|s| s.2 < 1e-5));
}

/// Whole codec path against the original: ANIM_POSES = tools/research/anim-pose-text.mjs output of a
/// frida-anim-pose.mjs capture (sequence fields from memory, time, looping, (atom, track) pairs and the FBoneAtoms
/// the original's GetPoseRotations / GetPoseTranslations wrote). Recomputes every pair with `anim::Sequence`.
#[test]
#[ignore = "needs a capture from the original (ANIM_POSES)"]
fn poses_match_original() {
    use bl2_formats::anim::Sequence;
    use std::collections::HashMap;
    let text = std::fs::read_to_string(std::env::var("ANIM_POSES").expect("set ANIM_POSES")).unwrap();
    let hex = |s: &str| if s == "-" { vec![] } else { (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>() };
    let floats = |b: &[u8]| b.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<f32>>();
    let mut seqs = HashMap::<String, Sequence>::new();
    let mut stats = BTreeMap::<String, (usize, usize, f32)>::new();
    for l in text.lines() {
        let t: Vec<&str> = l.split(' ').collect();
        if t[0] == "S" {
            let offsets = hex(t[7]).chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
            seqs.insert(t[1].into(), Sequence { sequence_length: t[2].parse().unwrap(), num_frames: t[3].parse().unwrap(), translation_format: t[4].parse().unwrap(),
                rotation_format: t[5].parse().unwrap(), key_format: t[6].parse().unwrap(), track_offsets: offsets, stream: hex(t[8]) });
            continue;
        }
        let (slot, s, time, looping) = (t[1], &seqs[t[2]], t[3].parse::<f32>().unwrap(), t[4] != "0");
        for p in t[6..].chunks(3) {
            let track: usize = p[1].parse().unwrap();
            let want = floats(&hex(p[2]));
            let (got, want, name) = if slot == "4" {
                (s.rotation(track, time, looping).unwrap().to_vec(), want[..4].to_vec(), format!("key format {} rotation format {}", s.key_format, s.rotation_format))
            } else {
                (s.translation(track, time, looping).unwrap().to_vec(), want[4..7].to_vec(), format!("key format {} translation format {}", s.key_format, s.translation_format))
            };
            let e = stats.entry(name).or_default();
            e.0 += 1;
            if got.iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits()) { e.1 += 1; }
            e.2 = e.2.max(got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max));
        }
    }
    for (f, (n, exact, max)) in &stats { println!("{f}: {n} bone samples, {exact} bit-exact, max abs error {max:e}"); }
}

/// Decoding coverage: every track of every sequence at every frame and half frame, rotation and translation; counts
/// sequences per (formats, outcome).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn decode_coverage() {
    use bl2_formats::anim::{COMPRESSION_FORMATS, KEY_FORMATS};
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "AnimSequence" { continue; }
            let s = sequence(&p, &tree, e);
            let mut outcome = "decoded".to_string();
            // Every frame (and half frame) of every track.
            let steps = (s.num_frames - 1).max(1) * 2;
            'tracks: for track in 0..s.tracks() {
                for k in 0..=steps {
                    let t = s.sequence_length * k as f32 / steps as f32;
                    let r = s.translation(track, t, false).map(|v| v.iter().all(|x| x.is_finite()))
                        .and_then(|a| s.rotation(track, t, false).map(|v| a && v.iter().all(|x| x.is_finite())));
                    match r { Ok(true) => {}, Ok(false) => { outcome = "non-finite value".into(); break 'tracks; } Err(err) => { outcome = err.to_string(); break 'tracks; } }
                }
            }
            *stats.entry(format!("{}/{}/{}: {outcome}", COMPRESSION_FORMATS[s.translation_format as usize], COMPRESSION_FORMATS[s.rotation_format as usize], KEY_FORMATS[s.key_format as usize])).or_default() += 1;
        }
    }
    for (k, v) in &stats { println!("{v:7} {k}"); }
}

/// PerTrackCompression track headers in the data: (rotation/translation, format, flags) counts. Offsets are two i32
/// per track (translation, rotation; −1 = identity); the u32 header at an offset holds format in bits 28–31, flags in
/// 24–27 and the key count in 0–23 (exe 0x4fd530).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn per_track_headers() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "AnimSequence" { continue; }
            let s = sequence(&p, &tree, e);
            if s.key_format != 2 { continue; }
            for (i, &o) in s.track_offsets.iter().enumerate() {
                let kind = if i % 2 == 0 { "translation" } else { "rotation" };
                if o < 0 { *stats.entry(format!("{kind} identity (-1)")).or_default() += 1; continue; }
                let h = u32::from_le_bytes(s.stream[o as usize..o as usize + 4].try_into().unwrap());
                let keys = h & 0xff_ffff;
                *stats.entry(format!("{kind} format {} flags {:#x} keys {}", h >> 28, h >> 24 & 0xf, if keys < 2 { keys.to_string() } else { "2+".into() })).or_default() += 1;
            }
        }
    }
    for (k, v) in &stats { println!("{v:8} {k}"); }
}

/// Packages holding sequences of the rarer key/compression formats (to choose scenes for comparing with the original).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn rare_format_packages() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "AnimSequence" { continue; }
            let s = sequence(&p, &tree, e);
            let kind = match (s.key_format, s.rotation_format) { (2, _) => "per-track", (1, _) => "variable", (_, 7) => "delta48", _ => continue };
            *stats.entry(format!("{kind} {}", path.file_name().unwrap().to_string_lossy())).or_default() += 1;
        }
    }
    for (k, v) in &stats { println!("{v:6} {k}"); }
}

/// AnimSet → sequences: every sequence an AnimSet lists (Sequences, export references) has as many tracks as the
/// set has TrackBoneNames (FName array), so track i animates bone TrackBoneNames[i].
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn anim_sets_match_tracks() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "AnimSet" { continue; }
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let (mut bones, mut seqs) = (0, Vec::new());
            for t in p.read_tags(&mut r).unwrap() {
                let mut a = Reader::new(&p.data[..t.value.end], t.value.start);
                match p.name(t.name).as_str() {
                    "TrackBoneNames" => bones = a.count().unwrap(),
                    "Sequences" => seqs = (0..a.count().unwrap()).map(|_| a.i32().unwrap()).collect(),
                    _ => {}
                }
            }
            for s in seqs {
                if s <= 0 { *stats.entry(format!("sequence reference {}", if s == 0 { "null" } else { "import" })).or_default() += 1; continue; }
                let se = &p.exports[s as usize - 1];
                let tracks = sequence(&p, &tree, se).tracks();
                *stats.entry(format!("tracks == TrackBoneNames: {}", tracks == bones)).or_default() += 1;
            }
        }
    }
    for (k, v) in &stats { println!("{v:7} {k}"); }
}

/// Native data of every SkeletalMesh with `skeletal::SkeletalMesh::read`; on failure SKEL_OUT gets the tail.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn skeletal_meshes() {
    use bl2_formats::skeletal::SkeletalMesh;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "SkeletalMesh" { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let tags = p.read_tags(&mut r).unwrap();
            let colors = tags.iter().any(|t| p.name(t.name) == "bHasVertexColors" && matches!(p.read_scalar_tag(t), Ok(Some(ScalarValue::Bool(true)))));
            let start = r.pos;
            match SkeletalMesh::read(&mut r, end, colors) {
                Ok(m) => {
                    *stats.entry("decoded".into()).or_default() += 1;
                    *stats.entry(format!("lods {}", m.lods.len())).or_default() += 1;
                    for l in &m.lods {
                        *stats.entry(format!("index size {} vertex size {} uvs {} colors {}", l.index_size, l.vertex_buffer.vertex_size, l.vertex_buffer.num_tex_coords, l.colors.is_some())).or_default() += 1;
                        if l.chunks.iter().any(|c| !c.rigid_vertices.is_empty() || !c.soft_vertices.is_empty()) { *stats.entry("chunks with CPU vertices".into()).or_default() += 1; }
                    }
                }
                Err(err) => {
                    let k = format!("error: {err}");
                    if !stats.contains_key(&k) && let Ok(out) = std::env::var("SKEL_OUT") { std::fs::write(out, &p.data[start..end]).unwrap(); println!("{k}: {} {}", path.display(), p.name(e.object_name)); }
                    *stats.entry(k).or_default() += 1;
                }
            }
        }
    }
    for (k, v) in &stats { println!("{v:6} {k}"); }
}

/// GPU vertex layout hypothesis (TangentX u32, TangentZ u32, 4 bone indices, 4 weights, position 3×f32, UVs) checked
/// on every vertex: weights sum to 255, positions lie in the mesh bounds (+1 uu), bone indices are within the bone map
/// of the vertex's chunk.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn gpu_vertices() {
    use bl2_formats::skeletal::SkeletalMesh;
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "SkeletalMesh" { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            let tags = p.read_tags(&mut r).unwrap();
            let colors = tags.iter().any(|t| p.name(t.name) == "bHasVertexColors" && matches!(p.read_scalar_tag(t), Ok(Some(ScalarValue::Bool(true)))));
            let m = SkeletalMesh::read(&mut r, end, colors).unwrap();
            let (o, ext) = ([m.bounds[0], m.bounds[1], m.bounds[2]], [m.bounds[3], m.bounds[4], m.bounds[5]]);
            for l in &m.lods {
                let vb = &l.vertex_buffer;
                let data = &p.data[vb.vertices.clone()];
                for (i, v) in data.chunks(vb.vertex_size).enumerate() {
                    let weights: u32 = v[12..16].iter().map(|&w| w as u32).sum();
                    *stats.entry(format!("weights sum 255: {}", weights == 255)).or_default() += 1;
                    let pos: Vec<f32> = (0..3).map(|c| f32::from_le_bytes(v[16 + c * 4..20 + c * 4].try_into().unwrap())).collect();
                    let inside = (0..3).all(|c| (pos[c] - o[c]).abs() <= ext[c] + 1.0);
                    *stats.entry(format!("position in bounds: {inside}")).or_default() += 1;
                    let chunk = l.chunks.iter().rev().find(|c| c.base_vertex as usize <= i);
                    let ok = chunk.is_some_and(|c| v[8..12].iter().zip(&v[12..16]).all(|(&b, &w)| w == 0 || (b as usize) < c.bone_map.len()));
                    *stats.entry(format!("bones within chunk bone map: {ok}")).or_default() += 1;
                }
            }
        }
    }
    for (k, v) in &stats { println!("{v:9} {k}"); }
}

/// Native data of every MorphTarget (`skeletal::read_morph_target`).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every package"]
fn morph_targets() {
    let root = root();
    let tree = tree(&root);
    let mut paths = Vec::new(); upks(&root, &mut paths); paths.sort();
    for path in &paths {
        let p = Package::open(path).unwrap();
        for e in p.exports.iter().filter(|e| e.serial_size > 0 && e.class != 0 && e.object_flags & 0x200 == 0) {
            if p.object_name(e.class) != "MorphTarget" { continue; }
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            p.read_object_header(e, &tree, &mut r).unwrap();
            p.read_tags(&mut r).unwrap();
            let lods = bl2_formats::skeletal::read_morph_target(&mut r, end).unwrap();
            println!("{} {}: {:?}", path.file_name().unwrap().to_string_lossy(), p.name(e.object_name), lods.iter().map(|(v, n)| (v.len() / 20, *n)).collect::<Vec<_>>());
        }
    }
}

/// Sequence → LocalAtoms against the original: ANIM_NODES = lines "N <mesh> <sequence nodes on the component>
/// <bAnimRotationOnly> <CurrentTime> <SequenceLength> <NumFrames> <t> <r> <k> <offsets> <stream> <tracks,…> <bones,…>
/// <LocalAtoms>" converted from tools/research/frida-anim-nodes.mjs. For components with one sequence node, every
/// track is decoded at the node's CurrentTime and compared with the LocalAtom of the bone of the same name (rotation
/// always; translation unless the set is rotation-only).
#[test]
#[ignore = "needs a capture from the original (ANIM_NODES)"]
fn sequences_match_local_atoms() {
    use bl2_formats::anim::Sequence;
    let text = std::fs::read_to_string(std::env::var("ANIM_NODES").expect("set ANIM_NODES")).unwrap();
    let hex = |s: &str| if s == "-" { vec![] } else { (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>() };
    let (mut nodes, mut nodes_ok, mut tracks, mut worst_t, mut worst_r) = (0, 0, 0, 0f32, 0f32);
    let mut bad = BTreeMap::<String, usize>::new();
    for l in text.lines() {
        let t: Vec<&str> = l.split(' ').collect();
        if t[2] != "1" { continue; }
        let rotation_only = t[3] != "0";
        // ANIM_NODES_SHIFT: seconds subtracted from CurrentTime (the pose may predate this frame's time advance).
        let shift: f32 = std::env::var("ANIM_NODES_SHIFT").ok().map(|v| v.parse().unwrap()).unwrap_or(0.0);
        let time: f32 = t[4].parse::<f32>().unwrap() - shift;
        let s = Sequence { sequence_length: t[5].parse().unwrap(), num_frames: t[6].parse().unwrap(), translation_format: t[7].parse().unwrap(),
            rotation_format: t[8].parse().unwrap(), key_format: t[9].parse().unwrap(),
            track_offsets: hex(t[10]).chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect(), stream: hex(t[11]) };
        let names: Vec<&str> = if t[12] == "-" { vec![] } else { t[12].split(',').collect() };
        let bones: Vec<&str> = t[13].split(',').collect();
        let local: Vec<f32> = hex(t[14]).chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        nodes += 1;
        let mut node_ok = true;
        for (track, name) in names.iter().enumerate() {
            let Some(b) = bones.iter().position(|n| n == name) else { continue };
            let a = &local[b * 8..b * 8 + 8];
            // The root track comes from GetBoneAtom (no w flip), the others from the batch GetPose (E-076).
            let q = if track == 0 { s.bone_rotation(track, time, true) } else { s.rotation(track, time, true) }.unwrap();
            let dot = (0..4).map(|i| q[i] * a[i]).sum::<f32>().abs();
            let er = 1.0 - dot.min(1.0);
            let et = if rotation_only { 0.0 } else { let p = s.translation(track, time, true).unwrap(); (0..3).map(|i| (p[i] - a[4 + i]).abs()).fold(0.0, f32::max) };
            tracks += 1;
            worst_r = worst_r.max(er);
            worst_t = worst_t.max(et);
            if er > 1e-5 || et > 0.01 { node_ok = false; *bad.entry(format!("{} rotation_only {rotation_only}", t[1])).or_default() += 1; }
            if (er > 1e-5 || et > 0.01) && std::env::var("ANIM_NODES_DEBUG").is_ok_and(|m| m == t[1]) { println!("{} track {track} {name} bone {b}: got {q:?} want {:?} t={time} len {} frames {} keys r {} t {}", t[1], &a[..4], s.sequence_length, s.num_frames, s.track_offsets[track * 4 + 3], s.track_offsets[track * 4 + 1]); }
        }
        if node_ok { nodes_ok += 1; }
    }
    for (k, v) in &bad { println!("{v:5} tracks over tolerance: {k}"); }
    println!("{nodes} single-node components, {nodes_ok} with every track matching; {tracks} tracks, worst translation {worst_t:e} uu, worst 1-|q·q'| {worst_r:e}");
}
