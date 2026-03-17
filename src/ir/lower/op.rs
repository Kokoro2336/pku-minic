//! Operand definition for Lower IR instructions.

use crate::base::Type;
use crate::ir::machine::MType;
use crate::ir::machine::Reg;
use crate::utils::arena::*;
use crate::utils::r#match::match_ops;

use std::ops::{Index, IndexMut};
use strum_macros::EnumDiscriminants;

#[derive(Debug, Clone)]
pub struct VirtReg {
    pub inst_id: LOperand,
    pub phys: Option<Reg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LOperand {
    Func(usize),
    BB(usize),
    Inst(usize),
    Virt(usize),
    Phys(Reg),

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
    pub data: LOpData,
    pub users: Vec<LOperand>,
}

impl LOp {
    pub fn new(typ: MType, attrs: Vec<LAttr>, data: LOpData) -> Self {
        Self {
            typ,
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
            | LOperand::Func(_)
            | LOperand::Phys(_)
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
            | LOperand::Phys(_)
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
            | LOperand::Func(_)
            | LOperand::Phys(_)
            | LOperand::Slot(_)
            | LOperand::Data(_)
            | LOperand::BB(_)
            | LOperand::Undef => return,
        };

        let op = &mut self[op_id];
        match_ops! {
            target: &mut op.data,
            bin_ops: [
                AddI, SubI, MulI, DivI, ModI,
                SNe, SEq, SGt, SLt, SGe, SLe,
                Xor, Shl, Shr, Sar,
                AddF, SubF, MulF, DivF,
                ONe, OEq, OGt, OLt, OGe, OLe
            ],
            bin_arm: LOpData { lhs, rhs } => {
                if *lhs == old {
                    *lhs = new.clone();
                }
                if *rhs == old {
                    *rhs = new.clone();
                }
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: LOpData { value } => {
                if *value == old {
                    *value = new.clone();
                }
            },
            fallback: {
                LOpData::Store { addr, value } => {
                    if *addr == old {
                        *addr = new.clone();
                    }
                    if *value == old {
                        *value = new.clone();
                    }
                }
                LOpData::Load { addr, .. } => {
                    if *addr == old {
                        *addr = new.clone();
                    }
                }
                LOpData::Move { src, .. } => {
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
