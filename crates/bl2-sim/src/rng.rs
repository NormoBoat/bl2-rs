//! Random generators of the original (plan/03-evidence.md E-024). Seeding: `srand(QueryPerformanceCounter)`
//! at engine init, or 0 for both generators with `-FIXEDSEED`.

use crate::fp::cvttss2si;

/// MSVCR100 `rand()`: per-thread LCG (decompiled from msvcr100.dll, E-024).
#[derive(Clone, Debug)]
pub struct CrtRand {
    holdrand: u32,
}

impl CrtRand {
    /// `srand(seed)`.
    pub fn new(seed: u32) -> Self { Self { holdrand: seed } }

    pub fn rand(&mut self) -> i32 {
        self.holdrand = self.holdrand.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        (self.holdrand >> 16 & 0x7FFF) as i32
    }

    // orig: UObject::execFRand 0x451d20: float(rand()) * f32::from_bits(0x38000100) (≈1/32767), range [0, 1].
    pub fn frand(&mut self) -> f32 { self.rand() as f32 * f32::from_bits(0x3800_0100) }

    // orig: UObject::execRand 0x455090: truncation of rand() * 2^-15 * max; 0 when max <= 0.
    pub fn rand_max(&mut self, max: i32) -> i32 {
        if max <= 0 { return 0; }
        cvttss2si(self.rand() as f32 * f32::from_bits(0x3800_0000) * max as f32)
    }

    // orig: UObject::execRotRand 0x456f50: rand() order is yaw, pitch, then roll. Returns (pitch, yaw, roll).
    pub fn rot_rand(&mut self, roll: bool) -> (i32, i32, i32) {
        let unit = |r: &mut Self| cvttss2si(r.rand() as f32 * f32::from_bits(0x3800_0000) * 65536.0);
        let yaw = unit(self);
        let pitch = unit(self);
        (pitch, yaw, if roll { unit(self) } else { 0 })
    }
}

/// Second global LCG (UE3 `appSRand`-like, seed at 0x19e806c), or a per-object seed (E-024).
pub fn srand_next(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(0x0BB3_8435).wrapping_add(0x3619_636B);
    // orig: 0x401060 / 0x7ab7c0: mantissa bits under 1.0f, minus the truncated integer part (1.0).
    let f = f32::from_bits(*seed & 0x7F_FFFF | 0x3F80_0000);
    f - cvttss2si(f) as f32
}

/// Uniform value in [min, max] from [`srand_next`]: 0x7ab7c0 evaluates `(max - min) * r + min` on the x87 stack
/// (53-bit precision, E-023), rounding to f32 only when the caller stores it — emulated with f64.
pub fn srand_range(seed: &mut u32, min: f32, max: f32) -> f32 {
    let r = srand_next(seed);
    ((max as f64 - min as f64) * r as f64 + min as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousand_algorithm_reference_vectors() {
        // E-024: generated separately with BigInt modular arithmetic and a rational float conversion.
        // This checks the recovered algorithms, not RNG consumers or the original's thread scheduling.
        let vectors = include_str!("../../../plan/evidence/raw/T-012-rng-vectors.csv");
        let (mut zero, mut one, mut seed) = (CrtRand::new(0), CrtRand::new(1), 0);
        let mut count = 0;
        for line in vectors.lines().skip(1) {
            let values: Vec<u32> = line.split(',').map(|s| s.parse().unwrap()).collect();
            assert_eq!(values.len(), 4);
            let actual = [zero.rand() as u32, one.rand() as u32, srand_next(&mut seed).to_bits()];
            assert_eq!([actual[0], actual[1], seed, actual[2]], values.as_slice(), "step {count}");
            count += 1;
        }
        assert_eq!(count, 1000);
    }

    #[test]
    fn msvc_rand_reference_sequence() {
        // Published MSVC CRT sequence for srand(1).
        let mut r = CrtRand::new(1);
        assert_eq!((0..10).map(|_| r.rand()).collect::<Vec<_>>(), [41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962, 24464]);
    }

    #[test]
    fn native_formulas() {
        assert_eq!(f32::from_bits(0x3800_0100), 1.0 / 32767.0); // FRand reaches exactly 1.0
        assert_eq!(32767.0f32 * f32::from_bits(0x3800_0100), 1.0);
        let mut r = CrtRand::new(1);
        assert_eq!(r.frand().to_bits(), (41.0f32 * f32::from_bits(0x3800_0100)).to_bits());
        let mut r = CrtRand::new(1);
        assert_eq!((r.rand_max(100), r.rand_max(100), r.rand_max(0)), (0, 56, 0)); // 41/32768*100, 18467/32768*100
        let mut r = CrtRand::new(1);
        assert_eq!(r.rot_rand(true), (18467 * 2, 41 * 2, 6334 * 2));
    }

    #[test]
    fn second_lcg() {
        let mut seed = 0;
        let a = srand_next(&mut seed);
        assert_eq!(seed, 0x3619_636B);
        assert_eq!(a, f32::from_bits(0x3F80_0000 | 0x19_636B) - 1.0);
        assert!((0.0..1.0).contains(&a));
        let mut s = 0;
        let v = srand_range(&mut s, 10.0, 20.0);
        assert_eq!(v, ((10.0f64 * a as f64) + 10.0) as f32);
    }
}
