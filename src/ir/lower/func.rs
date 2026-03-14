//! LFunction definition.

use crate::ir::lower::{VirtReg, LCFG, LDFG};
use crate::ir::machine::{FrameInfo, MType};
use crate::utils::arena::*;
use crate::ir::lower::LOperand;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type LCG = IndexedArena<LFunction>;
pub type VRegs = IndexedArena<VirtReg>;

#[derive(Debug, Clone)]
pub struct LFunction {
    pub name: String,
    pub typ: MType,
    pub cfg: LCFG,
    pub dfg: LDFG,
    /// Virtual registers used in this function.
    /// Distinct from MidIR, the virtual register is represented as a separate entity
    /// rather than the instruction id like MidIR.
    pub vregs: VRegs,
    /// Stack frame information.
    pub frame_info: FrameInfo,
}

impl LFunction {
    pub fn new(name: String, typ: MType) -> Self {
        Self {
            name,
            typ,
            cfg: LCFG::new(),
            dfg: LDFG::new(),
            vregs: VRegs::new(),
            frame_info: FrameInfo::new(),
        }
    }
}

impl Index<LOperand> for LCG {
    type Output = LFunction;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Func(id) => self.get(id).unwrap(),
            _ => panic!("LCG index: expected LOperand::Func, got {:?}", index),
        }
    }
}

impl IndexMut<LOperand> for LCG {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Func(id) => self.get_mut(id).unwrap(),
            _ => panic!("LCG index_mut: expected LOperand::Func, got {:?}", index),
        }
    }
}

impl Index<LOperand> for VRegs {
    type Output = VirtReg;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Virt(id) => self.get(id).unwrap(),
            _ => panic!("VRegs index: expected LOperand::Virt, got {:?}", index),
        }
    }
}

impl IndexMut<LOperand> for VRegs {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Virt(id) => self.get_mut(id).unwrap(),
            _ => panic!("VRegs index_mut: expected LOperand::Virt, got {:?}", index),
        }
    }
}
