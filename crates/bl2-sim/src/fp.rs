//! Float behaviour of the original's SSE code (plan/14-decisions.md D-007, plan/03-evidence.md E-023).

/// SSE `cvttss2si`: truncation; NaN and out-of-range give the "integer indefinite" `i32::MIN`.
pub fn cvttss2si(x: f32) -> i32 {
    if x.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&x) { i32::MIN } else { x as i32 }
}

/// SSE `cvtss2si` with the default MXCSR rounding (nearest, ties to even).
pub fn cvtss2si(x: f32) -> i32 { cvttss2si(x.round_ties_even()) }

// orig: UObject::execRound 0x451bd0, execFFloor 0x451c40, execFCeil 0x451cb0 (E-023).
pub fn round(x: f32) -> i32 { cvtss2si(x * 2.0 + 0.5) >> 1 }
pub fn floor(x: f32) -> i32 { cvtss2si(x * 2.0 - 0.5) >> 1 }
pub fn ceil(x: f32) -> i32 { -(cvtss2si(-0.5 - x * 2.0) >> 1) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_match_sse() {
        assert_eq!((cvttss2si(-1.9), cvttss2si(1.9), cvttss2si(f32::NAN), cvttss2si(3e9), cvttss2si(-2_147_483_648.0)), (-1, 1, i32::MIN, i32::MIN, i32::MIN));
        assert_eq!((cvtss2si(0.5), cvtss2si(1.5), cvtss2si(-0.5), cvtss2si(2.5)), (0, 2, 0, 2));
    }

    #[test]
    fn round_floor_ceil() {
        // The 2x±0.5 trick rounds halves up (round) and is exact floor/ceil for these values.
        for (x, r, f, c) in [(0.5f32, 1, 0, 1), (1.5, 2, 1, 2), (-0.5, 0, -1, 0), (-1.5, -1, -2, -1), (2.0, 2, 2, 2), (-2.25, -2, -3, -2), (0.0, 0, 0, 0)] {
            assert_eq!((round(x), floor(x), ceil(x)), (r, f, c), "{x}");
        }
    }
}
