//! FaceFX archives (T-021b): `FACE` version 1740 as stored in `FaceFXAsset`/`FaceFXAnimSet` and in Wwise cue labels
//! (E-070). Layout from the original's loaders (REA, plan/03-evidence.md E-072): `FxArchive` header, class table,
//! name table, then the objects; at archive version > 1699 a class's version comes from the class table, not from
//! the object.

use std::io::{Error, ErrorKind, Result};

use crate::package::Reader;

fn bad(message: impl Into<String>) -> Error { Error::new(ErrorKind::InvalidData, message.into()) }

pub const VERSION: u32 = 1740;

/// One class table entry: `[u32 id][u32 1][string name][u16 version]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassEntry {
    pub id: u32,
    pub name: String,
    pub version: u16,
}

/// Archive header and tables; `pos` is where the objects start.
#[derive(Debug, Clone)]
pub struct Archive<'a> {
    pub licensee: String,
    pub project: String,
    pub licensee_version: u32,
    /// u16 after the licensee version (1 in every archive of the game).
    pub flags: u16,
    pub classes: Vec<ClassEntry>,
    pub names: Vec<String>,
    data: &'a [u8],
    pos: usize,
}

/// `FxString`: u32 length, then that many bytes (no NUL).
fn string(r: &mut Reader) -> Result<String> {
    let n = r.count()?;
    Ok(r.bytes(n)?.iter().map(|&b| b as char).collect())
}

fn f32_(r: &mut Reader) -> Result<f32> { Ok(f32::from_bits(r.u32()?)) }

/// Header strings carry their NUL in the length.
fn header_string(r: &mut Reader) -> Result<String> {
    let s = string(r)?;
    s.strip_suffix('\0').map(str::to_owned).ok_or_else(|| bad("FaceFX header string lacks NUL"))
}

impl<'a> Archive<'a> {
    pub fn read(data: &'a [u8]) -> Result<Self> {
        let mut r = Reader::new(data, 0);
        if r.bytes(4)? != b"FACE" { return Err(bad("not a FaceFX archive")); }
        let version = r.u32()?;
        if version != VERSION { return Err(bad(format!("FaceFX archive version {version}"))); }
        if r.u32()? != 0 { return Err(bad("FaceFX header word after version is not 0")); }
        let (licensee, project) = (header_string(&mut r)?, header_string(&mut r)?);
        let licensee_version = r.u32()?;
        let flags = r.u16()?;
        let classes = (0..r.count()?).map(|_| {
            let id = r.u32()?;
            if r.u32()? != 1 { return Err(bad("FaceFX class entry word is not 1")); }
            Ok(ClassEntry { id, name: string(&mut r)?, version: r.u16()? })
        }).collect::<Result<Vec<_>>>()?;
        let names = (0..r.count()?).map(|_| string(&mut r)).collect::<Result<_>>()?;
        Ok(Self { licensee, project, licensee_version, flags, classes, names, data, pos: r.pos })
    }

    pub fn class_version(&self, name: &str) -> Option<u16> { self.classes.iter().find(|c| c.name == name).map(|c| c.version) }

    fn name(&self, r: &mut Reader) -> Result<String> {
        let i = r.count()?;
        self.names.get(i).cloned().ok_or_else(|| bad(format!("FaceFX name index {i} of {}", self.names.len())))
    }

    /// The archive of a single animation (Wwise cue label): a u32 0, then one `FxAnim`; nothing may follow.
    pub fn animation(&self) -> Result<Anim> {
        let mut r = Reader::new(self.data, self.pos);
        let lead = r.u32()?;
        if lead != 0 { return Err(bad(format!("FaceFX animation archive lead word {lead}"))); }
        let anim = self.anim(&mut r)?;
        if r.pos != self.data.len() { return Err(bad(format!("{} bytes after FxAnim", self.data.len() - r.pos))); }
        Ok(anim)
    }

    /// The archive of a `FaceFXAsset`: a u32 0, then one `FxActor` (`FxActor::Serialize` 0x14e7390, class version 3);
    /// nothing may follow.
    pub fn actor(&self) -> Result<Actor> {
        if self.class_version("FxActor") != Some(3) || self.class_version("FxMasterBoneList") != Some(2) || self.class_version("FxPhonemeMap") != Some(1) {
            return Err(bad("FxActor/FxMasterBoneList/FxPhonemeMap class versions are not 3/2/1"));
        }
        let mut r = Reader::new(self.data, self.pos);
        let lead = r.u32()?;
        if lead != 0 { return Err(bad(format!("FaceFX actor archive lead word {lead}"))); }
        let r = &mut r;
        let name = self.name(r)?;
        // FxMasterBoneList v2 (0x14e8d20, entry 0x14e85d0): bone, quaternion, u32, f32, linked bones.
        let bones = (0..r.count()?).map(|_| {
            let bone = self.bone(r)?;
            let rotation = [f32_(r)?, f32_(r)?, f32_(r)?, f32_(r)?];
            let (word, weight) = (r.u32()?, f32_(r)?);
            let links = (0..r.count()?).map(|_| Ok((r.u32()?, self.bone(r)?))).collect::<Result<_>>()?;
            Ok(MasterBone { bone, rotation, word, weight, links })
        }).collect::<Result<_>>()?;
        // FxCompiledFaceGraph (0x14ed190): nodes (0x14ec880 → 0x14ec400), then 0x14ec0f0.
        let nodes = (0..r.count()?).map(|_| {
            let kind = r.u32()?;
            let name = self.name(r)?;
            let values = [f32_(r)?, f32_(r)?, f32_(r)?, f32_(r)?];
            let input_op = r.u32()?;
            // Inputs (0x14eb980): source node, link function, FxLinkFnParameters (0x14f82d0: u32 count + f32).
            let inputs = (0..r.count()?).map(|_| {
                let (node, function) = (r.u32()?, r.u32()?);
                let params = (0..r.count()?).map(|_| f32_(r)).collect::<Result<_>>()?;
                Ok(Link { node, function, params })
            }).collect::<Result<_>>()?;
            // FxFaceGraphNodeUserProperty (0x14f5630): name, type, integer, float, choice strings (0x14f5140).
            let properties = (0..r.count()?).map(|_| Ok(UserProperty {
                name: self.name(r)?, kind: r.u32()?, integer: r.u32()?, float: f32_(r)?,
                choices: (0..r.count()?).map(|_| string(r)).collect::<Result<_>>()?,
            })).collect::<Result<_>>()?;
            Ok(Node { kind, name, values, input_op, inputs, properties })
        }).collect::<Result<_>>()?;
        let tables = (0..r.count()?).map(|_| {
            let key = r.u32()?;
            Ok((key, (0..r.count()?).map(|_| Ok((string(r)?, r.u32()?))).collect::<Result<_>>()?))
        }).collect::<Result<_>>()?;
        // Default anim group (actor +0x6058), then the other groups (+0x606c, 0x14e6880); FxAnimGroup (0x14e52e0):
        // name + FxAnim array (0x14e5100).
        let mut groups = vec![self.group(r)?];
        for _ in 0..r.count()? { groups.push(self.group(r)?); }
        // FxPhonemeMap v1 (0x14f34c0): (phoneme, target, amount) entries (0x14f3000), then target names (0x14f31f0).
        let phonemes = (0..r.count()?).map(|_| Ok((r.u32()?, self.name(r)?, f32_(r)?))).collect::<Result<_>>()?;
        let phoneme_targets = (0..r.count()?).map(|_| self.name(r)).collect::<Result<_>>()?;
        if r.pos != self.data.len() { return Err(bad(format!("{} bytes after FxActor", self.data.len() - r.pos))); }
        Ok(Actor { name, bones, nodes, tables, groups, phonemes, phoneme_targets })
    }

    /// `FxBone::Serialize` (0x14f46f0): name, position, rotation (quaternion), scale.
    fn bone(&self, r: &mut Reader) -> Result<Bone> {
        Ok(Bone { name: self.name(r)?, position: [f32_(r)?, f32_(r)?, f32_(r)?], rotation: [f32_(r)?, f32_(r)?, f32_(r)?, f32_(r)?], scale: [f32_(r)?, f32_(r)?, f32_(r)?] })
    }

    fn group(&self, r: &mut Reader) -> Result<AnimGroup> {
        Ok(AnimGroup { name: self.name(r)?, anims: (0..r.count()?).map(|_| self.anim(r)).collect::<Result<_>>()? })
    }

    /// `FxAnim::Serialize` (exe 0x14e48c0), class version 7.
    pub fn anim(&self, r: &mut Reader) -> Result<Anim> {
        if self.class_version("FxAnim") != Some(7) { return Err(bad("FxAnim class version is not 7")); }
        let name = self.name(r)?;
        // FxAnimCurve::Serialize (0x14edc10): FxNamedObject + u32; its keys come after all curves.
        let curves = (0..r.count()?).map(|_| Ok((self.name(r)?, r.u32()?))).collect::<Result<Vec<_>>>()?;
        // Class version > 5: all keys in one block, then a u32 array of keys per curve (only when there are keys).
        let total = r.count()?;
        let mut keys = Vec::new();
        for _ in 0..total {
            let mut k = [0f32; 4];
            for v in &mut k { *v = f32::from_bits(r.u32()?); }
            keys.push(Key { time: k[0], value: k[1], slope_in: k[2], slope_out: k[3] });
        }
        let counts = if total == 0 { vec![0; curves.len()] } else { (0..r.count()?).map(|_| r.count()).collect::<Result<Vec<_>>>()? };
        if counts.len() != curves.len() || counts.iter().sum::<usize>() != total { return Err(bad("FxAnim per-curve key counts do not match")); }
        let mut at = 0;
        let curves = curves.into_iter().zip(counts).map(|((name, word), n)| { at += n; Curve { name, word, keys: keys[at - n..at].to_vec() } }).collect();
        let (blend_in, blend_out) = (f32::from_bits(r.u32()?), f32::from_bits(r.u32()?));
        let bone_weights = (0..r.count()?).map(|_| Ok((self.name(r)?, f32::from_bits(r.u32()?)))).collect::<Result<_>>()?;
        let strings_a = [string(r)?, string(r)?];
        let word_a = r.u32()?;
        let strings_b = [string(r)?, string(r)?];
        let word_b = r.u32()?;
        Ok(Anim { name, curves, blend_in, blend_out, bone_weights, strings_a, word_a, strings_b, word_b })
    }
}

/// `FxBone`: bind transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// Master bone list entry: the bone, a quaternion, a u32 and an f32 (meanings in E-072) and the linked bones with
/// their indices.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterBone {
    pub bone: Bone,
    pub rotation: [f32; 4],
    pub word: u32,
    pub weight: f32,
    pub links: Vec<(u32, Bone)>,
}

/// Input of a compiled face graph node: source node index, link function id (`FxNullLinkFn` 0 …
/// `FxClampedLinearLinkFn` 10, E-072) and its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub node: u32,
    pub function: u32,
    pub params: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserProperty {
    pub name: String,
    pub kind: u32,
    pub integer: u32,
    pub float: f32,
    pub choices: Vec<String>,
}

/// Compiled face graph node (evaluated by 0x14ea0d0): type (1 delta, 2 current time, others combine their inputs),
/// name, `[min, 1/min (1 when min is 0), max, 1/max]`, input operation (0 sum, 1 product, 2 max, 3 min), inputs,
/// user properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: u32,
    pub name: String,
    pub values: [f32; 4],
    pub input_op: u32,
    pub inputs: Vec<Link>,
    pub properties: Vec<UserProperty>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimGroup {
    pub name: String,
    pub anims: Vec<Anim>,
}

/// `FxActor` v3. `groups[0]` is the default group.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub name: String,
    pub bones: Vec<MasterBone>,
    pub nodes: Vec<Node>,
    /// Second table of `FxCompiledFaceGraph` (0x14ec0f0): key, (string, u32) entries.
    pub tables: Vec<(u32, Vec<(String, u32)>)>,
    pub groups: Vec<AnimGroup>,
    /// (phoneme id, target name, amount).
    pub phonemes: Vec<(u32, String, f32)>,
    pub phoneme_targets: Vec<String>,
}

impl Anim {
    /// Start and end time as computed after loading (0x14e3a60): min of the curves' first key times and max of their
    /// last key times, a curve without keys counting as (0, 0) (0x14ed5f0); (0, 1) when there are no curves.
    pub fn time_range(&self) -> (f32, f32) {
        let (mut start, mut end) = (f32::MAX, f32::MIN_POSITIVE);
        for c in &self.curves {
            let (s, e) = match (c.keys.first(), c.keys.last()) { (Some(f), Some(l)) => (f.time, l.time), _ => (0.0, 0.0) };
            start = start.min(s);
            end = end.max(e);
        }
        if start == f32::MAX && end == f32::MIN_POSITIVE { (0.0, 1.0) } else { (start, end) }
    }
}

/// `FxAnimKey`: 4 × f32.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub time: f32,
    pub value: f32,
    pub slope_in: f32,
    pub slope_out: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub name: String,
    /// u32 of `FxAnimCurve` (+0x1c; meaning in E-072).
    pub word: u32,
    pub keys: Vec<Key>,
}

/// `FxAnim`; fields after the bone weights keep their stored order (meaning in E-072).
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub name: String,
    pub curves: Vec<Curve>,
    pub blend_in: f32,
    pub blend_out: f32,
    pub bone_weights: Vec<(String, f32)>,
    pub strings_a: [String; 2],
    pub word_a: u32,
    pub strings_b: [String; 2],
    pub word_b: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(out: &mut Vec<u8>, t: &str) { out.extend((t.len() as u32).to_le_bytes()); out.extend(t.as_bytes()); }
    fn w(out: &mut Vec<u8>, v: u32) { out.extend(v.to_le_bytes()); }

    /// Synthetic animation archive with the layout of the game's cue-label archives (E-072).
    #[test]
    fn animation_archive() {
        let mut a = b"FACE".to_vec();
        w(&mut a, VERSION); w(&mut a, 0);
        s(&mut a, "Unreal Engine 3 Licensee\0"); s(&mut a, "Unreal Engine 3 Project\0");
        w(&mut a, 1000); a.extend(1u16.to_le_bytes());
        let classes = [("FxObject", 0u16), ("FxAnim", 7), ("FxNamedObject", 0), ("FxName", 1), ("FxAnimCurve", 1)];
        w(&mut a, classes.len() as u32);
        for (i, (c, v)) in classes.iter().enumerate() { w(&mut a, i as u32 * 10); w(&mut a, 1); s(&mut a, c); a.extend(v.to_le_bytes()); }
        w(&mut a, 3); for n in ["Anim", "open", "Blink"] { s(&mut a, n); }
        w(&mut a, 0); w(&mut a, 0); // lead word, animation name
        w(&mut a, 2); w(&mut a, 1); w(&mut a, 0); w(&mut a, 2); w(&mut a, 0); // two curves
        w(&mut a, 3);
        for k in [[0.0f32, 1.0, 0.0, 0.0], [0.5, 0.0, 0.0, 0.0], [0.25, 0.75, 1.0, -1.0]] { for v in k { w(&mut a, v.to_bits()); } }
        w(&mut a, 2); w(&mut a, 2); w(&mut a, 1); // keys per curve
        w(&mut a, 0.16f32.to_bits()); w(&mut a, 0.22f32.to_bits());
        w(&mut a, 0); s(&mut a, ""); s(&mut a, ""); w(&mut a, u32::MAX); s(&mut a, "None"); s(&mut a, ""); w(&mut a, u32::MAX);
        let ar = Archive::read(&a).unwrap();
        assert_eq!(ar.names, ["Anim", "open", "Blink"]);
        let anim = ar.animation().unwrap();
        assert_eq!(anim.name, "Anim");
        assert_eq!(anim.curves.iter().map(|c| (c.name.as_str(), c.keys.len())).collect::<Vec<_>>(), [("open", 2), ("Blink", 1)]);
        assert_eq!(anim.curves[1].keys[0], Key { time: 0.25, value: 0.75, slope_in: 1.0, slope_out: -1.0 });
        assert_eq!((anim.blend_in, anim.blend_out, anim.strings_b[0].as_str()), (0.16, 0.22, "None"));
        a.push(0);
        assert!(Archive::read(&a).unwrap().animation().is_err(), "trailing byte");
    }
}
