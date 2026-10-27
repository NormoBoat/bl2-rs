//! Animation sequences (T-036): `AnimSequence` native data and key decoding as done by the original's animation
//! codecs (`AnimationFormat_SetInterfaceLinks` exe 0x5028a0; per-format codec vtables 0x166cb18…; plan/03-evidence.md
//! E-073).

use std::io::{Error, ErrorKind, Result};

use crate::package::Reader;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

/// `AnimationCompressionFormat` (Engine.AnimSequence) in enum order.
pub const COMPRESSION_FORMATS: [&str; 11] = [
    "ACF_None", "ACF_Float96NoW", "ACF_Fixed48NoW", "ACF_IntervalFixed32NoW", "ACF_Fixed32NoW", "ACF_Float32NoW",
    "ACF_Delta40NoW", "ACF_Delta48NoW", "ACF_Identity", "ACF_PolarEncoded32", "ACF_PolarEncoded48",
];
/// `AnimationKeyFormat` in enum order.
pub const KEY_FORMATS: [&str; 3] = ["AKF_ConstantKeyLerp", "AKF_VariableKeyLerp", "AKF_PerTrackCompression"];

/// Native data after the tagged properties: a u32 that is 0 in every cooked sequence (the raw-data array), then
/// `TArray<u8>` CompressedByteStream.
pub fn read_native(r: &mut Reader, end: usize) -> Result<Vec<u8>> {
    let lead = r.u32()?;
    if lead != 0 { return Err(bad(format!("AnimSequence native lead word {lead}"))); }
    let n = r.count()?;
    let stream = r.bytes(n)?.to_vec();
    if r.pos != end { return Err(bad(format!("{} bytes after CompressedByteStream", end as i64 - r.pos as i64))); }
    Ok(stream)
}

/// `floor` as the codecs compute it with SSE: add and subtract 2^23 with the sign of `x` (round to nearest), then
/// subtract 1 when that rounded up.
fn sse_floor(x: f32) -> f32 {
    const MAGIC: f32 = 8_388_608.0;
    let sign = f32::from_bits(x.to_bits() & 0x8000_0000);
    let magic = if (x.abs()) < MAGIC { f32::from_bits(MAGIC.to_bits() | sign.to_bits()) } else { sign };
    let r = (x + magic) - magic;
    if r - x > sign.abs() { r - 1.0 } else { r }
}

/// Key pair and blend alpha for `relative_pos` = time / sequence length (the codecs' inline TimeToIndex, e.g. in
/// 0x51c320): fewer than 2 keys or `relative_pos` ≤ 0 → (0, 0, 0); not looping: ≥ 1 → last key, else
/// position `relative_pos × (keys − 1)`; looping: ≥ 1 → (0, 0, 0), else position `relative_pos × frames`, the
/// frame after the last wraps to (last key, 0), and when keys ≠ frames the frame position is rescaled to
/// `frame_pos / (frames − 1) × (keys − 1)`.
pub fn time_to_index(relative_pos: f32, looping: bool, keys: i32, frames: i32) -> (i32, i32, f32) {
    if keys < 2 || relative_pos <= 0.0 { return (0, 0, 0.0); }
    let last = keys - 1;
    if !looping {
        if relative_pos >= 1.0 { return (last, last, 0.0); }
        let pos = last as f32 * relative_pos;
        let f = sse_floor(pos);
        let i0 = (f as i32).min(last);
        return (i0, (i0 + 1).min(last), pos - f);
    }
    if relative_pos >= 1.0 { return (0, 0, 0.0); }
    let pos = frames as f32 * relative_pos;
    let f = sse_floor(pos);
    let i0 = (f as i32).min(frames - 1);
    let i1 = i0 + 1;
    let alpha = pos - f;
    if i1 == frames { return (last, 0, alpha); }
    if frames == keys { return (i0.clamp(0, last), i1.clamp(0, last), alpha); }
    let pos = pos / (frames - 1) as f32 * last as f32;
    let f = sse_floor(pos);
    let i0 = (f as i32).min(last);
    (i0.clamp(0, last), (i0 + 1).min(last).clamp(0, last), pos - f)
}

/// Compressed animation of one `AnimSequence`: tag values the codecs read and the native byte stream.
#[derive(Debug, Clone, Default)]
pub struct Sequence {
    pub sequence_length: f32,
    pub num_frames: i32,
    /// Indices into [`COMPRESSION_FORMATS`] / [`KEY_FORMATS`].
    pub translation_format: u8,
    pub rotation_format: u8,
    pub key_format: u8,
    /// Per track: translation offset, translation keys, rotation offset, rotation keys (constant/variable key
    /// formats).
    pub track_offsets: Vec<i32>,
    pub stream: Vec<u8>,
}

/// Polar key constants (exe 0x166c9c4…, exact bits): quarter turn with margin, w steps of the 32- and 48-bit forms,
/// row steps of the 32- and 48-bit forms.
const HALF_PI: f32 = f32::from_bits(0x3fc9_309f);
const W32: f32 = f32::from_bits(0x3a00_1002);
const W48: f32 = f32::from_bits(0x3780_0080);
const ROW32: f32 = f32::from_bits(0x3b00_4020);
const ROW48: f32 = f32::from_bits(0x3880_0200);
/// 1/1023 (0x166bcd0); ROW32 is also the 1/511 step of the interval formats.
const STEP1023: f32 = f32::from_bits(0x3a80_2008);
/// 1/32767 (0x165aeac), the z step of `ACF_Delta48NoW` rotations.
const Z32767: f32 = f32::from_bits(0x3800_0100);

/// Direction part shared by the polar keys (0x4f92a0, 0x4f9430): index `i` is a point on the octant of the unit
/// sphere — row `r = ⌊√i⌋` (corrected up when (r+1)² ≤ i), column `i − r²` of `2r`; `X = cos(col/2r · HALF_PI)`,
/// `Z = cos(r · row_step · HALF_PI)`, direction `(X·√(1−Z²), √(1−X²)·√(1−Z²), Z)` scaled to length `√(1 − w²)`;
/// `signs` bits 31/30/29 negate x/y/z. The original uses the MSVC `cosf` (here f64 `cos` rounded to f32).
fn polar(w: f32, i: u32, row_step: f32, signs: u32) -> [f32; 4] {
    let mut row = (i as f64).sqrt() as u32;
    if (row + 1) * (row + 1) <= i { row += 1; }
    let span = (row + 1) * (row + 1) - row * row - 1;
    let a = if span == 0 { 0.0 } else { (i - row * row) as f32 / span as f32 * HALF_PI };
    let cos = |v: f32| (v as f64).cos() as f32;
    let x0 = cos(a);
    let s = (1.0 - x0 * x0).sqrt();
    let z = cos(row as f32 * row_step * HALF_PI);
    let r = (1.0 - z * z).sqrt();
    let (x, y) = (x0 * r, s * r);
    let scale = (1.0 - w * w).sqrt() / (y * y + x * x + z * z).sqrt();
    let mut q = [x * scale, y * scale, z * scale, w];
    for (bit, v) in [(31, 0), (30, 1), (29, 2)] { if signs >> bit & 1 != 0 { q[v] *= -1.0; } }
    q
}

/// `ACF_PolarEncoded32` key (exe 0x4f92a0): bits 0–10 give w = `1 − (k·W32)²` (x87 in the original, here f64),
/// bits 11–28 the direction index, bits 29–31 signs.
pub fn polar32(key: u32) -> [f32; 4] {
    let t = (key & 0x7ff) as f64 * W32 as f64;
    polar((1.0 - t * t) as f32, (key >> 11) & 0x3ffff, ROW32, key)
}

/// `ACF_PolarEncoded48` key (exe 0x4f9430): u16 w = `1 − (k·W48)²` (SSE), then u32 with bits 0–28 the direction
/// index and bits 29–31 signs.
pub fn polar48(key: [u8; 6]) -> [f32; 4] {
    let t = u16::from_le_bytes([key[0], key[1]]) as f32 * W48;
    let d = u32::from_le_bytes([key[2], key[3], key[4], key[5]]);
    polar(1.0 - t * t, d & 0x1fff_ffff, ROW48, d)
}

/// `ACF_Float96NoW` (also every one-key track): x, y, z; w = √(1 − x² − y² − z²), 0 when that is not positive.
fn float96(s: &[u8]) -> [f32; 4] {
    let f = |i: usize| f32::from_le_bytes(s[i * 4..i * 4 + 4].try_into().unwrap());
    let (x, y, z) = (f(0), f(1), f(2));
    let ww = 1.0 - x * x - y * y - z * z;
    [x, y, z, if ww > 0.0 { ww.sqrt() } else { 0.0 }]
}

/// The original's `rsqrtss` estimate: the CPU instruction on x86 (its table differs between CPU vendors), the
/// exact reciprocal square root elsewhere.
fn rsqrt_estimate(l: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    { use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ss, _mm_set_ss}; unsafe { _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(l))) } }
    #[cfg(not(target_arch = "x86_64"))]
    { 1.0 / l.sqrt() }
}

/// Blend of two rotation keys as in the codecs (e.g. 0x51c830): `q0·(1−α) + q1·(α·sign(q0·q1))`, normalised with
/// the rsqrt estimate and one Newton step when the squared length exceeds 1e-8, otherwise identity.
fn blend(q0: [f32; 4], q1: [f32; 4], alpha: f32) -> [f32; 4] {
    let dot = q0[0] * q1[0] + q0[1] * q1[1] + q0[2] * q1[2] + q0[3] * q1[3];
    let b = if dot >= 0.0 { 1.0 } else { -1.0 } * alpha;
    let q: [f32; 4] = std::array::from_fn(|i| q0[i] * (1.0 - alpha) + q1[i] * b);
    let l = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if l <= 1e-8 { return [0.0, 0.0, 0.0, 1.0]; }
    let rs = rsqrt_estimate(l);
    let rs = rs * (3.0 - l * rs * rs) * 0.5;
    q.map(|v| v * rs)
}

/// Per-track key size factors (exe 0x166b8c0: bytes or components by format and component mask, 0x166b868: unit per
/// format). Interval formats keep one u32 per key and the table gives their range data size instead.
const PER_TRACK_SIZE: [[u8; 8]; 11] = [
    [4; 8], [3, 1, 1, 2, 1, 2, 2, 3], [3, 1, 1, 2, 1, 2, 2, 3], [6, 2, 2, 4, 2, 4, 4, 6], [1; 8], [1; 8], [1; 8], [1; 8],
    [0; 8], [1; 8], [1; 8],
];
const PER_TRACK_UNIT: [usize; 11] = [4, 4, 2, 4, 4, 4, 4, 2, 0, 4, 6];

/// A PerTrackCompression track: header u32 at the track's offset — format in bits 28–31, flags in 24–27 (bit 3:
/// a frame table follows the keys, bits 0–2: components present), key count in 0–23 — then the range data (interval
/// formats) and the keys (exe 0x4fd530 / 0x4fe090).
struct PerTrack {
    format: usize,
    flags: u32,
    keys: i32,
    /// Start of the range data; keys follow at `data + range`.
    data: usize,
    range: usize,
    key_size: usize,
}

impl Sequence {
    fn key_bytes(&self, at: usize, n: usize) -> Result<&[u8]> {
        self.stream.get(at..at + n).ok_or_else(|| bad("animation key past CompressedByteStream"))
    }

    fn f32_at(&self, at: usize) -> Result<f32> { Ok(f32::from_le_bytes(self.key_bytes(at, 4)?.try_into().unwrap())) }
    fn u16_at(&self, at: usize) -> Result<u16> { Ok(u16::from_le_bytes(self.key_bytes(at, 2)?.try_into().unwrap())) }

    /// `ACF_Delta48NoW` translation key `k` (constant-key slot 0x518610): min xyz, range xyz, key 0 xyz (f32 each),
    /// then per later key three u16 deltas; key k = key k−1 + (u16·(1/65535)·range + min), summed from key 0
    /// (the original caches partial sums per track, which gives the same values).
    fn delta48_translation(&self, offset: usize, k: i32) -> Result<[f32; 3]> {
        let mut v = [self.f32_at(offset + 24)?, self.f32_at(offset + 28)?, self.f32_at(offset + 32)?];
        for j in 0..k as usize {
            for (c, x) in v.iter_mut().enumerate() {
                let d = self.u16_at(offset + 36 + j * 6 + c * 2)? as f32 * W48 * self.f32_at(offset + 12 + c * 4)? + self.f32_at(offset + c * 4)?;
                *x += d;
            }
        }
        Ok(v)
    }

    /// `ACF_Delta48NoW` rotation key `k` (constant-key slot 0x516d90): min xyz, range xyz, key 0 as a full
    /// quaternion (16 bytes), then per later key u16 x, u16 y (·1/65535) and a u16 with z in bits 0–14 (·1/32767) and the
    /// sign of w in bit 15; each step adds `d·range + min` to the previous key's x/y/z, clamps them to [−1, 1]
    /// (NaN → 1), sets w = ±√(1 − x² − y² − z²) (0 when not positive) and clamps w the same way.
    fn delta48_rotation(&self, offset: usize, k: i32) -> Result<[f32; 4]> {
        let clamp = |x: f32| if -1.0 > x { -1.0 } else if 1.0 > x { x } else { 1.0 };
        let mut q = [self.f32_at(offset + 24)?, self.f32_at(offset + 28)?, self.f32_at(offset + 32)?, self.f32_at(offset + 36)?];
        for j in 0..k as usize {
            let at = offset + 40 + j * 6;
            let zw = self.u16_at(at + 4)?;
            let raw = [self.u16_at(at)? as f32 * W48, self.u16_at(at + 2)? as f32 * W48, (zw & 0x7fff) as f32 * Z32767];
            for (c, d) in raw.into_iter().enumerate() {
                q[c] = clamp(d * self.f32_at(offset + 12 + c * 4)? + self.f32_at(offset + c * 4)? + q[c]);
            }
            let ww = 1.0 - q[0] * q[0] - q[1] * q[1] - q[2] * q[2];
            let w = if ww > 0.0 { ww.sqrt() } else { 0.0 };
            q[3] = clamp(if zw >> 15 != 0 { -w } else { w });
        }
        Ok(q)
    }

    /// Number of tracks: CompressedTrackOffsets holds 4 values per track, 2 for PerTrackCompression.
    pub fn tracks(&self) -> usize { self.track_offsets.len() / if self.key_format == 2 { 2 } else { 4 } }

    fn per_track(&self, offset: i32) -> Result<PerTrack> {
        let o = usize::try_from(offset).map_err(|_| bad("negative per-track offset"))?;
        let h = u32::from_le_bytes(self.key_bytes(o, 4)?.try_into().unwrap());
        let (format, flags, keys) = ((h >> 28) as usize, h >> 24 & 0xf, (h & 0xff_ffff) as i32);
        if format > 10 { return Err(bad(format!("per-track format {format}"))); }
        let table = PER_TRACK_SIZE[format][(flags & 7) as usize] as usize;
        let (range, key_size) = if format == 3 { (table * 4, PER_TRACK_UNIT[3]) } else { (0, table * PER_TRACK_UNIT[format]) };
        Ok(PerTrack { format, flags, keys, data: o + 4, range, key_size })
    }

    /// Key pair and alpha of a per-track track: without a frame table as [`time_to_index`]; with one (flag 8, u8
    /// frames when the sequence has fewer than 256 frames, else u16, 4-byte aligned after the keys) the key whose frame
    /// is the last ≤ the current frame, found from the uniform guess (0x4fd530).
    fn per_track_index(&self, t: &PerTrack, relative_pos: f32, looping: bool) -> Result<(i32, i32, f32)> {
        if t.flags & 8 == 0 { return Ok(time_to_index(relative_pos, looping, t.keys, self.num_frames)); }
        self.variable_index(t.keys, t.data + t.range + t.keys as usize * t.key_size, relative_pos, looping)
    }

    /// Key pair and alpha from a frame table that starts at the next 4-byte boundary after `keys_end` (shared by
    /// VariableKeyLerp, e.g. 0x52ee30, and per-track tracks with flag 8).
    fn variable_index(&self, keys: i32, keys_end: usize, relative_pos: f32, looping: bool) -> Result<(i32, i32, f32)> {
        let last = keys - 1;
        let wrap = if looping { 0 } else { last };
        if keys < 2 || relative_pos <= 0.0 { return Ok((0, 0, 0.0)); }
        if relative_pos >= 1.0 { return Ok((wrap, wrap, 0.0)); }
        let table = (keys_end + 3) & !3;
        let wide = self.num_frames >= 0x100;
        let frame_of = |k: i32| -> Result<i32> {
            Ok(if wide { u16::from_le_bytes(self.key_bytes(table + k as usize * 2, 2)?.try_into().unwrap()) as i32 } else { self.key_bytes(table + k as usize, 1)?[0] as i32 })
        };
        let span = if looping { self.num_frames } else { self.num_frames - 1 };
        let pos = span as f32 * relative_pos;
        let frame = (pos as i32).clamp(0, span - 1);
        let mut k = ((last as f32 * relative_pos) as i32).clamp(0, last);
        let i0 = if frame < frame_of(k)? {
            loop { k -= 1; if k < 1 { break 0; } else if frame_of(k)? <= frame { break k; } }
        } else {
            loop { if k + 1 > last { break last; } else if frame_of(k + 1)? > frame { break k; } k += 1; }
        };
        let i1 = if i0 + 1 > last { wrap } else { i0 + 1 };
        let (f0, f1) = (frame_of(i0)?, frame_of(i1)?);
        Ok((i0, i1, (pos - f0 as f32) / (f1 - f0).max(1) as f32))
    }

    /// Per-track rotation (0x4fd530): offset −1 → identity; formats 1 (three f32), 9, 10; blend as the other codecs
    /// but normalised always, with the squared length summed as (z²+x²)+(w²+y²), `rsqrtps` and the step
    /// `rs + rs·0.5·(1 − rs²·len)`, identity when the length is below 1e-8.
    fn per_track_rotation(&self, offset: i32, relative_pos: f32, looping: bool) -> Result<[f32; 4]> {
        if offset == -1 { return Ok([0.0, 0.0, 0.0, 1.0]); }
        let t = self.per_track(offset)?;
        let (i0, i1, alpha) = self.per_track_index(&t, relative_pos, looping)?;
        let key = |k: i32| -> Result<[f32; 4]> {
            let at = t.data + t.range + k as usize * t.key_size;
            match t.format {
                1 => Ok(float96(self.key_bytes(at, 12)?)),
                8 => Ok([0.0, 0.0, 0.0, 1.0]),
                9 => Ok(polar32(u32::from_le_bytes(self.key_bytes(at, 4)?.try_into().unwrap()))),
                10 => Ok(polar48(self.key_bytes(at, 6)?.try_into().unwrap())),
                f => Err(bad(format!("per-track rotation format {} not decoded yet", COMPRESSION_FORMATS[f]))),
            }
        };
        let mut q = key(i0)?;
        if i0 != i1 {
            let q1 = key(i1)?;
            let dot = q1[0] * q[0] + q1[1] * q[1] + q1[2] * q[2] + q1[3] * q[3];
            let b = alpha * if dot < 0.0 { -1.0 } else { 1.0 };
            let a = 1.0 - alpha;
            q = std::array::from_fn(|i| a * q[i] + q1[i] * b);
        }
        let len = (q[2] * q[2] + q[0] * q[0]) + (q[3] * q[3] + q[1] * q[1]);
        if len < 1e-8 || len.is_nan() { return Ok([0.0, 0.0, 0.0, 1.0]); }
        let rs = rsqrt_estimate(len);
        let rs = (0.0 - (rs * rs * len - 1.0)) * rs * 0.5 + rs;
        Ok(q.map(|v| rs * v))
    }

    /// Per-track translation (0x4fe090): offset −1 → zero; format 1 (f32 for each present component, others 0),
    /// format 3 (range min/extent for each present component, then u32 keys as the constant-key interval format);
    /// blended linearly.
    fn per_track_translation(&self, offset: i32, relative_pos: f32, looping: bool) -> Result<[f32; 3]> {
        if offset == -1 { return Ok([0.0; 3]); }
        let t = self.per_track(offset)?;
        let (i0, i1, alpha) = self.per_track_index(&t, relative_pos, looping)?;
        let f = |at: usize| -> Result<f32> { Ok(f32::from_le_bytes(self.key_bytes(at, 4)?.try_into().unwrap())) };
        let key = |k: i32| -> Result<[f32; 3]> {
            let mut at = t.data + t.range + k as usize * t.key_size;
            let mut v = [0.0; 3];
            match t.format {
                1 => for (c, x) in v.iter_mut().enumerate() { if t.flags >> c & 1 != 0 { *x = f(at)?; at += 4; } },
                3 => {
                    let mut r = t.data;
                    let mut range = [(0.0, 0.0); 3];
                    for (c, m) in range.iter_mut().enumerate() { if t.flags >> c & 1 != 0 { *m = (f(r)?, f(r + 4)?); r += 8; } }
                    let k = u32::from_le_bytes(self.key_bytes(at, 4)?.try_into().unwrap());
                    v = [
                        ((k & 0x3ff) as i32 - 0x1ff) as f32 * ROW32 * range[0].1 + range[0].0,
                        ((k >> 10 & 0x7ff) as i32 - 0x3ff) as f32 * STEP1023 * range[1].1 + range[1].0,
                        ((k >> 21) as i32 - 0x3ff) as f32 * STEP1023 * range[2].1 + range[2].0,
                    ];
                }
                8 => {}
                fm => return Err(bad(format!("per-track translation format {} not decoded yet", COMPRESSION_FORMATS[fm]))),
            }
            Ok(v)
        };
        if i0 == i1 { return key(i0); }
        let (a, b) = (key(i0)?, key(i1)?);
        Ok(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
    }

    /// Key pair and alpha of a constant- or variable-key track whose keys end at `keys_end`.
    fn key_index(&self, keys: i32, keys_end: usize, time: f32, looping: bool) -> Result<(i32, i32, f32)> {
        let rel = time / self.sequence_length;
        match self.key_format {
            0 => Ok(time_to_index(rel, looping, keys, self.num_frames)),
            1 => self.variable_index(keys, keys_end, rel, looping),
            k => Err(bad(format!("key format {}", KEY_FORMATS[k as usize]))),
        }
    }

    /// Rotation of `track` at `time` (seconds), as the codecs' batch `GetPoseRotations` returns it (used for every
    /// track but the root): the sampled quaternion multiplied by (1, 1, 1, −1).
    pub fn rotation(&self, track: usize, time: f32, looping: bool) -> Result<[f32; 4]> {
        let q = self.bone_rotation(track, time, looping)?;
        Ok([q[0], q[1], q[2], -q[3]])
    }

    /// Rotation of `track` at `time` as the single-bone `GetBoneAtom` (0x4fc880 → codec slot 6, e.g. 0x51b840) returns
    /// it, without the w flip; the root track's LocalAtom has this form (E-076).
    pub fn bone_rotation(&self, track: usize, time: f32, looping: bool) -> Result<[f32; 4]> {
        if self.key_format == 2 {
            let o = *self.track_offsets.get(track * 2 + 1).ok_or_else(|| bad("track past CompressedTrackOffsets"))?;
            return self.per_track_rotation(o, time / self.sequence_length, looping);
        }
        let o = self.track_offsets.get(track * 4 + 2..track * 4 + 4).ok_or_else(|| bad("track past CompressedTrackOffsets"))?;
        let (offset, keys) = (o[0] as usize, o[1]);
        let key = |k: i32| -> Result<[f32; 4]> {
            if keys == 1 { return Ok(float96(self.key_bytes(offset, 12)?)); }
            match self.rotation_format {
                9 => Ok(polar32(u32::from_le_bytes(self.key_bytes(offset + k as usize * 4, 4)?.try_into().unwrap()))),
                10 => Ok(polar48(self.key_bytes(offset + k as usize * 6, 6)?.try_into().unwrap())),
                7 => self.delta48_rotation(offset, k),
                f => Err(bad(format!("rotation format {} not decoded yet", COMPRESSION_FORMATS[f as usize]))),
            }
        };
        let size = match self.rotation_format { 9 => 4, 10 => 6, _ => 0 };
        let (i0, i1, alpha) = self.key_index(keys, offset + keys as usize * size, time, looping)?;
        if i0 == i1 { key(i0) } else { Ok(blend(key(i0)?, key(i1)?, alpha)) }
    }

    /// Translation of `track` at `time` (constant-key `GetPoseTranslations`, e.g. 0x50e470): one key → three f32;
    /// more keys → for `ACF_IntervalFixed32NoW` a 24-byte header (min xyz, range xyz) and u32 keys with x in bits
    /// 0–9 (`(v − 511) / 511`), y in bits 10–20 and z in bits 21–31 (`(v − 1023) / 1023`), each `· range + min`;
    /// keys are blended linearly.
    pub fn translation(&self, track: usize, time: f32, looping: bool) -> Result<[f32; 3]> {
        if self.key_format == 2 {
            let o = *self.track_offsets.get(track * 2).ok_or_else(|| bad("track past CompressedTrackOffsets"))?;
            return self.per_track_translation(o, time / self.sequence_length, looping);
        }
        let o = self.track_offsets.get(track * 4..track * 4 + 2).ok_or_else(|| bad("track past CompressedTrackOffsets"))?;
        let (offset, keys) = (o[0] as usize, o[1]);
        let f = |s: &[u8], i: usize| f32::from_le_bytes(s[i * 4..i * 4 + 4].try_into().unwrap());
        let key = |k: i32| -> Result<[f32; 3]> {
            if keys == 1 { let s = self.key_bytes(offset, 12)?; return Ok([f(s, 0), f(s, 1), f(s, 2)]); }
            match self.translation_format {
                3 => {
                    let h = self.key_bytes(offset, 24)?;
                    let v = u32::from_le_bytes(self.key_bytes(offset + 24 + k as usize * 4, 4)?.try_into().unwrap());
                    Ok([
                        ((v & 0x3ff) as i32 - 0x1ff) as f32 * ROW32 * f(h, 3) + f(h, 0),
                        ((v >> 10 & 0x7ff) as i32 - 0x3ff) as f32 * STEP1023 * f(h, 4) + f(h, 1),
                        ((v >> 21) as i32 - 0x3ff) as f32 * STEP1023 * f(h, 5) + f(h, 2),
                    ])
                }
                7 => self.delta48_translation(offset, k),
                fmt => Err(bad(format!("translation format {} not decoded yet", COMPRESSION_FORMATS[fmt as usize]))),
            }
        };
        let (i0, i1, alpha) = self.key_index(keys, offset + 24 + keys as usize * 4, time, looping)?;
        if i0 == i1 { return key(i0); }
        let (a, b) = (key(i0)?, key(i1)?);
        Ok(std::array::from_fn(|i| (b[i] - a[i]) * alpha + a[i]))
    }
}
