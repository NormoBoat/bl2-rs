//! Wwise Convolution Reverb (plugin 0x007F0003, `CAkConvolutionReverbFX`) as in the exe (E-071, E-077):
//! KissFFT real transforms and the uniformly partitioned frequency-domain convolution.

use bl2_formats::wwise::ConvolutionIr;

type C = [f32; 2];

fn add(a: C, b: C) -> C { [a[0] + b[0], a[1] + b[1]] }
fn sub(a: C, b: C) -> C { [a[0] - b[0], a[1] - b[1]] }
fn mul(a: C, b: C) -> C { [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]] }
fn conj(a: C) -> C { [a[0], -a[1]] }

/// `kiss_fftr` state (alloc 0x1357aa0 / 0x1358450): an `n`-point real transform through an `n/2`-point complex one.
pub struct RealFft {
    backward: bool,
    twiddles: Vec<C>,
    super_twiddles: Vec<C>,
    tmp: Vec<C>,
}

impl RealFft {
    pub fn new(n: usize, backward: bool) -> Self {
        let half = n / 2;
        // The exe's only butterfly (0x1357f10) is radix 4.
        assert!(n.is_multiple_of(2) && half >= 4 && half.is_power_of_two() && half.trailing_zeros().is_multiple_of(2), "FFT size {n}: n/2 must be a power of 4");
        let sign = if backward { 1.0 } else { -1.0 };
        // The exe evaluates phases on the x87 and rounds cos/sin to f32; f64 differs only at a near-tie.
        let cexp = |p: f64| [p.cos() as f32, p.sin() as f32];
        let twiddles = (0..half).map(|i| cexp(i as f64 * (sign * std::f64::consts::TAU) / half as f64)).collect();
        let super_twiddles = (0..half / 2).map(|i| cexp(((i + 1) as f64 / half as f64 + 0.5) * (sign * std::f64::consts::PI))).collect();
        Self { backward, twiddles, super_twiddles, tmp: vec![[0.0; 2]; half] }
    }

    /// `kf_work` 0x13582b0 with `kf_bfly4` 0x1357f10 (every factor is 4).
    fn work(twiddles: &[C], backward: bool, out: &mut [C], input: &[C], start: usize, fstride: usize) {
        let m = out.len() / 4;
        if m == 1 {
            for (k, o) in out.iter_mut().enumerate() { *o = input[start + k * fstride]; }
        } else {
            for (k, chunk) in out.chunks_mut(m).enumerate() { Self::work(twiddles, backward, chunk, input, start + k * fstride, fstride * 4); }
        }
        for k in 0..m {
            let s0 = mul(out[k + m], twiddles[k * fstride]);
            let s1 = mul(out[k + 2 * m], twiddles[2 * k * fstride]);
            let s2 = mul(out[k + 3 * m], twiddles[3 * k * fstride]);
            let s5 = sub(out[k], s1);
            out[k] = add(out[k], s1);
            let (s3, s4) = (add(s0, s2), sub(s0, s2));
            out[k + 2 * m] = sub(out[k], s3);
            out[k] = add(out[k], s3);
            let (a, b) = ([s5[0] + s4[1], s5[1] - s4[0]], [s5[0] - s4[1], s5[1] + s4[0]]);
            (out[k + m], out[k + 3 * m]) = if backward { (b, a) } else { (a, b) };
        }
    }

    /// `kiss_fftr` (0x1357bc0): `time` (n samples) → `freq` (n/2 + 1 bins), unnormalised.
    pub fn forward(&mut self, time: &[f32], freq: &mut [C]) {
        assert!(!self.backward);
        let half = self.tmp.len();
        let (input, []) = time.as_chunks::<2>() else { panic!("odd FFT input") };
        Self::work(&self.twiddles, false, &mut self.tmp, input, 0, 1);
        let dc = self.tmp[0];
        freq[0] = [dc[0] + dc[1], 0.0];
        freq[half] = [dc[0] - dc[1], 0.0];
        for k in 1..=half / 2 {
            let (fpk, fpnk) = (self.tmp[k], conj(self.tmp[half - k]));
            let (f1k, tw) = (add(fpk, fpnk), mul(sub(fpk, fpnk), self.super_twiddles[k - 1]));
            freq[k] = [(f1k[0] + tw[0]) * 0.5, (f1k[1] + tw[1]) * 0.5];
            freq[half - k] = [(f1k[0] - tw[0]) * 0.5, (tw[1] - f1k[1]) * 0.5];
        }
    }

    /// `kiss_fftri` (0x1357ce0): `freq` (n/2 + 1 bins; imaginary parts of DC and Nyquist ignored) → `time`, scaled by n.
    pub fn inverse(&mut self, freq: &[C], time: &mut [f32]) {
        assert!(self.backward);
        let half = self.tmp.len();
        self.tmp[0] = [freq[0][0] + freq[half][0], freq[0][0] - freq[half][0]];
        for k in 1..=half / 2 {
            let (fk, fnkc) = (freq[k], conj(freq[half - k]));
            let (fek, fok) = (add(fk, fnkc), mul(sub(fk, fnkc), self.super_twiddles[k - 1]));
            self.tmp[k] = add(fek, fok);
            self.tmp[half - k] = conj(sub(fek, fok));
        }
        let (out, []) = time.as_chunks_mut::<2>() else { panic!("odd FFT output") };
        Self::work(&self.twiddles, true, out, &self.tmp, 0, 1);
    }
}

/// The convolution engine of `CAkConvolutionReverbFX` (`Init` 0x1355ff0, per channel 0x1356160).
pub struct Convolution {
    block: usize,
    table: Vec<u16>,
    /// Bins per slot of the frequency-domain delay line (IR header word 9).
    slot: usize,
    /// Per IR channel: the partition spectra, `table[u]` bins each.
    spectra: Vec<Vec<C>>,
    /// Per processed channel: `table.len()` slots of `slot` bins.
    lines: Vec<Vec<C>>,
    pos: usize,
    forward: RealFft,
    inverse: RealFft,
    time: Vec<f32>,
    freq: Vec<C>,
}

impl Convolution {
    /// `media` is the IR media that `ir` was read from; `channels` is the number of channels to process.
    pub fn new(ir: &ConvolutionIr, media: &[u8], channels: usize) -> Result<Self, String> {
        let h = &ir.header;
        let (block, fft, slot) = (h[0] as usize, h[1] as usize, h[9] as usize);
        if ir.table.is_empty() || 2 * block != fft || slot > fft / 2 || ir.table.iter().any(|&t| t as usize > slot) {
            return Err(format!("unsupported IR: block {block}, FFT {fft}, slot {slot}, table {:?}", &ir.table[..ir.table.len().min(4)]));
        }
        let f32_at = |c: &[u8]| f32::from_le_bytes(c.try_into().unwrap());
        let spectra = ir.channels.iter().map(|r| media[r.clone()].as_chunks::<8>().0.iter().map(|c| [f32_at(&c[..4]), f32_at(&c[4..])]).collect()).collect();
        Ok(Self {
            block, slot, spectra, pos: 0,
            lines: vec![vec![[0.0; 2]; ir.table.len() * slot]; channels],
            table: ir.table.clone(),
            forward: RealFft::new(fft, false),
            inverse: RealFft::new(fft, true),
            time: vec![0.0; fft],
            freq: vec![[0.0; 2]; fft / 2 + 1],
        })
    }

    pub fn block(&self) -> usize { self.block }

    pub fn ir_channels(&self) -> usize { self.spectra.len() }

    /// 0x1356160: convolves the first `valid` samples of `samples` (one block) of `channel` with IR channel
    /// `ir_channel` and overwrites the whole block with this block's output.
    pub fn channel(&mut self, samples: &mut [f32], valid: usize, channel: usize, ir_channel: usize) {
        assert_eq!(samples.len(), self.block);
        let p = self.table.len();
        self.time[..valid].copy_from_slice(&samples[..valid]);
        self.time[valid..].fill(0.0);
        self.forward.forward(&self.time, &mut self.freq);
        let line = &mut self.lines[channel];
        let mut h = &self.spectra[ir_channel][..];
        for (u, &len) in self.table.iter().enumerate() {
            let (part, rest) = h.split_at(len as usize);
            let slot = &mut line[(self.pos + u) % p * self.slot..];
            for ((s, &x), &hk) in slot.iter_mut().zip(&self.freq).zip(part) { *s = add(mul(x, hk), *s); }
            h = rest;
        }
        let current = &mut line[self.pos % p * self.slot..][..self.slot];
        self.freq[..self.slot].copy_from_slice(current);
        self.freq[self.slot..].fill([0.0; 2]);
        current.fill([0.0; 2]);
        self.inverse.inverse(&self.freq, &mut self.time);
        let scale = 1.0 / self.time.len() as f32;
        for (s, &t) in samples.iter_mut().zip(&self.time[self.block..]) { *s = t * scale; }
    }

    /// End of `Execute` (0x1355050): the delay line moves one block.
    pub fn advance(&mut self) { self.pos = (self.pos + 1) % self.table.len(); }
}

/// Per-sample ramp step of the exe's gain ramps: `(new − old) / n` on the x87 (53-bit precision), stored as f32.
fn step(old: f32, new: f32, n: usize) -> f32 { ((new as f64 - old as f64) / n as f64) as f32 }

/// 0x13571a0: `a[i] = a[i]·ga + b[i]·gb`, each gain going from the previous block's value (`.0`) by
/// `step` per sample (constant when neither changed).
pub fn mix(a: &mut [f32], b: &[f32], ga: (f32, f32), gb: (f32, f32)) {
    if ga.0 == ga.1 && gb.0 == gb.1 {
        for (x, &y) in a.iter_mut().zip(b) { *x = *x * ga.1 + y * gb.1; }
        return;
    }
    let (da, db) = (step(ga.0, ga.1, a.len()), step(gb.0, gb.1, a.len()));
    let (mut g, mut h) = (ga.0, gb.0);
    for (x, &y) in a.iter_mut().zip(b) {
        *x = *x * g + y * h;
        (g, h) = (da + g, db + h);
    }
}

/// Width coefficients of 0x13566f0/0x1356af0 for ID 2 (degrees): `c = w/180·0.292894 + 0.707106`, `s = √(1 − c²)`.
fn width_gains(w: f32) -> (f32, f32) {
    let c = w * f32::from_bits(0x3bb6_0b61) * f32::from_bits(0x3e95_f634) + f32::from_bits(0x3f35_04e6);
    let s = 1.0 - c * c;
    (c, if s > 0.0 { s.sqrt() } else { 0.0 })
}

/// 0x13566f0 (0x1356af0 is the same into other buffers): `l' = l·c + r·s`, `r' = r·c + l·s` with the width ramped.
fn width(l: &mut [f32], r: &mut [f32], w: (f32, f32)) {
    let ((mut c, mut s), (c1, s1)) = (width_gains(w.0), width_gains(w.1));
    // Unchanged gains give zero steps, the same arithmetic as the exe's constant-gain loop.
    let (dc, ds) = (step(c, c1, l.len()), step(s, s1, l.len()));
    for (x, y) in l.iter_mut().zip(r.iter_mut()) {
        (*x, *y) = (*y * s + *x * c, *x * s + *y * c);
        (c, s) = (dc + c, ds + s);
    }
}

/// 0x1356fb0 with constant gains: `out = a·g + b·h`.
fn matrix(a: &[f32], b: &[f32], g: f32, h: f32) -> Vec<f32> { a.iter().zip(b).map(|(&x, &y)| x * g + y * h).collect() }

/// Sample delay line (0x1356450 sizes it down to a multiple of 4; 0x1356500 swaps a block through it).
struct Delay { buf: Vec<f32>, pos: usize }

impl Delay {
    fn new(samples: usize) -> Self { Self { buf: vec![0.0; samples / 4 * 4], pos: 0 } }

    fn run(&mut self, s: &mut [f32]) {
        if self.buf.is_empty() { return; }
        for x in s {
            std::mem::swap(x, &mut self.buf[self.pos]);
            self.pos = (self.pos + 1) % self.buf.len();
        }
    }
}

/// Milliseconds → samples as 0x13555b0/0x1355660/0x1354640: `round(rate·ms·0.001)`, ties to even.
fn ms_samples(rate: u32, ms: f32) -> usize { (rate as f64 * ms as f64 * 0.001).round_ties_even() as usize }

/// The 12 plugin parameters in `SetParamsBlock` order (IDs 0, 1, 2, 10, 11, 20, 21, 22, 23, 30, 31, 32) as the
/// effect holds them (0x1355b10): IDs 10…31 converted from dB by `10^(x·0.05f)`.
pub fn params(block: [f32; 12]) -> [f32; 12] {
    let mut p = block;
    for v in &mut p[3..11] { *v = 10f64.powf(*v as f64 * 0.05f32 as f64) as f32; }
    p
}

/// `CAkConvolutionReverbFX` with ID 32 = 0 (every parameter set of the game, E-077): the bus is mixed down to the
/// convolved channels, convolved, pre-delayed and mixed back with stereo width, levels and dry/wet gains.
pub struct ConvolutionReverb {
    conv: Convolution,
    /// Channel mask of the bus (FX+0x13c): bits FL, FR, FC, LFE, BL, BR; channels in bit order, LFE last.
    mask: u32,
    /// Convolved channels (FX+0x140): FL+FR, or FC when the bus is FC/LFE only.
    process: u32,
    rate: u32,
    /// Send-mode effect (FX+0x14e): the dry gain is forced to 0.
    send: bool,
    cur: [f32; 12],
    prev: [f32; 12],
    pre: Vec<Delay>,
    rear: [Delay; 2],
}

impl ConvolutionReverb {
    /// `Init` 0x13546a0; `params` as returned by [`params`].
    pub fn new(ir: &ConvolutionIr, media: &[u8], mask: u32, rate: u32, params: [f32; 12], send: bool) -> Result<Self, String> {
        // shortcut: ID 32 ≠ 0 (per-channel path of Execute) never occurs in the game's data; port it if it shows up.
        if params[11] != 0.0 { return Err(format!("Convolution Reverb ID 32 = {} is not implemented", params[11])); }
        if mask == 0 || mask & !0x3f != 0 { return Err(format!("unsupported channel mask {mask:#x}")); }
        if ir.header[2] != rate { return Err(format!("IR rate {} ≠ bus rate {rate}", ir.header[2])); }
        let process: u32 = if matches!(mask, 4 | 8 | 0xc) { 4 } else { 3 };
        let conv = Convolution::new(ir, media, process.count_ones() as usize)?;
        let pre = (0..process.count_ones()).map(|_| Delay::new(ms_samples(rate, params[0]))).collect();
        let rear = [0x10, 0x20].map(|bit| Delay::new(if mask & bit != 0 { ms_samples(rate, params[1]) } else { 0 }));
        Ok(Self { conv, mask, process, rate, send, cur: params, prev: params, pre, rear })
    }

    /// Samples the effect keeps sounding after its input ends (0x1354640).
    pub fn tail(&self) -> usize { self.conv.table.len() * self.conv.block + ms_samples(self.rate, self.cur[0] + self.cur[1]) }

    /// `Execute` 0x1355050 on one full block: `io` holds the bus channels in mask order, each `block` samples.
    pub fn execute(&mut self, io: &mut [Vec<f32>], params: [f32; 12]) {
        let n = self.conv.block;
        assert!(io.len() == self.mask.count_ones() as usize && io.iter().all(|c| c.len() == n) && params[11] == 0.0);
        // 0x13548f0: new parameters; changed delays are reallocated (cleared).
        self.cur = params;
        if self.cur[0] != self.prev[0] { for d in &mut self.pre { *d = Delay::new(ms_samples(self.rate, self.cur[0])); } }
        if self.cur[1] != self.prev[1] {
            for (d, bit) in self.rear.iter_mut().zip([0x10, 0x20]) { *d = Delay::new(if self.mask & bit != 0 { ms_samples(self.rate, self.cur[1]) } else { 0 }); }
        }
        if self.send { (self.cur[9], self.prev[9]) = (0.0, 0.0); }
        let mut mixed = self.mixdown(io);
        let ir_last = self.conv.ir_channels() - 1;
        for (ch, buf) in mixed.iter_mut().enumerate() {
            self.conv.channel(buf, n, ch, ch.min(ir_last));
            self.pre[ch].run(buf);
        }
        self.upmix(io, &mut mixed);
        self.conv.advance();
        self.prev = self.cur;
    }

    /// 0x1357470: bus → convolved channels. IDs 10/11 (input center/LFE level) enter squared.
    fn mixdown(&self, io: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let (mask, n) = (self.mask, self.conv.block);
        let mut out = vec![vec![0.0f32; n]; self.process.count_ones() as usize];
        if matches!(mask, 3 | 4 | 8) {
            for (o, i) in out.iter_mut().zip(io) { o.copy_from_slice(i); }
            return out;
        }
        let (p, c) = (&self.prev, &self.cur);
        let mut count = (mask & 0x33).count_ones() as f32;
        let (mut c0, mut c1, mut l0, mut l1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        if mask & 4 != 0 {
            if mask & 7 == 7 { (c0, c1) = (p[3] * p[3], c[3] * c[3]); } else { count += 1.0; }
        }
        if mask & 8 != 0 { (l0, l1) = (p[4] * p[4], c[4] * c[4]); }
        let norm = |l: f32, c: f32| (1.0 / ((l as f64 + c as f64) + count as f64)).sqrt();
        let (n0, n1) = (norm(l0, c0), norm(l1, c1));
        let (g0, dn) = (n0 as f32, ((n1 - n0) / n as f64) as f32);
        // out[i] = in[i]·g (+ out[i]) with g ramping from `g` by `d`
        let add = |o: &mut [f32], i: &[f32], mut g: f32, d: f32, acc: bool| for (o, &x) in o.iter_mut().zip(i) {
            *o = if acc { x * g + *o } else { x * g };
            g += d;
        };
        let k = (mask & !8).count_ones().min(2) as usize;
        for ch in 0..k { add(&mut out[ch], &io[ch], g0, dn, false); }
        if mask & 7 == 7 { for o in out.iter_mut().take(2) { add(o, &io[2], c0, step(c0, c1, n), true); } }
        if mask & 0x30 != 0 {
            let src = (((mask & 4) | 8) >> 2) as usize;
            for (ch, o) in out.iter_mut().take(2).enumerate() { add(o, &io[src + ch], g0, dn, true); }
        }
        if mask & 8 != 0 {
            let lfe = &io[io.len() - 1];
            for o in out.iter_mut().take(k) { add(o, lfe, l0, step(l0, l1, n), true); }
        }
        out
    }

    /// 0x1354a50: convolved channels → bus, `bus = bus·dry + wet·level·conv`. Levels: ID 20 front (and center
    /// when the bus has no L/R), 21 rear, 22 center, 23 LFE; ID 2 stereo width.
    fn upmix(&mut self, io: &mut [Vec<f32>], mixed: &mut [Vec<f32>]) {
        let (p, c, mask) = (self.prev, self.cur, self.mask);
        let dry = (p[9], c[9]);
        let wet = |level: usize| (p[10] * p[level], c[10] * c[level]);
        let w = (p[2], c[2]);
        match mask {
            8 => return mix(&mut io[0], &mixed[0], dry, wet(8)),
            4 => return mix(&mut io[0], &mixed[0], dry, wet(5)),
            _ => {}
        }
        match mask & 7 {
            3 => {
                let (mut l, mut r) = (mixed[0].clone(), mixed[1].clone());
                width(&mut l, &mut r, w);
                mix(&mut io[0], &l, dry, wet(5));
                mix(&mut io[1], &r, dry, wet(5));
            }
            4 => mix(&mut io[0], &mixed[0], dry, wet(5)),
            7 => {
                let (g, h, k) = (f32::from_bits(0x3ee8_b439), f32::from_bits(0x3f62_0c4a), f32::from_bits(0xbdef_9db2));
                mix(&mut io[2], &matrix(&mixed[0], &mixed[1], g, g), dry, wet(7));
                let (mut l, mut r) = (matrix(&mixed[0], &mixed[1], h, k), matrix(&mixed[1], &mixed[0], h, k));
                width(&mut l, &mut r, w);
                mix(&mut io[0], &l, dry, wet(5));
                mix(&mut io[1], &r, dry, wet(5));
            }
            _ => {}
        }
        if mask & 0x30 != 0 {
            let (mut l, mut r) = (mixed[0].clone(), mixed[1].clone());
            self.rear[0].run(&mut l);
            self.rear[1].run(&mut r);
            width(&mut l, &mut r, w);
            let at = ((mask >> 2) & 1 | 2) as usize;
            mix(&mut io[at], &l, dry, wet(6));
            mix(&mut io[at + 1], &r, dry, wet(6));
        }
        if mask & 8 != 0 {
            let last = io.len() - 1;
            mix(&mut io[last], &mixed[0], dry, wet(8));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 23) as f32 - 1.0
    }

    /// Real IDFT of bins 0..=n/2 in f64, ignoring the imaginary parts of DC and Nyquist like `kiss_fftri`, unscaled.
    fn idft(freq: &[[f64; 2]], n: usize) -> Vec<f64> {
        (0..n).map(|t| {
            let mut s = freq[0][0] + freq[n / 2][0] * if t % 2 == 0 { 1.0 } else { -1.0 };
            for (k, f) in freq.iter().enumerate().take(n / 2).skip(1) {
                let w = std::f64::consts::TAU * (k * t) as f64 / n as f64;
                s += 2.0 * (f[0] * w.cos() - f[1] * w.sin());
            }
            s
        }).collect()
    }

    #[test]
    fn real_fft_matches_dft() {
        let n = 128;
        let mut seed = 1;
        let x: Vec<f32> = (0..n).map(|_| noise(&mut seed)).collect();
        let mut freq = vec![[0.0; 2]; n / 2 + 1];
        RealFft::new(n, false).forward(&x, &mut freq);
        for (k, f) in freq.iter().enumerate() {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (t, &v) in x.iter().enumerate() {
                let w = -std::f64::consts::TAU * (k * t) as f64 / n as f64;
                (re, im) = (re + v as f64 * w.cos(), im + v as f64 * w.sin());
            }
            assert!((f[0] as f64 - re).abs() < 1e-4 && (f[1] as f64 - im).abs() < 1e-4, "bin {k}: {f:?} vs {re} {im}");
        }
        let mut back = vec![0.0; n];
        RealFft::new(n, true).inverse(&freq, &mut back);
        for (b, v) in back.iter().zip(&x) { assert!((b / n as f32 - v).abs() < 1e-5); }
    }

    /// Partitioned convolution against an f64 evaluation of the same frequency-domain sums.
    #[test]
    fn convolution_matches_reference() {
        let (block, fft, slot, table) = (16usize, 32usize, 16usize, vec![16u16, 10, 6]);
        let mut seed = 7;
        let total: usize = table.iter().map(|&t| t as usize).sum();
        let spectrum: Vec<C> = (0..total).map(|_| [noise(&mut seed), noise(&mut seed)]).collect();
        let mut media = Vec::new();
        for c in &spectrum { media.extend(c[0].to_le_bytes()); media.extend(c[1].to_le_bytes()); }
        let mut header = [0; 12];
        (header[0], header[1], header[3], header[7], header[8], header[9]) = (block as u32, fft as u32, 1, 3, total as u32, slot as u32);
        let ir = ConvolutionIr { header, table: table.clone(), channels: std::iter::once(0..media.len()).collect() };
        let mut conv = Convolution::new(&ir, &media, 1).unwrap();
        let blocks: Vec<Vec<f32>> = (0..6).map(|_| (0..block).map(|_| noise(&mut seed)).collect()).collect();
        let spectra_f64: Vec<Vec<[f64; 2]>> = blocks.iter().map(|x| (0..=fft / 2).map(|k| {
            x.iter().enumerate().fold([0.0, 0.0], |[re, im], (t, &v)| {
                let w = -std::f64::consts::TAU * (k * t) as f64 / fft as f64;
                [re + v as f64 * w.cos(), im + v as f64 * w.sin()]
            })
        }).collect()).collect();
        for (b, x) in blocks.iter().enumerate() {
            let mut y = x.clone();
            conv.channel(&mut y, block, 0, 0);
            conv.advance();
            let mut acc = vec![[0.0f64; 2]; fft / 2 + 1];
            let mut start = 0;
            for (u, &len) in table.iter().enumerate() {
                if u <= b {
                    for k in 0..len as usize {
                        let (xk, hk) = (spectra_f64[b - u][k], spectrum[start + k]);
                        acc[k][0] += xk[0] * hk[0] as f64 - xk[1] * hk[1] as f64;
                        acc[k][1] += xk[0] * hk[1] as f64 + xk[1] * hk[0] as f64;
                    }
                }
                start += len as usize;
            }
            let reference = idft(&acc, fft);
            for (j, &v) in y.iter().enumerate() {
                let r = reference[block + j] / fft as f64;
                assert!((v as f64 - r).abs() < 1e-5, "block {b} sample {j}: {v} vs {r}");
            }
        }
    }

    #[test]
    fn mix_ramps_to_the_new_gains() {
        let dry = vec![1.0; 10];
        let mut wet = vec![0.0; 10];
        mix(&mut wet, &dry, (1.0, 1.0), (0.0, 1.0));
        for (i, w) in wet.iter().enumerate() { assert!((w - i as f32 / 10.0).abs() < 1e-6, "{i}: {w}"); }
        let mut wet = vec![2.0; 3];
        mix(&mut wet, &dry[..3], (0.5, 0.5), (0.25, 0.25));
        assert_eq!(wet, [1.25; 3]);
    }

    #[test]
    fn width_spreads_and_ramps() {
        let (mut l, mut r) = (vec![1.0f32; 4], vec![0.0f32; 4]);
        width(&mut l, &mut r, (0.0, 0.0));
        let (c, s) = width_gains(0.0);
        assert_eq!(c, f32::from_bits(0x3f35_04e6)); // 0.707106, not 1/√2
        assert!((s - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert!(l.iter().all(|&v| v == c) && r.iter().all(|&v| v == s));
        let (mut l, mut r) = (vec![1.0f32; 4], vec![0.0f32; 4]);
        width(&mut l, &mut r, (0.0, 180.0));
        assert_eq!((l[0], r[0]), (c, s));
        assert!(l.windows(2).all(|w| w[1] > w[0]) && r.windows(2).all(|w| w[1] < w[0]));
    }

    fn test_ir(seed: &mut u32) -> (ConvolutionIr, Vec<u8>) {
        let (block, fft, slot, table) = (16u32, 32u32, 16u32, vec![16u16, 8]);
        let mut media = Vec::new();
        for _ in 0..24 * 2 { media.extend(noise(seed).to_le_bytes()); }
        let mut header = [0; 12];
        (header[0], header[1], header[2], header[3], header[7], header[8], header[9]) = (block, fft, 48000, 1, 2, 24, slot);
        (ConvolutionIr { header, table, channels: std::iter::once(0..media.len()).collect() }, media)
    }

    /// Stereo bus, width 180°, 0 dB everywhere, pre-delay of one block: out = in + conv(in) one block later.
    #[test]
    fn stereo_reverb_is_dry_plus_delayed_convolution() {
        let mut seed = 3;
        let (ir, media) = test_ir(&mut seed);
        let pre_ms = 16.0 * 1000.0 / 48000.0;
        let p = params([pre_ms, 0.0, 180.0, 0.0, -96.0, 0.0, 0.0, -12.0, -96.0, 0.0, 0.0, 0.0]);
        let mut fx = ConvolutionReverb::new(&ir, &media, 3, 48000, p, false).unwrap();
        let mut conv = Convolution::new(&ir, &media, 2).unwrap();
        let mut late = vec![vec![0.0; 16]; 2];
        for _ in 0..4 {
            let input: Vec<Vec<f32>> = (0..2).map(|_| (0..16).map(|_| noise(&mut seed)).collect()).collect();
            let mut io = input.clone();
            fx.execute(&mut io, p);
            for ch in 0..2 {
                let mut c = input[ch].clone();
                conv.channel(&mut c, 16, ch, 0);
                for i in 0..16 {
                    let want = input[ch][i] + late[ch][i];
                    assert!((io[ch][i] - want).abs() < 1e-5, "ch {ch} sample {i}: {} vs {want}", io[ch][i]);
                }
                late[ch] = c;
            }
            conv.advance();
        }
        assert_eq!(fx.tail(), 2 * 16 + 16);
    }

    /// 5.1 bus: the convolved channels get (L, R) + c²·C + (BL, BR), scaled by √(1/(lfe² + c² + 4)), plus lfe²·LFE.
    #[test]
    fn surround_mixdown_weights() {
        let mut seed = 5;
        let (ir, media) = test_ir(&mut seed);
        let p = params([0.0, 0.0, 180.0, -6.0, -12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let fx = ConvolutionReverb::new(&ir, &media, 0x3f, 48000, p, false).unwrap();
        let io: Vec<Vec<f32>> = (0..6).map(|_| (0..16).map(|_| noise(&mut seed)).collect()).collect();
        let out = fx.mixdown(&io);
        let (c2, l2) = (p[3] * p[3], p[4] * p[4]);
        let norm = (1.0 / (l2 + c2 + 4.0)).sqrt();
        for i in 0..16 {
            for (ch, rear) in [(0, 3), (1, 4)] {
                let want = io[ch][i] * norm + io[2][i] * c2 + io[rear][i] * norm + io[5][i] * l2;
                assert!((out[ch][i] - want).abs() < 1e-6);
            }
        }
    }
}
