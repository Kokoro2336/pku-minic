//! Operand definition for Lower IR instructions.

use crate::ir::machine::{MOperand, MType};
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};
use strum_macros::EnumDiscriminants;

#[derive(Debug, Clone, EnumDiscriminants)]
// Specify the type enum's name
#[strum_discriminants(name(LOpType))]
#[strum_discriminants(derive(Hash, Ord, PartialOrd))]
#[allow(clippy::upper_case_acronyms)]
pub enum LOpData {
    /* regular instructions */
    /// Integer
    AddI {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SubI {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    MulI {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    DivI {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    ModI {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },

    // The comparisons are logical.
    Xor {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },

    // Comparison(S: Signed. And SysY only has signed comparison)
    SNe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SEq {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SGt {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SLt {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SGe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SLe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },

    // Bitwise shift
    Shl {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    Shr {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    Sar {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },

    /// Float
    AddF {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    SubF {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    MulF {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    DivF {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    // Mod is invalid for float in SysY

    // On the language level, SysY doesn't support And, Or, Xor for float

    // Comparison. SysY doesn't support NaN, so we only have one type of comparison here.
    ONe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    OEq {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    OGt {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    OLt {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    OGe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },
    OLe {
        rd: MOperand,
        lhs: MOperand,
        rhs: MOperand,
    },

    /// Cast operations
    Sitofp {
        rd: MOperand,
        value: MOperand,
    }, // int to float
    Fptosi {
        rd: MOperand,
        value: MOperand,
    }, // float to int
    Uitofp {
        rd: MOperand,
        value: MOperand,
    }, // bool to float
    Zext {
        rd: MOperand,
        value: MOperand,
    }, // bool to int

    // SysY doesn't support bitwise shift for float
    /// Memory operations
    Store {
        addr: MOperand,
        value: MOperand,
    },
    Load {
        rd: MOperand,
        addr: MOperand,
    },
    Move {
        rd: MOperand,
        src: MOperand,
    },

    // Immediate Loading
    /// Int immediate
    LoadIntImm {
        rd: MOperand,
        imm: i32,
    },
    /// Float immediate
    LoadFloatImm {
        rd: MOperand,
        imm: f32,
    },

    /// Control flow
    /// Call has no return value in Lower IR.
    Call {
        func: MOperand,
    },
    Br {
        cond: MOperand,
        then_bb: MOperand,
        else_bb: MOperand,
    },
    Jump {
        target_bb: MOperand,
    },
    Ret,
}

#[derive(Debug, Clone)]
pub enum LAttr {
    Name(String),
}

#[derive(Debug, Clone)]
pub struct LOp {
    pub typ: MType,
    pub attrs: Vec<LAttr>,
    pub data: LOpData,
}

impl LOp {
    pub fn new(typ: MType, attrs: Vec<LAttr>, data: LOpData) -> Self {
        Self { typ, attrs, data }
    }
}

pub type LDFG = IndexedArena<LOp>;

impl IndexedArena<LOp> {}

impl Index<MOperand> for LDFG {
    type Output = LOp;

    fn index(&self, index: MOperand) -> &Self::Output {
        match index {
            MOperand::Inst(id) => &self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}

impl IndexMut<MOperand> for LDFG {
    fn index_mut(&mut self, index: MOperand) -> &mut Self::Output {
        match index {
            MOperand::Inst(id) => &mut self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}
