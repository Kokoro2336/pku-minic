//! Operand definition for Lower IR instructions.

use crate::base::Type;
use crate::ir::machine::MType;
use crate::ir::machine::{FReg, XReg};
use crate::utils::arena::*;
use crate::ir::machine::SlotId;

#[derive(Debug, Clone)]
pub struct VirtReg {
    inst_id: LOperand,
    phys: Option<LOperand>,
}

#[derive(Debug, Clone, Copy)]
pub enum LOperand {
    Inst(usize),
    Virt(usize),
    IntImm(i32),
    FloatImm(f32),
    Func(usize),
    BB(usize),
}

#[derive(Debug, Clone)]
pub enum LOpData {
    // customized instructions for convenience
    GlobalAlloc {
        size: u32,
        align: u32,
    },
    // getelementptr

    /* regular instructions */
    /// Integer
    AddI {
        lhs: LOperand,
        rhs: LOperand,
    },
    SubI {
        lhs: LOperand,
        rhs: LOperand,
    },
    MulI {
        lhs: LOperand,
        rhs: LOperand,
    },
    DivI {
        lhs: LOperand,
        rhs: LOperand,
    },
    ModI {
        lhs: LOperand,
        rhs: LOperand,
    },

    // The comparisons are logical.
    Xor {
        lhs: LOperand,
        rhs: LOperand,
    },

    // Comparison(S: Signed. And SysY only has signed comparison)
    SNe {
        lhs: LOperand,
        rhs: LOperand,
    },
    SEq {
        lhs: LOperand,
        rhs: LOperand,
    },
    SGt {
        lhs: LOperand,
        rhs: LOperand,
    },
    SLt {
        lhs: LOperand,
        rhs: LOperand,
    },
    SGe {
        lhs: LOperand,
        rhs: LOperand,
    },
    SLe {
        lhs: LOperand,
        rhs: LOperand,
    },

    // Bitwise shift
    Shl {
        lhs: LOperand,
        rhs: LOperand,
    },
    Shr {
        lhs: LOperand,
        rhs: LOperand,
    },
    Sar {
        lhs: LOperand,
        rhs: LOperand,
    },

    /// Float
    AddF {
        lhs: LOperand,
        rhs: LOperand,
    },
    SubF {
        lhs: LOperand,
        rhs: LOperand,
    },
    MulF {
        lhs: LOperand,
        rhs: LOperand,
    },
    DivF {
        lhs: LOperand,
        rhs: LOperand,
    },
    // Mod is invalid for float in SysY

    // On the language level, SysY doesn't support And, Or, Xor for float

    // Comparison. SysY doesn't support NaN, so we only have one type of comparison here.
    ONe {
        lhs: LOperand,
        rhs: LOperand,
    },
    OEq {
        lhs: LOperand,
        rhs: LOperand,
    },
    OGt {
        lhs: LOperand,
        rhs: LOperand,
    },
    OLt {
        lhs: LOperand,
        rhs: LOperand,
    },
    OGe {
        lhs: LOperand,
        rhs: LOperand,
    },
    OLe {
        lhs: LOperand,
        rhs: LOperand,
    },

    /// Cast operations
    Sitofp {
        value: LOperand,
    }, // int to float
    Fptosi {
        value: LOperand,
    }, // float to int
    Uitofp {
        value: LOperand,
    }, // bool to float
    Zext {
        value: LOperand,
    }, // bool to int

    // SysY doesn't support bitwise shift for float
    /// Memory operations
    Store {
        addr: LOperand,
        value: LOperand,
    },
    Load {
        addr: LOperand,
    },
    LoadFrameAddr {
        slot_id: SlotId,
    },
    Move {
        src: LOperand,
    },

    /// Control flow
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
pub enum Attr {
    Name(String),
    GlobalArray {
        // if mutable -> .data; else .rodata
        name: String,
        mutable: bool,
        typ: Type,
        // None: zeroinitializer; Some: initializer list
        values: Option<Vec<LOperand>>,
    },
}

#[derive(Debug, Clone)]
pub struct LOp {
    pub typ: MType,
    pub attrs: Vec<Attr>,
    pub data: LOpData,
    pub users: Vec<LOperand>,
}

pub type LDFG = IndexedArena<LOp>;
