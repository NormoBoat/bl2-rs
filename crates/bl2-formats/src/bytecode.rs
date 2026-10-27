//! UnrealScript bytecode reader for research and porting (plan/12-tasks.md T-015, decision D-005:
//! bytecode is never executed). Checked by walking every function of the installation: the walk must end
//! at EX_EndOfScript exactly at both the stored size and the in-memory size (E-060).

use std::io::{Error, ErrorKind, Result};

fn bad(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

pub const EX_END_FUNCTION_PARMS: u8 = 0x16;
pub const EX_END_OF_SCRIPT: u8 = 0x53;
const EX_END_PARM_VALUE: u8 = 0x15;

/// Inline operand of a token.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Obj(i32),
    Name(u32, u32),
    U8(u8),
    U16(u16),
    I32(i32),
    F32(f32),
    Str(String),
    Raw(Vec<u8>),
}

/// One decoded expression: opcode, its position on disk and in memory, inline operands, sub-expressions.
#[derive(Clone, Debug)]
pub struct Expr {
    pub op: u8,
    pub disk: usize,
    pub mem: usize,
    pub operands: Vec<Operand>,
    pub children: Vec<Expr>,
}

/// Walks a function's bytecode. Object references take 4 bytes on disk and 8 in memory.
pub struct Walker<'a> {
    b: &'a [u8],
    /// Name-table index of `None` in the owning package (terminates label tables).
    none_name: u32,
    pub pos: usize,
    pub mem: usize,
}

impl<'a> Walker<'a> {
    pub fn new(b: &'a [u8], none_name: u32) -> Self {
        Self { b, none_name, pos: 0, mem: 0 }
    }

    fn take(&mut self, n: usize, mem: usize) -> Result<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos + n).ok_or_else(|| bad(format!("bytecode truncated at {}", self.pos)))?;
        self.pos += n;
        self.mem += mem;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1, 1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2, 2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4, 4)?.try_into().unwrap()))
    }
    fn obj(&mut self) -> Result<Operand> {
        Ok(Operand::Obj(i32::from_le_bytes(self.take(4, 8)?.try_into().unwrap())))
    }
    fn name(&mut self) -> Result<Operand> {
        let s = self.take(8, 8)?;
        Ok(Operand::Name(u32::from_le_bytes(s[..4].try_into().unwrap()), u32::from_le_bytes(s[4..].try_into().unwrap())))
    }
    fn cstr(&mut self, unit: usize) -> Result<Operand> {
        let mut units = Vec::new();
        loop {
            let c = self.take(unit, unit)?;
            let v = if unit == 1 { c[0] as u16 } else { u16::from_le_bytes([c[0], c[1]]) };
            if v == 0 {
                return Ok(Operand::Str(String::from_utf16_lossy(&units)));
            }
            units.push(v);
        }
    }
    /// Expressions up to and including EX_EndFunctionParms (which is not kept).
    fn params(&mut self, out: &mut Vec<Expr>) -> Result<()> {
        loop {
            let e = self.expr()?;
            if e.op == EX_END_FUNCTION_PARMS {
                return Ok(());
            }
            out.push(e);
        }
    }
    fn expr_end(&mut self) -> Result<()> {
        match self.expr()?.op {
            EX_END_FUNCTION_PARMS => Ok(()),
            op => Err(bad(format!("expected EX_EndFunctionParms, got {op:#04x} at {}", self.pos - 1))),
        }
    }

    /// Reads one expression.
    pub fn expr(&mut self) -> Result<Expr> {
        let (disk, mem) = (self.pos, self.mem);
        let op = self.u8()?;
        let (mut o, mut c) = (Vec::new(), Vec::new());
        match op {
            0x00 | 0x01 | 0x02 | 0x48 | 0x20 | 0x29 | 0x3A | 0x5E => o.push(self.obj()?), // variables, ObjectConst, NativeParm, ReturnNothing
            0x03 => {
                c.push(self.expr()?);
                o.push(Operand::Raw(self.take(4, 4)?.to_vec())); // switch on an expression + 4-byte size (E-060)
            }
            0x04 | 0x0D | 0x2D | 0x36 | 0x51 => c.push(self.expr()?), // Return, GotoLabel, BoolVariable, DynArrayLength, InterfaceContext
            0x05 => {
                o.push(self.obj()?);
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?); // Switch
            }
            0x06 | 0x5A => o.push(Operand::U16(self.u16()?)), // Jump; 0x5A skip only (E-060)
            0x07 | 0x18 => {
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?); // JumpIfNot, Skip
            }
            0x08 | 0x0B | 0x15 | 0x16 | 0x17 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2A | 0x30 | 0x31 | 0x3F | 0x4A | 0x53 => {}
            0x09 => {
                o.push(Operand::U16(self.u16()?));
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?); // Assert
            }
            0x0A => {
                let offset = self.u16()?;
                o.push(Operand::U16(offset));
                if offset != 0xFFFF {
                    c.push(self.expr()?); // Case
                }
            }
            0x0C => loop {
                // LabelTable (state code): (name, u32 offset) entries ending with the name None.
                let name = self.name()?;
                o.push(name.clone());
                o.push(Operand::I32(self.i32()?));
                if matches!(name, Operand::Name(i, _) if i == self.none_name) {
                    break;
                }
            },
            0x0E | 0x13 | 0x2E | 0x52 => {
                o.push(self.obj()?);
                c.push(self.expr()?); // EatReturnValue, MetaCast, DynamicCast, InterfaceCast
            }
            0x0F | 0x10 | 0x14 | 0x1A | 0x44 | 0x54 | 0x3B | 0x3C | 0x3D | 0x3E | 0x5F => {
                c.push(self.expr()?);
                c.push(self.expr()?); // Let, element access, LetBool, LetDelegate, DynArrayAdd, delegate compares, 0x5F let (E-060)
            }
            0x11 => {
                for _ in 0..5 {
                    c.push(self.expr()?); // New: outer, name, flags, class, template
                }
            }
            0x12 | 0x19 => {
                c.push(self.expr()?); // ClassContext / Context
                o.push(Operand::U16(self.u16()?));
                o.push(self.obj()?);
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?);
            }
            0x1B | 0x37 => {
                o.push(self.name()?);
                self.params(&mut c)?; // VirtualFunction / GlobalFunction
            }
            0x1C => {
                o.push(self.obj()?);
                self.params(&mut c)?; // FinalFunction
            }
            0x1D => o.push(Operand::I32(self.i32()?)),
            0x1E => o.push(Operand::F32(f32::from_le_bytes(self.take(4, 4)?.try_into().unwrap()))),
            0x1F => o.push(self.cstr(1)?),
            0x21 | 0x4B => o.push(self.name()?),
            0x22 => (0..3).try_for_each(|_| self.i32().map(|v| o.push(Operand::I32(v))))?,
            0x23 => (0..3).try_for_each(|_| self.take(4, 4).map(|v| o.push(Operand::F32(f32::from_le_bytes(v.try_into().unwrap())))))?,
            0x24 | 0x2C => o.push(Operand::U8(self.u8()?)),
            0x2F => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?)); // Iterator
            }
            0x32 | 0x33 => {
                o.push(self.obj()?);
                c.push(self.expr()?);
                c.push(self.expr()?); // StructCmpEq/Ne
            }
            0x34 => o.push(self.cstr(2)?),
            0x35 => {
                o.push(self.obj()?);
                o.push(self.obj()?);
                o.push(Operand::U8(self.u8()?));
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?); // StructMember
            }
            0x38 => {
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?); // PrimitiveCast
            }
            0x39 | 0x40 => (0..3).try_for_each(|_| self.expr().map(|e| c.push(e)))?,
            0x41 => {
                (0..3).try_for_each(|_| self.i32().map(|v| o.push(Operand::I32(v))))?;
                o.push(Operand::U8(self.u8()?)); // DebugInfo
            }
            0x42 => {
                o.push(Operand::U8(self.u8()?));
                o.push(self.obj()?);
                o.push(self.name()?);
                self.params(&mut c)?; // DelegateFunction
            }
            0x43 => {
                o.push(self.name()?);
                o.push(self.obj()?); // DelegateProperty
            }
            0x45 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?); // Conditional
            }
            0x46 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                self.expr_end()?; // DynArrayFind (E-060)
            }
            0x47 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                c.push(self.expr()?);
                self.expr_end()?; // DynArrayFindStruct (E-060)
            }
            0x49 => {
                o.push(Operand::U16(self.u16()?));
                loop {
                    let e = self.expr()?;
                    if e.op == EX_END_PARM_VALUE {
                        break;
                    }
                    c.push(e); // DefaultParmValue
                }
            }
            0x4C..=0x50 => o.push(Operand::Raw(self.take(4, 4)?.to_vec())), // indexed variable (E-060)
            0x55 | 0x56 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                self.expr_end()?; // DynArrayAddItem / RemoveItem
            }
            0x57 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                c.push(self.expr()?);
                self.expr_end()?; // DynArrayInsertItem
            }
            0x58 => {
                c.push(self.expr()?);
                c.push(self.expr()?);
                o.push(Operand::U8(self.u8()?));
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?)); // DynArrayIterator
            }
            0x59 => {
                c.push(self.expr()?);
                o.push(Operand::U16(self.u16()?));
                c.push(self.expr()?);
                self.expr_end()?; // DynArraySort
            }
            0x60..=0x6F => {
                o.push(Operand::U8(self.u8()?));
                self.params(&mut c)?; // extended native: index = ((op & 0x0F) << 8) | byte
            }
            0x70..=0xFF => self.params(&mut c)?, // native function by index
            _ => return Err(bad(format!("unknown opcode {op:#04x} at {disk}"))),
        }
        Ok(Expr { op, disk, mem, operands: o, children: c })
    }

    /// Walks statements until EX_EndOfScript (included as the last statement).
    pub fn walk_function(&mut self) -> Result<Vec<Expr>> {
        let mut statements = Vec::new();
        loop {
            let e = self.expr()?;
            let end = e.op == EX_END_OF_SCRIPT;
            statements.push(e);
            if end {
                return Ok(statements);
            }
        }
    }
}

impl Expr {
    /// Visits this expression and all sub-expressions.
    pub fn visit(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        for c in &self.children {
            c.visit(f);
        }
    }
}
