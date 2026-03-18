//! Operand definition for Lower IR instructions.

use crate::ir::machine::MType;
use crate::ir::machine::Reg;
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};
use strum_macros::EnumDiscriminants;

#[derive(Debug, Clone, Default)]
pub struct VirtReg {
    pub defs: Vec<LOperand>,
    pub uses: Vec<LOperand>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LOperand {
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

    Undef,
}

#[allow(unused)]
impl LOperand {
    pub fn get_bb_id(&self) -> usize {
        match self {
            LOperand::BB(id) => *id,
            _ => panic!("Not a basic block operand"),
        }
    }
    pub fn get_inst_id(&self) -> usize {
        match self {
            LOperand::Inst(id) => *id,
            _ => panic!("Not an instruction operand"),
        }
    }
    pub fn get_virt_id(&self) -> usize {
        match self {
            LOperand::Reg(Reg::Virt(id)) => *id,
            _ => panic!("Not a virtual register operand"),
        }
    }
    pub fn get_func_id(&self) -> usize {
        match self {
            LOperand::Func(id) => *id,
            _ => panic!("Not a function operand"),
        }
    }
    pub fn hi(imm: i32) -> Self {
        LOperand::IntImm(imm >> 16)
    }
    pub fn lo(imm: i32) -> Self {
        LOperand::IntImm(imm & 0xFFFF)
    }
}

#[derive(Debug, Clone, EnumDiscriminants)]
// Specify the type enum's name
#[strum_discriminants(name(LOpType))]
#[strum_discriminants(derive(Hash, Ord, PartialOrd))]
#[allow(clippy::upper_case_acronyms)]
pub enum LOpData {
    /* regular instructions */
    /// Integer
    AddI {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SubI {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    MulI {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    DivI {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    ModI {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },

    // The comparisons are logical.
    Xor {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },

    // Comparison(S: Signed. And SysY only has signed comparison)
    SNe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SEq {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SGt {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SLt {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SGe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SLe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },

    // Bitwise shift
    Shl {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    Shr {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    Sar {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },

    /// Float
    AddF {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    SubF {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    MulF {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    DivF {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    // Mod is invalid for float in SysY

    // On the language level, SysY doesn't support And, Or, Xor for float

    // Comparison. SysY doesn't support NaN, so we only have one type of comparison here.
    ONe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    OEq {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    OGt {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    OLt {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    OGe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },
    OLe {
        rd: LOperand,
        lhs: LOperand,
        rhs: LOperand,
    },

    /// Cast operations
    Sitofp {
        rd: LOperand,
        value: LOperand,
    }, // int to float
    Fptosi {
        rd: LOperand,
        value: LOperand,
    }, // float to int
    Uitofp {
        rd: LOperand,
        value: LOperand,
    }, // bool to float
    Zext {
        rd: LOperand,
        value: LOperand,
    }, // bool to int

    // SysY doesn't support bitwise shift for float
    /// Memory operations
    Store {
        addr: LOperand,
        value: LOperand,
    },
    Load {
        rd: LOperand,
        addr: LOperand,
    },
    Move {
        rd: LOperand,
        src: LOperand,
    },

    // Immediate Loading
    /// Int immediate
    LoadIntImm {
        rd: LOperand,
        imm: i32,
    },
    /// Float immediate
    LoadFloatImm {
        rd: LOperand,
        imm: f32,
    },

    /// Control flow
    /// Call has no return value in Lower IR.
    Call {
        func: LOperand,
    },
    Br {
        cond: LOperand,
        then_bb: LOperand,
        else_bb: LOperand,
    },
    Jump {
        target_bb: LOperand,
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

impl IndexedArena<LOp> {
}

impl Index<LOperand> for LDFG {
    type Output = LOp;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Inst(id) => &self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}

impl IndexMut<LOperand> for LDFG {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Inst(id) => &mut self[id],
            _ => panic!("Invalid operand index: {:?}", index),
        }
    }
}
