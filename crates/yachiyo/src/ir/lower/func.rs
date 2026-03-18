//! LFunction definition.

use crate::ir::lower::{LCFG, LDFG};
use crate::ir::machine::{FrameInfo, Reg, MOperand, VirtReg};
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

impl Index<MOperand> for LCG {
    type Output = LFunction;

    fn index(&self, index: MOperand) -> &Self::Output {
        match index {
            MOperand::Func(id) => self.get(id).unwrap(),
            _ => panic!("LCG index: expected MOperand::Func, got {:?}", index),
        }
    }
}

impl IndexMut<MOperand> for LCG {
    fn index_mut(&mut self, index: MOperand) -> &mut Self::Output {
        match index {
            MOperand::Func(id) => self.get_mut(id).unwrap(),
            _ => panic!("LCG index_mut: expected MOperand::Func, got {:?}", index),
        }
    }
}

impl VRegs {
    pub fn add_use(&mut self, vreg_id: MOperand, use_op_id: MOperand) {
        let op_id = match_minor! {
            target: vreg_id,
            minor_arms: {
                MOperand::Reg(Reg::Virt(id)) => id,
                MOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [
                MOperand::IntImm,
                MOperand::FloatImm,
                MOperand::Func,
                MOperand::Inst,
                MOperand::Slot,
                MOperand::Data,
                MOperand::RoData,
                MOperand::BB,
                MOperand::Undef
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
    pub fn remove_use(&mut self, vreg_id: MOperand, use_op_id: MOperand) {
        let op_id = match_minor! {
            target: vreg_id,
            minor_arms: {
                MOperand::Reg(Reg::Virt(id)) => id,
                MOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [
                MOperand::IntImm,
                MOperand::FloatImm,
                MOperand::Inst,
                MOperand::Func,
                MOperand::Slot,
                MOperand::Data,
                MOperand::RoData,
                MOperand::BB,
                MOperand::Undef
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

impl Index<MOperand> for VRegs {
    type Output = VirtReg;

    fn index(&self, index: MOperand) -> &Self::Output {
        match index {
            MOperand::Reg(Reg::Virt(id)) => self.get(id).unwrap(),
            _ => panic!(
                "VRegs index: expected MOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}

impl IndexMut<MOperand> for VRegs {
    fn index_mut(&mut self, index: MOperand) -> &mut Self::Output {
        match index {
            MOperand::Reg(Reg::Virt(id)) => self.get_mut(id).unwrap(),
            _ => panic!(
                "VRegs index_mut: expected MOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}
