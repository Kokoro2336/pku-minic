//! Operand definition for Lower IR instructions.

use crate::base::Type;
use crate::frontend::ast::Literal;
use crate::ir::machine::MType;
use crate::ir::machine::Reg;
use crate::ir::mid::{Attr, OpData, Operand};
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};

#[derive(Debug, Clone)]
pub struct VirtReg {
    pub inst_id: LOperand,
    pub phys: Option<Reg>,
}

impl VirtReg {
    pub fn new() -> Self {
        Self {
            inst_id: LOperand::Undef,
            phys: None,
        }
    }
    pub fn with_phys(phys: Reg) -> Self {
        Self {
            inst_id: LOperand::Undef,
            phys: Some(phys),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LOperand {
    Func(usize),
    BB(usize),
    Inst(usize),
    Virt(usize),
    Param(usize),

    // Immediate
    IntImm(i32),
    FloatImm(f32),

    /// Id of frame slot
    Slot(usize),
    /// Id of .data arena.
    Data(usize),

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
            LOperand::Virt(id) => *id,
            _ => panic!("Not a virtual register operand"),
        }
    }
    pub fn get_func_id(&self) -> usize {
        match self {
            LOperand::Func(id) => *id,
            _ => panic!("Not a function operand"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LOpData {
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
pub enum LAttr {
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
    pub attrs: Vec<LAttr>,
    pub vreg: LOperand,
    pub data: LOpData,
    pub users: Vec<LOperand>,
}

impl LOp {
    pub fn new(typ: MType, attrs: Vec<LAttr>, data: LOpData) -> Self {
        Self {
            typ,
            vreg: LOperand::Undef,
            attrs,
            data,
            users: vec![],
        }
    }
}

pub type LDFG = IndexedArena<LOp>;

impl IndexedArena<LOp> {
    pub fn add_use(&mut self, op_idx: LOperand, use_idx: LOperand) {
        let op_id = match op_idx {
            LOperand::Inst(op_id) => op_id,
            LOperand::Virt(_)
            | LOperand::IntImm(_)
            | LOperand::FloatImm(_)
            | LOperand::Param(_)
            | LOperand::Func(_)
            | LOperand::Slot(_)
            | LOperand::Data(_)
            | LOperand::BB(_)
            | LOperand::Undef => return,
        };
        let node = &mut self[op_id];
        if node.users.contains(&use_idx) {
            return;
        }
        node.users.push(use_idx);
    }

    pub fn remove_use(&mut self, op_idx: LOperand, use_idx: LOperand) {
        let op_id = match op_idx {
            LOperand::Inst(op_id) => op_id,
            LOperand::Virt(_)
            | LOperand::IntImm(_)
            | LOperand::FloatImm(_)
            | LOperand::Param(_)
            | LOperand::Func(_)
            | LOperand::Slot(_)
            | LOperand::Data(_)
            | LOperand::BB(_)
            | LOperand::Undef => return,
        };
        let node = &mut self[op_id];
        if let Some(pos) = node.users.iter().position(|x| *x == use_idx) {
            node.users.swap_remove(pos);
        } else {
            panic!("Use {:?}: not found in users of op {:?}", use_idx, op_idx);
        }
    }

    pub fn replace_use(&mut self, op_idx: LOperand, old: LOperand, new: LOperand) {
        let op_id = match op_idx {
            LOperand::Inst(op_id) => op_id,
            LOperand::Virt(_)
            | LOperand::IntImm(_)
            | LOperand::FloatImm(_)
            | LOperand::Param(_)
            | LOperand::Func(_)
            | LOperand::Slot(_)
            | LOperand::Data(_)
            | LOperand::BB(_)
            | LOperand::Undef => return,
        };

        let op = &mut self[op_id];
        match &mut op.data {
            LOpData::AddI { lhs, rhs }
            | LOpData::SubI { lhs, rhs }
            | LOpData::MulI { lhs, rhs }
            | LOpData::DivI { lhs, rhs }
            | LOpData::ModI { lhs, rhs }
            | LOpData::SNe { lhs, rhs }
            | LOpData::SEq { lhs, rhs }
            | LOpData::SGt { lhs, rhs }
            | LOpData::SLt { lhs, rhs }
            | LOpData::SGe { lhs, rhs }
            | LOpData::SLe { lhs, rhs }
            | LOpData::Xor { lhs, rhs }
            | LOpData::Shl { lhs, rhs }
            | LOpData::Shr { lhs, rhs }
            | LOpData::Sar { lhs, rhs }
            | LOpData::AddF { lhs, rhs }
            | LOpData::SubF { lhs, rhs }
            | LOpData::MulF { lhs, rhs }
            | LOpData::DivF { lhs, rhs }
            | LOpData::ONe { lhs, rhs }
            | LOpData::OEq { lhs, rhs }
            | LOpData::OGt { lhs, rhs }
            | LOpData::OLt { lhs, rhs }
            | LOpData::OGe { lhs, rhs }
            | LOpData::OLe { lhs, rhs } => {
                if *lhs == old {
                    *lhs = new.clone();
                }
                if *rhs == old {
                    *rhs = new.clone();
                }
            }

            LOpData::Sitofp { value }
            | LOpData::Fptosi { value }
            | LOpData::Uitofp { value }
            | LOpData::Zext { value } => {
                if *value == old {
                    *value = new.clone();
                }
            }
            LOpData::Store { addr, value } => {
                if *addr == old {
                    *addr = new.clone();
                }
                if *value == old {
                    *value = new.clone();
                }
            }
            LOpData::Load { addr } => {
                if *addr == old {
                    *addr = new.clone();
                }
            }
            LOpData::Move { src } => {
                if *src == old {
                    *src = new.clone();
                }
            }
            LOpData::Br { cond, .. } => {
                if *cond == old {
                    *cond = new.clone();
                }
            }

            LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret => {}
        }

        self.remove_use(old, op_idx.clone());
        self.add_use(new, op_idx);
    }
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
