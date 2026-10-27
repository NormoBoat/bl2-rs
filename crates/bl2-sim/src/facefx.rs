//! FaceFX (T-021b, plan/03-evidence.md E-072): the playback clock — what `IsPlayingFaceFXAnim` reports and when the
//! line's sound starts (the original's `FxActorInstance` tick, exe 0x14e5820, as driven by `USkeletalMeshComponent`'s
//! FaceFX update 0xa2d330) — and the face graph: curve values (0x14ed2d0) and compiled graph evaluation (0x14ea0d0).
//! The original evaluates on the x87 FPU with stores to f32 at the points mirrored here; intermediate extended
//! precision is not reproduced (outputs drive bones and morphs only).

use bl2_formats::facefx::{Curve, Node};

/// Curve value at `time` (0x14ed2d0): first/last key value outside the keys, otherwise Hermite between the
/// surrounding keys using the left key's slope out and the right key's slope in (`word` 0), linear (`word` 1),
/// 0 for other interpolations; 0 without keys.
pub fn curve_value(curve: &Curve, time: f32) -> f32 {
    let keys = &curve.keys;
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return 0.0 };
    if time <= first.time || keys.len() == 1 { return first.value; }
    if last.time <= time { return last.value; }
    let i = keys.windows(2).position(|w| w[0].time <= time && time < w[1].time).unwrap_or(keys.len() - 2);
    let (a, b) = (keys[i], keys[i + 1]);
    let dt = b.time - a.time;
    let u = (time - a.time) / dt;
    match curve.word {
        0 => {
            let m0 = a.slope_out * dt;
            u * (u * (u * (dt * b.slope_in + m0 + (a.value * 2.0 - b.value * 2.0)) + (((b.value * 3.0 - a.value * 3.0) - m0 * 2.0) - dt * b.slope_in)) + m0) + a.value
        }
        1 => u * (b.value - a.value) + a.value,
        _ => 0.0,
    }
}

/// Link function `function` applied to `x` (the switch in 0x14ea0d0). Corrective (9) is handled by the combiner and
/// yields its first parameter here, as in the original.
pub fn link(function: u32, params: &[f32], x: f32) -> f32 {
    let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
    match (function, params) {
        (1, [m, b]) => m * x + b,
        (1, [m]) => m * x,
        (1, _) => x,
        (2, [a]) => x * x * sign(x) * a,
        (2, _) => x * x * sign(x),
        (3, [a]) => x * x * x * a,
        (3, _) => x * x * x,
        (4, _) if x == 0.0 => 0.0,
        (4, [a]) => x.abs().sqrt() * sign(x) * a,
        (4, _) => x.abs().sqrt() * sign(x),
        (5, _) => -x,
        (6, _) if x.abs() < f32::EPSILON => 0.0,
        (6, _) => 1.0 / x,
        // Written as `(x < 1) == (x == 1)` in the original: NaN takes the 1/x branch.
        (7, _) => if 1.0 < x || x.is_nan() { 1.0 / x } else { 1.0 },
        (8, []) => 1.0,
        (8 | 9, [c, ..]) => *c,
        (10, [m, clamp_x, clamp_y, dir]) => {
            if (0.0 < *dir && *clamp_x < x) || (*dir <= 0.0 && x < *clamp_x) { (clamp_y - clamp_x * m) + m * x } else { *clamp_y }
        }
        (10, [m]) => if x <= 0.0 { 0.0 } else { x * m },
        (10, _) => x,
        _ => 0.0,
    }
}

/// Evaluation state of one actor's compiled face graph: node outputs (read by later nodes and, for inputs from
/// higher indices, from the previous evaluation) and the delta nodes' previous input.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub outputs: Vec<f32>,
    previous: Vec<f32>,
    evaluated: bool,
}

impl Graph {
    /// Evaluates `nodes` in index order at animation `time`; `track[i]` is node `i`'s value from the playing
    /// animation's curve of the same name (0 when none). FaceFX registers are not modelled (their operation stays
    /// "none", 3).
    pub fn evaluate(&mut self, nodes: &[Node], track: &[f32], time: f32) -> &[f32] {
        self.outputs.resize(nodes.len(), 0.0);
        self.previous.resize(nodes.len(), 0.0);
        let source = |outputs: &[f32], l: &bl2_formats::facefx::Link| outputs.get(l.node as usize).copied().unwrap_or(0.0);
        for (i, n) in nodes.iter().enumerate() {
            let out = match n.kind {
                1 => match n.inputs.first() {
                    Some(l) => {
                        let x = link(l.function, &l.params, source(&self.outputs, l));
                        let d = if self.evaluated { x - self.previous[i] } else { 0.0 };
                        self.previous[i] = x;
                        d
                    }
                    None => 0.0,
                },
                2 => time,
                _ => {
                    let mut v = if n.inputs.is_empty() { 0.0 } else { [0.0, 1.0, -f32::MAX, f32::MAX].get(n.input_op as usize).copied().unwrap_or(0.0) };
                    let mut correction = 0.0f32;
                    for l in &n.inputs {
                        if l.function == 9 {
                            let s = nodes.get(l.node as usize).map(|s| s.values).unwrap_or_default();
                            let x = source(&self.outputs, l);
                            let (up, down) = (s[3] * x, -x * s[1]);
                            correction = (l.params.first().copied().unwrap_or(0.0) * if down < up { up } else { down } + correction).min(1.0);
                            continue;
                        }
                        let x = link(l.function, &l.params, source(&self.outputs, l));
                        match n.input_op {
                            0 => v += x,
                            1 => v *= x,
                            2 if v <= x => v = x,
                            3 if x <= v => v = x,
                            _ => {}
                        }
                    }
                    v = (1.0 - correction) * (track.get(i).copied().unwrap_or(0.0) + v);
                    let (min, max) = (n.values[0], n.values[2]);
                    if max < v { v = max; }
                    // `(min < v) == (min == v)`: NaN becomes min.
                    if v < min || v.is_nan() { v = min; }
                    v
                }
            };
            self.outputs[i] = out;
        }
        self.evaluated = true;
        &self.outputs
    }
}

/// Result of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// The animation clock reached 0 for the first time: the component starts the sound now.
    StartSound,
    Playing,
    Stopped,
}

/// One playing animation (`PlayAnim` 0x14e5e60 sets state 1; the tick sets state 2 at the end).
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    /// Animation span from `facefx::Anim::time_range`.
    start: f32,
    end: f32,
    /// `PlayAnim`'s third argument (+0x2d): start at time 0 instead of the (negative) first key.
    from_zero: bool,
    /// Wall-clock time of animation time `-offset` (+0x1c); `None` until the first tick.
    base: Option<f64>,
    /// Lead-in before time 0 (+0x28).
    offset: f32,
    /// Current animation time (+0x24).
    time: f32,
    /// Time 0 already reached (+0x2c).
    sound_started: bool,
    stopped: bool,
}

impl Playback {
    pub fn new(span: (f32, f32), from_zero: bool) -> Self {
        Self { start: span.0, end: span.1, from_zero, base: None, offset: 0.0, time: f32::MIN, sound_started: false, stopped: false }
    }

    /// `IsPlayingFaceFXAnim` (0xa1d030): instance state ≠ 2.
    pub fn is_playing(&self) -> bool { !self.stopped }

    /// `StopFaceFXAnim` (0xa1a550 → instance stop 0x14e54c0). The component's FaceFX update (0xa2d330) calls it as
    /// soon as the line's Ak event (handle at component +0x664) is no longer playing, so a line ends at the earlier
    /// of its last animation key and the end of its sound.
    pub fn stop(&mut self) { self.stopped = true; }

    pub fn time(&self) -> f32 { self.time }

    /// One tick at wall-clock `now` (seconds). `sound_time` is the sound's playback position when known (≥ 0);
    /// a negative value means "not known yet" and the animation runs on the wall clock.
    pub fn tick(&mut self, now: f64, sound_time: f32) -> Tick {
        if self.stopped { return Tick::Stopped; }
        let base = *self.base.get_or_insert_with(|| {
            self.offset = if 0.0 <= self.start || self.from_zero { 0.0 } else { self.start.abs() };
            now
        });
        if 0.0 <= sound_time {
            self.time = sound_time;
            self.base = Some(now - sound_time as f64 - self.offset as f64);
        } else {
            self.time = (now - base) as f32 - self.offset;
        }
        if self.end <= self.time {
            self.stopped = true;
            return Tick::Stopped;
        }
        // Written as `(0 < t) != (t == 0)`: t ≥ 0, NaN excluded.
        if !self.sound_started && 0.0 <= self.time {
            self.sound_started = true;
            return Tick::StartSound;
        }
        Tick::Playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl2_formats::facefx::{Key, Link};

    fn key(time: f32, value: f32, slope_in: f32, slope_out: f32) -> Key { Key { time, value, slope_in, slope_out } }

    #[test]
    fn curve_hermite_linear_and_ends() {
        let mut c = Curve { name: "open".into(), word: 0, keys: vec![key(0.0, 0.0, 0.0, 0.0), key(1.0, 1.0, 0.0, 0.0)] };
        assert_eq!(curve_value(&c, -1.0), 0.0);
        assert_eq!(curve_value(&c, 2.0), 1.0);
        assert!((curve_value(&c, 0.5) - 0.5).abs() < 1e-6, "flat-slope Hermite is smoothstep");
        assert!((curve_value(&c, 0.25) - 0.15625).abs() < 1e-6);
        c.keys[0].slope_out = 2.0;
        assert!((curve_value(&c, 0.25) - (0.15625 + 2.0 * 0.25 * 0.75 * 0.75)).abs() < 1e-6, "left slope out");
        c.word = 1;
        assert!((curve_value(&c, 0.25) - 0.25).abs() < 1e-6);
        c.word = 2;
        assert_eq!(curve_value(&c, 0.25), 0.0);
    }

    #[test]
    fn link_functions() {
        assert_eq!(link(1, &[2.0, 1.0], 3.0), 7.0);
        assert_eq!(link(2, &[2.0], -3.0), -18.0, "quadratic keeps the sign");
        assert_eq!(link(4, &[1.0], -4.0), -2.0, "square root of |x| with the sign");
        assert_eq!(link(5, &[], 0.5), -0.5);
        assert_eq!(link(7, &[], 4.0), 0.25);
        assert_eq!(link(7, &[], 0.5), 1.0);
        assert_eq!(link(10, &[1.0, 0.5, 2.0, 1.0], 1.0), 2.5);
        assert_eq!(link(10, &[1.0, 0.5, 2.0, 1.0], 0.0), 2.0);
    }

    fn node(kind: u32, input_op: u32, min: f32, max: f32, inputs: Vec<Link>) -> Node {
        Node { kind, name: String::new(), values: [min, if min == 0.0 { 1.0 } else { 1.0 / min }, max, 1.0 / max], input_op, inputs, properties: vec![] }
    }

    #[test]
    fn graph_combines_clamps_and_corrects() {
        let l = |node, function, params: &[f32]| Link { node, function, params: params.to_vec() };
        let nodes = vec![
            node(0, 0, 0.0, 1.0, vec![]),
            node(0, 0, 0.0, 1.0, vec![]),
            node(4, 0, 0.0, 1.0, vec![l(0, 1, &[1.0, 0.0]), l(1, 2, &[1.0])]),
            node(4, 0, 0.0, 1.0, vec![l(2, 1, &[1.0, 0.0]), l(1, 9, &[0.5])]),
            node(1, 0, 0.0, 1.0, vec![l(0, 1, &[1.0, 0.0])]),
        ];
        let mut g = Graph::default();
        let out = g.evaluate(&nodes, &[0.4, 0.5, 0.0, 0.0, 0.0], 0.0).to_vec();
        assert_eq!(out[2], 0.65, "sum of linear 0.4 and quadratic 0.25");
        assert_eq!(out[3], 0.65 * (1.0 - 0.5 * 0.5), "corrective scales by 1 - 0.5 * 0.5");
        assert_eq!(out[4], 0.0, "delta is 0 on the first evaluation");
        let out = g.evaluate(&nodes, &[0.9, 1.0, 0.0, 0.0, 0.0], 0.0).to_vec();
        assert_eq!(out[2], 1.0, "clamped to max");
        assert!((out[4] - 0.5).abs() < 1e-6, "delta of the source");
    }

    #[test]
    fn lead_in_then_sound_clock_then_stop() {
        let mut p = Playback::new((-0.3, 2.0), false);
        assert_eq!(p.tick(10.0, -1.0), Tick::Playing);
        assert!((p.time() + 0.3).abs() < 1e-6, "starts at the first key");
        assert_eq!(p.tick(10.2, -1.0), Tick::Playing);
        assert_eq!(p.tick(10.4, -1.0), Tick::StartSound, "sound starts 0.3 s after PlayFaceFXAnim");
        assert_eq!(p.tick(11.5, 1.0), Tick::Playing);
        assert_eq!(p.time(), 1.0, "the sound position drives the clock");
        assert!(p.is_playing());
        assert_eq!(p.tick(12.0, -1.0), Tick::Playing, "wall clock continues from the last sound position");
        assert!((p.time() - 1.5).abs() < 1e-5);
        assert_eq!(p.tick(12.6, -1.0), Tick::Stopped, "ends at the last key, not with the sound");
        assert!(!p.is_playing());
    }

    #[test]
    fn from_zero_skips_lead_in() {
        let mut p = Playback::new((-0.3, 1.0), true);
        assert_eq!(p.tick(0.0, -1.0), Tick::StartSound, "time 0 is enough to start the sound");
        assert_eq!(p.time(), 0.0);
        assert_eq!(p.tick(0.01, -1.0), Tick::Playing);
    }
}

/// The playback clock against the original: FACEFX_TICKS = tools/research/facefx-ticks-text.mjs output of a
/// frida-facefx.mjs capture (every PlayAnim and the following ticks of the instance with the original's now,
/// sound time, return value, animation time and state). Replays the same inputs through `Playback`.
#[cfg(test)]
mod original {
    use super::*;
    use std::collections::HashMap;

    #[test]
    #[ignore = "needs a capture from the original (FACEFX_TICKS)"]
    fn ticks_match_original() {
        let text = std::fs::read_to_string(std::env::var("FACEFX_TICKS").expect("set FACEFX_TICKS")).unwrap();
        let mut live = HashMap::<String, Playback>::new();
        let (mut ticks, mut same, mut worst, mut stopped_between) = (0, 0, 0f32, 0);
        for l in text.lines() {
            let t: Vec<&str> = l.split(' ').collect();
            let f = |i: usize| t[i].parse::<f64>().unwrap();
            if t[0] == "P" {
                live.insert(t[1].into(), Playback::new((f(2) as f32, f(3) as f32), t[4] != "0"));
                continue;
            }
            let Some(p) = live.get_mut(t[1]) else { continue };
            // Stopped between ticks by StopFaceFXAnim (the sound ended or the line was interrupted): the original's
            // tick then returns 1 or 2 by its blend-out flags (+0xc/+0xd), which are not modelled.
            let external = t[7] == "2";
            if external { p.stop(); }
            let got = p.tick(f(2), f(3) as f32);
            let want = match t[4] { "0" => Tick::StartSound, "2" => Tick::Stopped, _ => Tick::Playing };
            let (time, state) = (f(5) as f32, t[6]);
            ticks += 1;
            if external { stopped_between += 1; }
            let ok_state = (external || got == want) && (state == "2") == !p.is_playing();
            let err = if state == "2" { 0.0 } else { (p.time() - time).abs() };
            worst = worst.max(err);
            if ok_state && err == 0.0 { same += 1; } else { println!("{l} -> {got:?} time {}", p.time()); }
            if state == "2" { live.remove(t[1]); }
        }
        println!("{ticks} ticks ({stopped_between} after a stop between ticks), {same} with the same result, state and time; worst time error {worst:e}");
        assert_eq!(same, ticks);
    }
}
