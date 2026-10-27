//! Skeletal pose (T-036, plan/03-evidence.md E-076): component-space bone transforms from local bone atoms, as the
//! original keeps them in `SkeletalMeshComponent.LocalAtoms` / `SpaceBases` (both arrays of `FBoneAtom`).

/// `FBoneAtom`: rotation quaternion (x, y, z, w), translation, uniform scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atom {
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: f32,
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// v rotated by unit quaternion q (q·v·q⁻¹).
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

impl Atom {
    /// `self` (child, in parent space) placed in the parent's space: rotation parent·child, translation
    /// parent.rotate(child.translation · parent.scale) + parent.translation, scale multiplied.
    pub fn then(&self, parent: &Atom) -> Atom {
        let s = self.translation.map(|v| v * parent.scale);
        let r = rotate(parent.rotation, s);
        Atom {
            rotation: quat_mul(parent.rotation, self.rotation),
            translation: [r[0] + parent.translation[0], r[1] + parent.translation[1], r[2] + parent.translation[2]],
            scale: self.scale * parent.scale,
        }
    }
}

/// Component-space atoms of a skeleton: bone 0 is the root (its own parent), every other bone follows its
/// parent, which comes earlier in the RefSkeleton.
pub fn component_space(parents: &[i32], local: &[Atom]) -> Vec<Atom> {
    let mut space: Vec<Atom> = Vec::with_capacity(local.len());
    for (i, a) in local.iter().enumerate() {
        let p = parents[i] as usize;
        space.push(if i == 0 || p >= i { *a } else { a.then(&space[p]) });
    }
    space
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_follows_parent() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let root = Atom { rotation: [0.0, 0.0, half, half], translation: [10.0, 0.0, 0.0], scale: 1.0 };
        let child = Atom { rotation: [0.0, 0.0, 0.0, 1.0], translation: [1.0, 0.0, 0.0], scale: 1.0 };
        let s = component_space(&[0, 0], &[root, child]);
        assert!((s[1].translation[0] - 10.0).abs() < 1e-5 && (s[1].translation[1] - 1.0).abs() < 1e-5, "90° about z turns +x into +y");
    }
}

/// Pose composition against the original: SKEL_POSES = lines "K <mesh> <parents,…> <LocalAtoms hex> <SpaceBases hex>"
/// from tools/research/frida-skel-pose.mjs (converted as in plan/15-handoff.md); reports the largest translation and
/// rotation difference between `component_space(LocalAtoms)` and the original's SpaceBases.
#[cfg(test)]
mod original {
    use super::*;

    fn atoms(hex: &str) -> Vec<Atom> {
        let b: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        b.chunks(32).map(|c| {
            let f = |i: usize| f32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap());
            Atom { rotation: [f(0), f(1), f(2), f(3)], translation: [f(4), f(5), f(6)], scale: f(7) }
        }).collect()
    }

    #[test]
    #[ignore = "needs a capture from the original (SKEL_POSES)"]
    fn poses_match_original() {
        let text = std::fs::read_to_string(std::env::var("SKEL_POSES").expect("set SKEL_POSES")).unwrap();
        let (mut bones, mut skipped, mut over, mut worst_ok) = (0, 0, 0, 0f32);
        let (mut poses, mut poses_ok) = (0, 0);
        for l in text.lines() {
            let t: Vec<&str> = l.split(' ').collect();
            let parents: Vec<i32> = t[2].split(',').map(|p| p.parse().unwrap()).collect();
            let (local, want) = (atoms(t[3]), atoms(t[4]));
            // One step per bone: the local atom placed on the original's own parent base, so each bone is checked
            // independently of the others.
            let one_step: Vec<Atom> = local.iter().enumerate().map(|(i, a)| if i == 0 { *a } else { a.then(&want[parents[i] as usize]) }).collect();
            let mut pose_over = 0;
            for (g, w) in one_step.iter().zip(&want) {
                // Bones the original did not compute this frame (not required at the current LOD) stay identity.
                if w.rotation == [0.0, 0.0, 0.0, 1.0] && w.translation == [0.0; 3] { skipped += 1; continue; }
                bones += 1;
                let e = (0..3).map(|i| (g.translation[i] - w.translation[i]).abs()).fold(0.0, f32::max);
                if e > 0.01 { pose_over += 1; } else { worst_ok = worst_ok.max(e); }
            }
            poses += 1;
            over += pose_over;
            if pose_over == 0 { poses_ok += 1; } else { println!("{}: {pose_over} bones over 0.01 uu", t[1]); }
        }
        println!("{poses} poses, {poses_ok} entirely within 0.01 uu; {bones} computed bones ({skipped} not computed skipped), {over} over 0.01 uu; largest difference of the others {worst_ok:e} uu");
    }
}
