//! Definition of Lower IR (LIR) instructions.

use super::{BType, Reg};
use crate::debug::info;
use crate::ir::back::LOpData;
use crate::ir::back::MOpData;
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};

#[derive(Debug, Clone, Default)]
pub struct VirtReg {
    pub defs: Vec<BOperand>,
    /// (OpId of uses, operand idx in the use instruction)
    pub uses: Vec<(BOperand, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BOperand {
    Func(usize),
    BB(usize),
    Inst(usize),
    Reg(Reg),

    // Immediate
    IntImm(i32),
    /// Float immediate, stored as its bit representation for the convenience of hashing.
    FloatImm(u32),

    /// Id of frame slot
    Slot(usize),
    /// Id of .data arena.
    Data(usize),
    /// Id of .rodata arena.
    RoData(usize),

    /// External function
    Extern(&'static str),

    #[default]
    /// If an instruction instance is created with its rd undef, the builder will automatically assign it a new virtual register.
    /// Else if the undef lies in other kinds of operands, then the operand is really undefined.
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
            BOperand::Extern(name) => write!(f, "extern.{name}"),
            BOperand::Undef => write!(f, "undef"),
        }
    }
}

#[allow(unused)]
impl BOperand {
    #[inline(always)]
    pub fn get_bb_id(&self) -> usize {
        match self {
            BOperand::BB(id) => *id,
            _ => panic!("Not a basic block operand: {:?}", self),
        }
    }
    #[inline(always)]
    pub fn get_inst_id(&self) -> usize {
        match self {
            BOperand::Inst(id) => *id,
            _ => panic!("Not an instruction operand: {:?}", self),
        }
    }
    #[inline(always)]
    pub fn get_virt_id(&self) -> usize {
        match self {
            BOperand::Reg(Reg::Virt(id)) => *id,
            _ => panic!("Not a virtual register operand: {:?}", self),
        }
    }
    #[inline(always)]
    pub fn get_func_id(&self) -> usize {
        match self {
            BOperand::Func(id) => *id,
            _ => panic!("Not a function operand: {:?}", self),
        }
    }
    #[inline(always)]
    pub fn hi(imm: i32) -> Self {
        BOperand::IntImm(imm >> 16)
    }
    #[inline(always)]
    pub fn lo(imm: i32) -> Self {
        BOperand::IntImm(imm & 0xFFFF)
    }
    #[inline(always)]
    pub fn is_literal(&self) -> bool {
        matches!(self, BOperand::IntImm(_) | BOperand::FloatImm(_))
    }
    #[inline(always)]
    pub fn is_phys(&self) -> bool {
        matches!(self, BOperand::Reg(Reg::X(_)) | BOperand::Reg(Reg::F(_)))
    }
    #[inline(always)]
    pub fn is_virt(&self) -> bool {
        matches!(self, BOperand::Reg(Reg::Virt(_)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BAttr {
    Name(String),
    /// Indicates that this move is a phi move. If an instruction has this attribute, ISel won't create.
    PhiMove,
    /// For call instructions, indicates the operand is a return value.
    ImplicitDef(BOperand),
    /// For call instructions, indicates the operand is a used value that is not explicitly passed in the operand list,
    /// e.g. caller-saved registers.
    /// Ret value operand is also considered implicit use, since it's not explicitly passed in the operand list of the call instruction.
    ImplicitUse(Vec<BOperand>),
}

#[derive(Debug, Clone)]
pub struct BOp {
    pub typ: BType,
    pub attrs: Vec<BAttr>,
    pub data: BOpData,
}

impl BOp {
    pub fn new(typ: BType, attrs: Vec<BAttr>, data: BOpData) -> Self {
        Self { typ, attrs, data }
    }
}

#[derive(Debug, Clone)]
pub enum BOpData {
    M(MOpData),
    L(LOpData),
}

impl BOpData {
    pub fn is_move(&self) -> bool {
        match self {
            BOpData::M(mop_data) => matches!(mop_data, MOpData::Mv { .. } | MOpData::FmvS { .. }),
            BOpData::L(lop_data) => matches!(lop_data, LOpData::Move { .. }),
        }
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

impl From<BOperand> for usize {
    fn from(operand: BOperand) -> Self {
        match operand {
            BOperand::Func(id) => id,
            BOperand::BB(id) => id,
            BOperand::Inst(id) => id,
            BOperand::Reg(Reg::Virt(id)) => id,
            _ => panic!("Cannot convert operand {:?} to usize", operand),
        }
    }
}

impl Arena<BOp> for BDFG {
    fn remove(&mut self, idx: usize) -> BOp {
        if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
            data
        } else {
            panic!("BDFG remove: index {} points to None or NewIndex", idx);
        }
    }
    fn gc(&mut self) -> Vec<ArenaItem<BOp>> {
        let new_arena: Vec<ArenaItem<BOp>> = vec![];
        let mut old_arena = std::mem::replace(&mut self.storage, new_arena);

        // Transport
        old_arena.iter_mut().for_each(|item| {
            if matches!(item, ArenaItem::Data(_)) {
                let new_idx = self.storage.len();
                let data = item.replace(new_idx);
                self.storage.push(data);
            }
        });

        info!(
            "BDFG GC: {} instructions collected, recycle rate: {:.2}%",
            old_arena.len() - self.storage.len(),
            (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
        );

        // No entry. No need to remap.
        // Values of BOp are virtual registers, which should be remapped outside of this function.

        old_arena
    }
}
