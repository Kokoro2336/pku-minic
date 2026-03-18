//! LFunction definition.

use crate::ir::lower::{LOperand, VirtReg, LCFG, LDFG};
use crate::ir::machine::{FrameInfo, Reg};
use crate::utils::arena::*;
use crate::utils::r#match::match_minor;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type LCG = IndexedArena<LFunction>;
pub type VRegs = IndexedArena<VirtReg>;

#[derive(Debug, Clone)]
pub struct LFunction {
    pub name: String,
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
    pub fn new(name: String) -> Self {
        Self {
            name,
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

impl VRegs {
    pub fn add_use(&mut self, vreg_id: LOperand, use_op_id: LOperand) {
        let op_id = match_minor! {
            target: vreg_id,
            minor_arms: {
                LOperand::Reg(Reg::Virt(id)) => id,
                LOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [
                LOperand::IntImm,
                LOperand::FloatImm,
                LOperand::Func,
                LOperand::Inst,
                LOperand::Slot,
                LOperand::Data,
                LOperand::RoData,
                LOperand::BB,
                LOperand::Undef
            ],
            other_patterns: [],
            uni_arm: return
        };
        let vreg = &mut self[op_id];
        if vreg.uses.contains(&use_op_id) {
            return;
        }
        vreg.uses.push(use_op_id);
    }

    /// op_idx: VReg, use_idx: Inst that uses the VReg.
    pub fn remove_use(&mut self, vreg_id: LOperand, use_op_id: LOperand) {
        let op_id = match_minor! {
            target: vreg_id,
            minor_arms: {
                LOperand::Reg(Reg::Virt(id)) => id,
                LOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [
                LOperand::IntImm,
                LOperand::FloatImm,
                LOperand::Inst,
                LOperand::Func,
                LOperand::Slot,
                LOperand::Data,
                LOperand::RoData,
                LOperand::BB,
                LOperand::Undef
            ],
            other_patterns: [],
            uni_arm: return
        };
        let vreg = &mut self[op_id];
        if let Some(pos) = vreg.uses.iter().position(|x| *x == use_op_id) {
            vreg.uses.swap_remove(pos);
        } else {
            panic!(
                "Use {:?}: not found in users of op {:?}",
                use_op_id, vreg_id
            );
        }
    }
}

impl Index<LOperand> for VRegs {
    type Output = VirtReg;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Reg(Reg::Virt(id)) => self.get(id).unwrap(),
            _ => panic!(
                "VRegs index: expected LOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}

impl IndexMut<LOperand> for VRegs {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Reg(Reg::Virt(id)) => self.get_mut(id).unwrap(),
            _ => panic!(
                "VRegs index_mut: expected LOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}
