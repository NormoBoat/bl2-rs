//! Wwise Vorbis media of the user's copy (T-020a); prints counts only, writes no decoded audio.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bl2_formats::vorbis::WwiseVorbis;
use bl2_formats::wwise::{Bank, FilePackage, wave_format};

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { files(&p, ext, out) } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) { out.push(p) }
    }
}

/// Every Wwise Vorbis media of every package: (package, stream or embedded, media id, bytes).
fn for_each_media(mut f: impl FnMut(&Path, bool, u32, &[u8])) {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut paths = Vec::new(); files(&root, "pck", &mut paths); paths.sort();
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let pkg = FilePackage::read(&data).unwrap();
        for e in &pkg.streams { if wave_format(&data[e.data.clone()]) == Some(0xffff) { f(path, true, e.id, &data[e.data.clone()]) } }
        for e in &pkg.banks {
            for (id, m) in &Bank::read(&data, e.data.clone()).unwrap().media {
                if wave_format(&data[m.clone()]) == Some(0xffff) { f(path, false, *id, &data[m.clone()]) }
            }
        }
    }
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn headers_and_packets() {
    let mut stats = BTreeMap::<String, usize>::new();
    let mut n = 0;
    for_each_media(|path, stream, id, m| {
        n += 1;
        let v = WwiseVorbis::read(m).unwrap_or_else(|e| panic!("{} {id:#x}: {e}", path.display()));
        let kind = if stream { "stream" } else { "embedded" };
        *stats.entry(format!("{kind} channels={} rate={}", v.channels, v.sample_rate)).or_default() += 1;
        *stats.entry(format!("mod_signal={:#x} blocksizes={}/{} setup_offset={}", v.mod_signal, v.blocksize_0_pow, v.blocksize_1_pow, v.setup_packet_offset)).or_default() += 1;
        let (setup, _) = v.packets(m, v.setup_packet_offset).unwrap();
        let setup_end = setup.first().map(|p| p.end - v.data.start);
        *stats.entry(format!("first audio packet right after setup packet: {}", setup_end == Some(v.first_audio_packet_offset as usize))).or_default() += 1;
        let (audio, cut) = v.packets(m, v.first_audio_packet_offset).unwrap();
        *stats.entry(format!("{kind} data {} declared", if v.data.end <= m.len() { "complete as" } else { "shorter than" })).or_default() += 1;
        *stats.entry(format!("{kind} last packet cut={cut}")).or_default() += 1;
        if audio.is_empty() { *stats.entry(format!("{kind} without audio packets")).or_default() += 1; }
    });
    println!("media={n}");
    for (k, c) in &stats { println!("{c} {k}"); }
}


/// Every Wwise Vorbis media decodes with the codebook library of the user's exe (T-020a).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; decodes every media (several minutes)"]
fn decode_all() {
    use bl2_formats::vorbis::CodebookLibrary;
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let exe = std::fs::read(root.join("Binaries/Win32/Borderlands2.exe")).unwrap();
    let lib = CodebookLibrary::from_exe(&exe).unwrap();
    lib.verify().unwrap();
    println!("codebooks={} library bytes={}", lib.offsets.len() - 1, lib.offsets.last().unwrap() - lib.offsets[0]);
    let mut stats = BTreeMap::<String, usize>::new();
    let (mut samples, mut peak) = (0u64, 0f32);
    for_each_media(|path, stream, id, m| {
        let v = WwiseVorbis::read(m).unwrap();
        match v.decode(m, &lib) {
            Ok(pcm) => {
                samples += pcm.channels[0].len() as u64;
                for c in &pcm.channels { for s in c { peak = peak.max(s.abs()); } }
                *stats.entry(format!("{} decoded{}", if stream { "stream" } else { "embedded" }, if pcm.partial { " (prefetch part)" } else { "" })).or_default() += 1;
            }
            Err(e) => {
                let k = format!("ERROR {e}");
                if !stats.contains_key(&k) { println!("{} {id:#x}: {e}", path.display()); }
                *stats.entry(k).or_default() += 1;
            }
        }
    });
    for (k, c) in &stats { println!("{c} {k}"); }
    println!("samples per channel={samples} peak={peak}");
    assert!(!stats.keys().any(|k| k.starts_with("ERROR")));
}


/// Writes 20 media of different kinds (channels, rate, packet variant, stream/bank) and our PCM for comparison with
/// an independent decoder: VORBIS_SAMPLE_DIR gets <n>.wem and <n>.ours (i16 LE, interleaved).
#[test]
#[ignore = "local verification: VORBIS_SAMPLE_DIR (writes derived data; keep it under .research/)"]
fn write_samples() {
    use bl2_formats::vorbis::CodebookLibrary;
    use std::collections::BTreeSet;
    let dir = PathBuf::from(std::env::var("VORBIS_SAMPLE_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let exe = std::fs::read(root.join("Binaries/Win32/Borderlands2.exe")).unwrap();
    let lib = CodebookLibrary::from_exe(&exe).unwrap();
    let (mut seen, mut n) = (BTreeSet::new(), 0);
    for_each_media(|path, stream, id, m| {
        let v = WwiseVorbis::read(m).unwrap();
        if n >= 20 || v.data.end > m.len() || !seen.insert((stream, v.channels, v.sample_rate / 12000, v.modified_packets())) { return; }
        let pcm = v.decode(m, &lib).unwrap();
        let mut out = Vec::new();
        for i in 0..pcm.channels[0].len() { for c in &pcm.channels { out.extend((((c[i] * 32768.0).clamp(-32768.0, 32767.0)) as i16).to_le_bytes()); } }
        std::fs::write(dir.join(format!("{n:02}.wem")), m).unwrap();
        std::fs::write(dir.join(format!("{n:02}.ours")), out).unwrap();
        println!("{n:02} {} {id:#x} stream={stream} channels={} rate={} modified={} samples={}", path.file_name().unwrap().to_string_lossy(), v.channels, v.sample_rate, v.modified_packets(), pcm.channels[0].len());
        n += 1;
    });
}

/// Compares <n>.ours with <n>.wav written by the independent decoder (16-bit PCM WAV).
#[test]
#[ignore = "local verification: VORBIS_SAMPLE_DIR after write_samples and the independent decoder"]
fn compare_samples() {
    let dir = PathBuf::from(std::env::var("VORBIS_SAMPLE_DIR").unwrap());
    let mut worst = 0;
    for n in 0..20 {
        let ours = std::fs::read(dir.join(format!("{n:02}.ours"))).unwrap();
        let wav = std::fs::read(dir.join(format!("{n:02}.wav"))).unwrap();
        let mut pos = 12;
        let data = loop {
            let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if &wav[pos..pos + 4] == b"data" { break &wav[pos + 8..pos + 8 + size]; }
            pos += 8 + size;
        };
        let s = |b: &[u8]| b.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as i32).collect::<Vec<_>>();
        let (a, b) = (s(&ours), s(data));
        let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).max().unwrap_or(0);
        println!("{n:02}: ours={} independent={} max |diff|={diff}", a.len(), b.len());
        assert_eq!(a.len(), b.len(), "{n:02}: sample count");
        worst = worst.max(diff);
    }
    println!("worst max |diff| = {worst}");
}




/// Cue labels of the media: music markers and FaceFX animations (T-021a, E-070).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn cue_labels_and_facefx() {
    use bl2_formats::wwise::{cue_labels, facefx_animation};
    use std::collections::BTreeSet;
    let mut stats = BTreeMap::<String, usize>::new();
    let (mut ids, mut classes, mut bytes) = (BTreeSet::new(), BTreeMap::<String, usize>::new(), 0usize);
    for_each_media(|path, stream, id, m| {
        let labels = cue_labels(m).unwrap_or_else(|e| panic!("{} {id:#x}: {e}", path.display()));
        let kind = if stream { "stream" } else { "embedded" };
        let mut faces = 0;
        for (_, l) in &labels {
            if let Some(a) = facefx_animation(l) {
                faces += 1;
                assert_eq!(l.len(), 12 + a.len(), "label is exactly the FaceFX block");
                *stats.entry(format!("archive version {}", u32::from_le_bytes(a[4..8].try_into().unwrap()))).or_default() += 1;
                if ids.insert(id) {
                    bytes += a.len();
                    let mut seen = BTreeSet::new();
                    for i in 0..a.len().saturating_sub(6) {
                        let n = u32::from_le_bytes(a[i..i + 4].try_into().unwrap()) as usize;
                        if (4..40).contains(&n) && i + 4 + n <= a.len() && a[i + 4..i + 6] == *b"Fx" && a[i + 4..i + 4 + n].iter().all(|c| c.is_ascii_alphanumeric()) {
                            seen.insert(String::from_utf8_lossy(&a[i + 4..i + 4 + n]).into_owned());
                        }
                    }
                    for c in seen { *classes.entry(c).or_default() += 1; }
                }
            } else if let Some(t) = Some(l.iter().rposition(|&c| c != 0).map_or(&l[..0], |e| &l[..=e])).filter(|t| t.iter().all(|c| c.is_ascii() && *c >= 0x20)) {
                *stats.entry(format!("text label{}", if t.is_empty() { " (empty)" } else if t.starts_with(b"Tempo:") { " Tempo:…" } else { "" })).or_default() += 1;
            } else {
                *stats.entry(format!("OTHER label {:02x?}", &l[..l.len().min(8)])).or_default() += 1;
            }
        }
        *stats.entry(format!("{kind} media with {} FaceFX animation(s)", faces.min(2))).or_default() += 1;
    });
    for (k, c) in &stats { println!("{c} {k}"); }
    println!("distinct media ids with FaceFX: {}, archive bytes {bytes}", ids.len());
    println!("classes named in animation archives (archives naming each): {classes:?}");
    assert!(!stats.keys().any(|k| k.starts_with("OTHER")));
}

/// Impulse responses of the Convolution Reverb plugin parse to the last byte (T-020b, E-071).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn convolution_irs() {
    use bl2_formats::wwise::ConvolutionIr;
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut paths = Vec::new(); files(&root, "pck", &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let pkg = FilePackage::read(&data).unwrap();
        for e in &pkg.banks {
            for (id, m) in &Bank::read(&data, e.data.clone()).unwrap().media {
                let m = &data[m.clone()];
                if m.is_empty() || wave_format(m).is_some() { continue; }
                let ir = ConvolutionIr::read(m).unwrap_or_else(|err| panic!("{} {id:#x}: {err}", path.display()));
                let h = ir.header;
                let f = |i: usize| f32::from_bits(h[i]);
                *stats.entry(format!("block={} fft={} rate={} channels={} h9={} h10={} h11={}", h[0], h[1], h[2], h[3], h[9], h[10], h[11])).or_default() += 1;
                *stats.entry(format!("sum(table) == h8: {}; table[0]={} non-increasing: {}; h4-h7={}", ir.table.iter().map(|&v| v as u32).sum::<u32>() == h[8], ir.table[0], ir.table.windows(2).all(|w| w[0] >= w[1]), h[4] as i64 - h[7] as i64)).or_default() += 1;
                *stats.entry(format!("odd table entries: {}", ir.table.iter().filter(|&&v| v % 2 == 1).count())).or_default() += 1;
                *stats.entry(format!("h5 {:.3} s = {} samples, h6 {:.2}", f(5), (f(5) * h[2] as f32) as u32, f(6))).or_default() += 0;
            }
        }
    }
    for (k, c) in &stats { println!("{c} {k}"); }
}

#[test]
#[ignore = "local investigation: FACEFX_OUT writes the smallest FaceFX animation archive"]
fn smallest_facefx_animation() {
    use bl2_formats::wwise::{cue_labels, facefx_animation};
    let mut best: Option<Vec<u8>> = None;
    for_each_media(|_, _, _, m| {
        for (_, l) in cue_labels(m).unwrap() {
            if let Some(a) = facefx_animation(l) && best.as_ref().is_none_or(|b| a.len() < b.len()) { best = Some(a.to_vec()); }
        }
    });
    let b = best.unwrap();
    println!("smallest animation archive {} bytes", b.len());
    std::fs::write(std::env::var("FACEFX_OUT").unwrap(), b).unwrap();
}

#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn facefx_animations() {
    use bl2_formats::facefx::Archive;
    use bl2_formats::wwise::{cue_labels, facefx_animation};
    use std::collections::{BTreeMap, HashSet};
    let (mut seen, mut stats) = (HashSet::new(), BTreeMap::<String, usize>::new());
    let (mut anims, mut curves, mut keys) = (0, 0, 0);
    for_each_media(|path, _, id, m| {
        for (_, l) in cue_labels(m).unwrap() {
            let Some(a) = facefx_animation(l) else { continue };
            if !seen.insert(a.to_vec()) { continue; }
            let ar = Archive::read(a).unwrap_or_else(|e| { std::fs::write(std::env::var("FACEFX_OUT").unwrap(), a).unwrap(); panic!("{} {id}: {e}", path.display()) });
            let anim = ar.animation().unwrap_or_else(|e| { std::fs::write(std::env::var("FACEFX_OUT").unwrap(), a).unwrap(); panic!("{} {id}: {e}", path.display()) });
            anims += 1; curves += anim.curves.len(); keys += anim.curves.iter().map(|c| c.keys.len()).sum::<usize>();
            *stats.entry(format!("header {} / {} / {} {:?}", ar.licensee, ar.project, ar.licensee_version, ar.flags)).or_default() += 1;
            *stats.entry(format!("classes {:?}", ar.classes.iter().map(|c| format!("{}:{}@{}", c.name, c.version, c.id)).collect::<Vec<_>>())).or_default() += 1;
            *stats.entry(format!("strings_a {:?} word_a {:#x} strings_b {:?} word_b {:#x}", anim.strings_a.iter().map(|s| s.is_empty()).collect::<Vec<_>>(), anim.word_a, anim.strings_b, anim.word_b)).or_default() += 1;
            *stats.entry(format!("bone weights {}", anim.bone_weights.len())).or_default() += 1;
            for c in &anim.curves { *stats.entry(format!("curve word {:#x}", c.word)).or_default() += 1; }
            *stats.entry(format!("blend {} {}", anim.blend_in, anim.blend_out)).or_default() += 1;
            // Animation span (what IsPlayingFaceFXAnim follows) against the length of the sound carrying it.
            let (start, end) = anim.time_range();
            let v = WwiseVorbis::read(m).unwrap();
            let sound = v.sample_count as f32 / v.sample_rate as f32;
            *stats.entry(format!("start {:+.1}", start)).or_default() += 1;
            let d = end - sound;
            *stats.entry(format!("end - sound length {}", if d.abs() < 0.05 { "within 0.05 s".into() } else if d < 0.0 { format!("< 0 by {:.0} s", -d.floor()) } else { format!("> 0 by {:.0} s", d.ceil()) })).or_default() += 1;
            if anim.curves.iter().any(|c| c.keys.windows(2).any(|w| w[1].time < w[0].time)) { *stats.entry("keys out of time order".into()).or_default() += 1; }
        }
    });
    for (k, v) in &stats { println!("{v:6} {k}"); }
    println!("{anims} distinct animations, {curves} curves, {keys} keys");
}

/// Sound length of the media whose FaceFX animation span matches FACEFX_FIND ("start,end;start,end", f32 bits as
/// captured by tools/research/frida-facefx.mjs, compared with 1e-6 tolerance).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL and FACEFX_FIND"]
fn facefx_find_sound() {
    use bl2_formats::facefx::Archive;
    use bl2_formats::wwise::{cue_labels, facefx_animation};
    let want: Vec<(f32, f32)> = std::env::var("FACEFX_FIND").unwrap().split(';').map(|p| { let (a, b) = p.split_once(',').unwrap(); (a.parse().unwrap(), b.parse().unwrap()) }).collect();
    for_each_media(|path, stream, id, m| {
        for (_, l) in cue_labels(m).unwrap() {
            let Some(a) = facefx_animation(l) else { continue };
            let (s, e) = Archive::read(a).unwrap().animation().unwrap().time_range();
            if want.iter().any(|w| (w.0 - s).abs() < 1e-6 && (w.1 - e).abs() < 1e-6) {
                let v = WwiseVorbis::read(m).unwrap();
                println!("span ({s}, {e}) media {id} stream {stream} sound {:.4} s {}", v.sample_count as f32 / v.sample_rate as f32, path.file_name().unwrap().to_string_lossy());
            }
        }
    });
}

/// Effect objects (HIRC types 18/19, `wwise::FxObject`) of every bank: per plugin, parameter block sizes, media counts
/// and the rest bytes; for Convolution Reverb (0x007F0003) the 12 f32 parameters and whether the IR media exist.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn fx_objects() {
    use bl2_formats::wwise::{FxObject, hirc_objects};
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut paths = Vec::new(); files(&root, "pck", &mut paths); paths.sort();
    let mut stats = BTreeMap::<String, usize>::new();
    let mut media_ids = std::collections::HashSet::new();
    let mut conv = Vec::new();
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let pkg = FilePackage::read(&data).unwrap();
        for e in pkg.streams.iter().chain(&pkg.externals) { media_ids.insert(e.id); }
        for e in &pkg.banks {
            let bank = Bank::read(&data, e.data.clone()).unwrap();
            for (id, _) in &bank.media { media_ids.insert(*id); }
            for (tag, range) in &bank.chunks {
                if tag != b"HIRC" { continue; }
                for (kind, id, body) in hirc_objects(&data, range.clone()).unwrap() {
                    if kind != 18 && kind != 19 { continue; }
                    let fx = FxObject::read(&data, body).unwrap();
                    *stats.entry(format!("type {kind} plugin {:#010x} params {} B media {} rest {} B", fx.plugin, fx.params.len(), fx.media.len(), fx.rest.len())).or_default() += 1;
                    if fx.plugin == 0x007f_0003 {
                        let p: Vec<f32> = data[fx.params.clone()].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
                        conv.push((id, p, fx.media.clone()));
                    }
                }
            }
        }
    }
    for (k, v) in &stats { println!("{v:5} {k}"); }
    conv.sort_by_key(|c| c.0); conv.dedup_by_key(|c| c.0);
    for (id, p, m) in &conv { println!("conv {id:#010x} params {p:?} media {:?} present {}", m, m.iter().all(|(_, mid)| media_ids.contains(mid))); }
}
