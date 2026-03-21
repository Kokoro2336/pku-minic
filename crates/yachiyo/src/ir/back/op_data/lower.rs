//! Operand definition for Lower IR instructions.

use crate::ir::back::{BOperand, BOpData};
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
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SubI {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    MulI {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    DivI {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    ModI {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },

    // The comparisons are logical.
    Xor {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },

    // Comparison(S: Signed. And SysY only has signed comparison)
    SNe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SEq {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SGt {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SLt {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SGe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SLe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },

    // Bitwise shift
    Shl {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    Shr {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    Sar {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },

    /// Float
    AddF {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    SubF {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    MulF {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    DivF {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    // Mod is invalid for float in SysY

    // On the language level, SysY doesn't support And, Or, Xor for float

    // Comparison. SysY doesn't support NaN, so we only have one type of comparison here.
    ONe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    OEq {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    OGt {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    OLt {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    OGe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },
    OLe {
        rd: BOperand,
        lhs: BOperand,
        rhs: BOperand,
    },

    /// Cast operations
    Sitofp {
        rd: BOperand,
        value: BOperand,
    }, // int to float
    Fptosi {
        rd: BOperand,
        value: BOperand,
    }, // float to int

    // SysY doesn't support bitwise shift for float
    /// Memory operations
    Store {
        addr: BOperand,
        value: BOperand,
    },
    Load {
        rd: BOperand,
        addr: BOperand,
    },
    Move {
        rd: BOperand,
        src: BOperand,
    },

    // Immediate Loading
    /// Int immediate
    LoadIntImm {
        rd: BOperand,
        imm: i32,
    },
    /// Float immediate
    LoadFloatImm {
        rd: BOperand,
        imm: f32,
    },

    /// Control flow
    /// Call has no return value in Lower IR.
    Call {
        func: BOperand,
    },
    Br {
        cond: BOperand,
        then_bb: BOperand,
        else_bb: BOperand,
    },
    Jump {
        target_bb: BOperand,
    },
    Ret,
}

impl From<LOpData> for BOpData {
    fn from(op_data: LOpData) -> Self {
        BOpData::L(op_data)
    }
}

impl From<BOpData> for LOpData {
    fn from(op_data: BOpData) -> Self {
        match op_data {
            BOpData::L(l_op_data) => l_op_data,
            _ => panic!("Cannot convert MOpData to LOpData"),
        }
    }
}
