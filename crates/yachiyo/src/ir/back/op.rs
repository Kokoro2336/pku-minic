//! Definition of Lower IR (LIR) instructions.

use super::{BType, Reg};
use crate::ir::back::LOpData;
use crate::ir::back::MOpData;
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};


#[derive(Debug, Clone, Default)]
pub struct VirtReg {
    pub defs: Vec<BOperand>,
    pub uses: Vec<BOperand>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum BOperand {
    Func(usize),
    BB(usize),
    Inst(usize),
    Reg(Reg),

    // Immediate
    IntImm(i32),
    FloatImm(f32),

    /// Id of frame slot
    Slot(usize),
    /// Id of .data arena.
    Data(usize),
    /// Id of .rodata arena.
    RoData(usize),

    #[default]
    Undef,
}

impl std::fmt::Display for BOperand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BOperand::Func(id) => write!(f, "fn.{id}"),
            BOperand::BB(id) => write!(f, "bb.{id}"),
            BOperand::Inst(id) => write!(f, "inst.{id}"),
            BOperand::Reg(reg) => write!(f, "{reg}"),
            BOperand::IntImm(imm) => write!(f, "{imm}"),
            BOperand::FloatImm(imm) => write!(f, "{imm}"),
            BOperand::Slot(id) => write!(f, "slot.{id}"),
            BOperand::Data(id) => write!(f, "data.{id}"),
            BOperand::RoData(id) => write!(f, "rodata.{id}"),
            BOperand::Undef => write!(f, "undef"),
        }
    }
}

#[allow(unused)]
impl BOperand {
    pub fn get_bb_id(&self) -> usize {
        match self {
            BOperand::BB(id) => *id,
            _ => panic!("Not a basic block operand"),
        }
    }
    pub fn get_inst_id(&self) -> usize {
        match self {
            BOperand::Inst(id) => *id,
            _ => panic!("Not an instruction operand"),
        }
    }
    pub fn get_virt_id(&self) -> usize {
        match self {
            BOperand::Reg(Reg::Virt(id)) => *id,
            _ => panic!("Not a virtual register operand"),
        }
    }
    pub fn get_func_id(&self) -> usize {
        match self {
            BOperand::Func(id) => *id,
            _ => panic!("Not a function operand"),
        }
    }
    pub fn hi(imm: i32) -> Self {
        BOperand::IntImm(imm >> 16)
    }
    pub fn lo(imm: i32) -> Self {
        BOperand::IntImm(imm & 0xFFFF)
    }
}

#[derive(Debug, Clone)]
pub enum BAttr {
    Name(String),
}

#[derive(Debug, Clone)]
pub struct BOp {
    pub typ: BType,
    pub attrs: Vec<BAttr>,
    pub data: BOpData,
}

#[derive(Debug, Clone)]
pub enum BOpData {
    M(MOpData),
    L(LOpData),
}

impl BOp {
    pub fn new(typ: BType, attrs: Vec<BAttr>, data: BOpData) -> Self {
        Self { typ, attrs, data }
    }
}

pub type BDFG = IndexedArena<BOp>;

impl Index<BOperand> for BDFG {
    type Output = BOp;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::Inst(id) => &self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}

impl IndexMut<BOperand> for BDFG {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::Inst(id) => &mut self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}
