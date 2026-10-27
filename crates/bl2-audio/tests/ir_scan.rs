//! Convolution Reverb on the user's IRs (needs `BL2_INSTALL`).

use std::path::{Path, PathBuf};

use bl2_audio::convolution::Convolution;
use bl2_formats::wwise::{Bank, ConvolutionIr, FilePackage, wave_format};

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { files(&p, ext, out) } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) { out.push(p) }
    }
}

/// Impulse response of every IR, partition by partition, against an f64 inverse DFT of that partition's spectrum
/// (an impulse has a flat spectrum, so output block u is the second half of IDFT(H_u) / FFT size).
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn impulse_responses_match_partitions() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut paths = Vec::new();
    files(&root, "pck", &mut paths);
    paths.sort();
    let (mut irs, mut worst) = (0, 0.0f64);
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let pkg = FilePackage::read(&data).unwrap();
        for e in &pkg.banks {
            for (_, m) in &Bank::read(&data, e.data.clone()).unwrap().media {
                let m = &data[m.clone()];
                if m.is_empty() || wave_format(m).is_some() { continue; }
                let ir = ConvolutionIr::read(m).unwrap();
                let (block, fft) = (ir.header[0] as usize, ir.header[1] as usize);
                let (cos, sin): (Vec<f64>, Vec<f64>) = (0..fft).map(|i| { let w = std::f64::consts::TAU * i as f64 / fft as f64; (w.cos(), w.sin()) }).unzip();
                let p = ir.table.len();
                let mut conv = Convolution::new(&ir, m, ir.channels.len()).unwrap();
                let h: Vec<Vec<[f64; 2]>> = ir.channels.iter().map(|r| m[r.clone()].as_chunks::<8>().0.iter().map(|b| [0, 4].map(|o| f32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as f64)).collect()).collect();
                let mut start = 0;
                for (u, &len) in ir.table.iter().enumerate() {
                    for (c, h) in h.iter().enumerate() {
                        let mut y = vec![0.0f32; block];
                        if u == 0 { y[0] = 1.0; }
                        conv.channel(&mut y, block, c, c);
                        if u >= 3 && u + 1 != p { continue; }
                        let part = &h[start..start + len as usize];
                        for (j, &v) in y.iter().enumerate() {
                            let t = block + j;
                            let mut s = part[0][0];
                            for (k, hk) in part.iter().enumerate().skip(1) { let i = k * t % fft; s += 2.0 * (hk[0] * cos[i] - hk[1] * sin[i]); }
                            let r = s / fft as f64;
                            worst = worst.max((v as f64 - r).abs());
                            assert!((v as f64 - r).abs() < 1e-5, "{} IR channel {c} partition {u} sample {j}: {v} vs {r}", path.display());
                        }
                    }
                    conv.advance();
                    start += len as usize;
                }
                // The response ends after the last partition.
                for c in 0..h.len() {
                    let mut y = vec![0.0f32; block];
                    conv.channel(&mut y, block, c, c);
                    assert!(y.iter().all(|&v| v == 0.0));
                }
                irs += 1;
            }
        }
    }
    println!("{irs} IRs, worst |engine − f64| = {worst:e}");
    assert!(irs > 0);
}

/// Every Convolution Reverb parameter set of the game whose IR is installed, on stereo, 5.1 and mono buses: the
/// impulse response is finite and silent once `tail()` has passed.
#[test]
#[ignore = "needs the user's Borderlands 2 copy in BL2_INSTALL; reads every .pck"]
fn game_parameter_sets_run() {
    use bl2_audio::convolution::{ConvolutionReverb, params};
    use bl2_formats::wwise::{FxObject, hirc_objects};
    use std::collections::{BTreeMap, HashMap};
    let root = PathBuf::from(std::env::var("BL2_INSTALL").unwrap());
    let mut paths = Vec::new();
    files(&root, "pck", &mut paths);
    paths.sort();
    let (mut media, mut sets) = (HashMap::<u32, Vec<u8>>::new(), BTreeMap::<u32, ([f32; 12], u32)>::new());
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        for e in &FilePackage::read(&data).unwrap().banks {
            let bank = Bank::read(&data, e.data.clone()).unwrap();
            for (id, m) in &bank.media {
                if !m.is_empty() && wave_format(&data[m.clone()]).is_none() { media.insert(*id, data[m.clone()].to_vec()); }
            }
            for (tag, range) in &bank.chunks {
                if tag != b"HIRC" { continue; }
                for (kind, id, body) in hirc_objects(&data, range.clone()).unwrap() {
                    if kind != 18 && kind != 19 { continue; }
                    let fx = FxObject::read(&data, body).unwrap();
                    if fx.plugin != 0x007f_0003 { continue; }
                    let block: [f32; 12] = std::array::from_fn(|i| f32::from_le_bytes(data[fx.params.start + 4 * i..][..4].try_into().unwrap()));
                    sets.insert(id, (block, fx.media[0].1));
                }
            }
        }
    }
    let mut ran = 0;
    for (id, (block, ir_id)) in &sets {
        let Some(m) = media.get(ir_id) else { continue };
        let ir = ConvolutionIr::read(m).unwrap();
        let p = params(*block);
        for mask in [3u32, 0x3f, 4] {
            let mut fx = ConvolutionReverb::new(&ir, m, mask, ir.header[2], p, true).unwrap();
            let n = ir.header[0] as usize;
            let blocks = fx.tail().div_ceil(n) + 1;
            let mut energy = 0.0f64;
            for b in 0..blocks + 2 {
                let mut io = vec![vec![0.0f32; n]; mask.count_ones() as usize];
                if b == 0 { io[0][0] = 1.0; }
                fx.execute(&mut io, p);
                assert!(io.iter().flatten().all(|v| v.is_finite()), "{id:#x} mask {mask:#x}");
                if b < blocks { energy += io.iter().flatten().map(|&v| v as f64 * v as f64).sum::<f64>(); }
                else { assert!(io.iter().flatten().all(|&v| v == 0.0), "{id:#x} mask {mask:#x}: sound after the tail"); }
            }
            assert!(energy > 0.0, "{id:#x} mask {mask:#x}: silent");
        }
        ran += 1;
    }
    println!("{ran} of {} parameter sets have their IR installed and ran on 3 buses", sets.len());
    assert!(ran > 0);
}
