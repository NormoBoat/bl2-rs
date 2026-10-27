//! Readable text for UnrealScript bytecode (research and porting aid, plan/12-tasks.md T-015).
//! Output is UnrealScript-like, not compilable; unknown or build-specific forms are printed explicitly.

use std::collections::HashMap;

use crate::bytecode::{Expr, Operand};
use crate::package::{Package, Reader};
use crate::script::Definition;

const FUNC_PRE_OPERATOR: u32 = 0x10;
const FUNC_OPERATOR: u32 = 0x1000;

/// Native function index -> (name, function flags), collected from script packages.
#[derive(Default)]
pub struct Natives(HashMap<u16, (String, u32)>);

impl Natives {
    pub fn add_package(&mut self, p: &Package) {
        for e in p.exports.iter().filter(|e| e.class != 0 && p.object_name(e.class) == "Function") {
            let mut r = Reader::new(&p.data[..e.serial_offset + e.serial_size], e.serial_offset);
            if let Ok(Definition::Function { native, flags, friendly_name, .. }) = p.read_definition(e, "Function", &mut r)
                && native != 0
            {
                let friendly = p.name(friendly_name);
                let name = if flags & FUNC_OPERATOR != 0 && !friendly.is_empty() && friendly != "None" { friendly } else { p.name(e.object_name) };
                self.0.insert(native, (name, flags));
            }
        }
    }
}

fn obj(p: &Package, o: &Operand) -> String {
    match o {
        Operand::Obj(i) => p.object_name(*i),
        other => format!("{other:?}"),
    }
}

fn name(p: &Package, o: &Operand) -> String {
    match o {
        Operand::Name(i, n) if (*i as usize) < p.names.len() => {
            if *n == 0 { p.names[*i as usize].clone() } else { format!("{}_{}", p.names[*i as usize], n - 1) }
        }
        other => format!("{other:?}"),
    }
}

fn args(p: &Package, n: &Natives, c: &[Expr]) -> String {
    c.iter().map(|e| expr(p, n, e)).collect::<Vec<_>>().join(", ")
}

/// Text of one expression.
pub fn expr(p: &Package, n: &Natives, e: &Expr) -> String {
    let o = &e.operands;
    let c = &e.children;
    let ch = |i: usize| expr(p, n, &c[i]);
    match e.op {
        0x00 | 0x01 | 0x48 => obj(p, &o[0]),
        0x02 => format!("default.{}", obj(p, &o[0])),
        0x5E => format!("{}/*5E*/", obj(p, &o[0])),
        0x4C..=0x50 => match &o[0] {
            Operand::Raw(b) => format!("$var{:02X}_{}", e.op, u32::from_le_bytes(b[..4].try_into().unwrap())),
            _ => unreachable!(),
        },
        0x03 => format!("switch ({})", ch(0)),
        0x05 => format!("switch ({})", ch(0)),
        0x04 if c[0].op == 0x3A => "return".into(),
        0x04 => format!("return {}", ch(0)),
        0x06 => format!("goto @m{:?}", o[0]),
        0x07 => format!("if (!({})) goto @m{:?}", ch(0), o[0]),
        0x08 => "stop".into(),
        0x09 => format!("assert({})", ch(0)),
        0x0A if c.is_empty() => "default:".into(),
        0x0A => format!("case {}:", ch(0)),
        0x0B | 0x41 | 0x53 | 0x15 | 0x16 | 0x4A => String::new(),
        0x0C => "labels".into(),
        0x0D => format!("goto {}", ch(0)),
        0x0E | 0x18 | 0x2D | 0x51 => ch(0),
        0x0F | 0x14 | 0x44 => format!("{} = {}", ch(0), ch(1)),
        0x5F => format!("{} = {} /*5F*/", ch(0), ch(1)),
        0x10 | 0x1A => format!("{}[{}]", ch(1), ch(0)),
        0x11 => format!("new({}, {}, {}) {}({})", ch(0), ch(1), ch(2), ch(3), ch(4)),
        0x12 => format!("{}.static.{}", ch(0), ch(1)),
        0x13 => format!("class<{}>({})", obj(p, &o[0]), ch(0)),
        0x17 => "self".into(),
        0x19 => format!("{}.{}", ch(0), ch(1)),
        0x1B => format!("{}({})", name(p, &o[0]), args(p, n, c)),
        0x37 => format!("global.{}({})", name(p, &o[0]), args(p, n, c)),
        0x1C => format!("{}({})", obj(p, &o[0]), args(p, n, c)),
        0x1D => match o[0] { Operand::I32(v) => v.to_string(), _ => unreachable!() },
        0x1E => match o[0] { Operand::F32(v) => format!("{v:?}"), _ => unreachable!() },
        0x1F | 0x34 => match &o[0] { Operand::Str(s) => format!("{s:?}"), _ => unreachable!() },
        0x20 => format!("'{}'", obj(p, &o[0])),
        0x21 => format!("'{}'", name(p, &o[0])),
        0x22 => format!("rot({:?}, {:?}, {:?})", o[0], o[1], o[2]),
        0x23 => format!("vect({:?}, {:?}, {:?})", o[0], o[1], o[2]),
        0x24 | 0x2C => match o[0] { Operand::U8(v) => v.to_string(), _ => unreachable!() },
        0x25 => "0".into(),
        0x26 => "1".into(),
        0x27 => "true".into(),
        0x28 => "false".into(),
        0x29 => format!("/*nativeparm {}*/", obj(p, &o[0])),
        0x2A => "None".into(),
        0x2E | 0x52 => format!("{}({})", obj(p, &o[0]), ch(0)),
        0x2F => format!("foreach {}", ch(0)),
        0x30 => "/*iterator pop*/".into(),
        0x31 => "/*iterator next*/".into(),
        0x32 => format!("({} == {})", ch(0), ch(1)),
        0x33 => format!("({} != {})", ch(0), ch(1)),
        0x35 => format!("{}.{}", ch(0), obj(p, &o[0])),
        0x36 => format!("{}.Length", ch(0)),
        0x38 => match o[0] {
            Operand::U8(t) => format!("{}({})", cast_name(t), ch(0)),
            _ => unreachable!(),
        },
        0x39 => format!("{}.Insert({}, {})", ch(0), ch(1), ch(2)),
        0x3A => "return".into(),
        0x3B | 0x3D => format!("({} == {})", ch(0), ch(1)),
        0x3C | 0x3E => format!("({} != {})", ch(0), ch(1)),
        0x3F => "None".into(),
        0x40 => format!("{}.Remove({}, {})", ch(0), ch(1), ch(2)),
        0x42 => format!("{}({})", name(p, &o[2]), args(p, n, c)),
        0x43 => name(p, &o[0]),
        0x45 => format!("({} ? {} : {})", ch(0), ch(1), ch(2)),
        0x46 => format!("{}.Find({})", ch(0), ch(1)),
        0x47 => format!("{}.Find({}, {})", ch(0), ch(1), ch(2)),
        0x49 => format!("/*default*/ {}", args(p, n, c)),
        0x4B => format!("{}/*delegate*/", name(p, &o[0])),
        0x54 => format!("{}.Add({})", ch(0), ch(1)),
        0x55 => format!("{}.AddItem({})", ch(0), ch(1)),
        0x56 => format!("{}.RemoveItem({})", ch(0), ch(1)),
        0x57 => format!("{}.InsertItem({}, {})", ch(0), ch(1), ch(2)),
        0x58 => format!("foreach {}({}, {})", ch(0), ch(1), ch(2)),
        0x59 => format!("{}.Sort({})", ch(0), ch(1)),
        0x5A => format!("/*5A skip {:?}*/", o[0]),
        0x60..=0xFF => {
            let index = match (e.op, o.first()) {
                (0x60..=0x6F, Some(Operand::U8(b))) => (((e.op & 0x0F) as u16) << 8) | *b as u16,
                (op, _) => op as u16,
            };
            match n.0.get(&index) {
                Some((name, flags)) if flags & FUNC_OPERATOR != 0 && c.len() == 2 => format!("({} {name} {})", ch(0), ch(1)),
                Some((name, flags)) if flags & FUNC_OPERATOR != 0 && c.len() == 1 && flags & FUNC_PRE_OPERATOR != 0 => format!("{name}{}", ch(0)),
                Some((name, flags)) if flags & FUNC_OPERATOR != 0 && c.len() == 1 => format!("{}{name}", ch(0)),
                Some((name, _)) => format!("{name}({})", args(p, n, c)),
                None => format!("native{index}({})", args(p, n, c)),
            }
        }
        op => format!("op{op:02X}({})", args(p, n, c)),
    }
}

/// UE3 primitive cast tokens (checked against decompiled stock functions, E-061).
fn cast_name(t: u8) -> String {
    let n = match t {
        0x36 => "InterfaceToObject", 0x37 => "InterfaceToString", 0x38 => "InterfaceToBool", 0x39 => "RotatorToVector", 0x3A => "ByteToInt", 0x3B => "ByteToBool", 0x3C => "ByteToFloat", 0x3D => "IntToByte",
        0x3E => "IntToBool", 0x3F => "IntToFloat", 0x40 => "BoolToByte", 0x41 => "BoolToInt", 0x42 => "BoolToFloat",
        0x43 => "FloatToByte", 0x44 => "FloatToInt", 0x45 => "FloatToBool", 0x46 => "ObjectToInterface", 0x47 => "ObjectToBool",
        0x48 => "NameToBool", 0x49 => "StringToByte", 0x4A => "StringToInt", 0x4B => "StringToBool", 0x4C => "StringToFloat",
        0x4D => "StringToVector", 0x4E => "StringToRotator", 0x4F => "VectorToBool", 0x50 => "VectorToRotator",
        0x51 => "RotatorToBool", 0x52 => "ByteToString", 0x53 => "IntToString", 0x54 => "BoolToString", 0x55 => "FloatToString",
        0x56 => "ObjectToString", 0x57 => "NameToString", 0x58 => "VectorToString", 0x59 => "RotatorToString",
        0x5A => "DelegateToString", 0x60 => "StringToName",
        _ => return format!("cast{t:02X}"),
    };
    n.to_string()
}

/// One statement line with its disk and memory offsets (jump targets are memory offsets).
pub fn statement(p: &Package, n: &Natives, e: &Expr) -> String {
    format!("/*d{:04} m{:04}*/ {}", e.disk, e.mem, expr(p, n, e))
}
