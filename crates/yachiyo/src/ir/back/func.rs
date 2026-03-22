//! BFunction definition.

use crate::ir::back::{BOperand, FrameInfo, Reg, VirtReg, BCFG, BDFG};
use crate::utils::arena::*;
use crate::utils::r#match::match_some;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type BCG = IndexedArena<BFunction>;
pub type VRegs = IndexedArena<VirtReg>;

#[derive(Debug, Clone)]
pub struct BFunction {
    pub name: String,
    pub cfg: BCFG,
    pub dfg: BDFG,
    /// Virtual registers used in this function.
    /// Distinct from MidIR, the virtual register is represented as a separate entity
    /// rather than the instruction id like MidIR.
    pub vregs: VRegs,
    /// Stack frame information.
    pub frame_info: FrameInfo,
}

impl BFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            cfg: BCFG::new(),
            dfg: BDFG::new(),
            vregs: VRegs::new(),
            frame_info: FrameInfo::new(),
        }
    }
}

impl Index<BOperand> for BCG {
    type Output = BFunction;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::Func(id) => self.get(id).unwrap(),
            _ => panic!("BCG index: expected BOperand::Func, got {:?}", index),
        }
    }
}

impl IndexMut<BOperand> for BCG {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::Func(id) => self.get_mut(id).unwrap(),
            _ => panic!("BCG index_mut: expected BOperand::Func, got {:?}", index),
        }
    }
}

impl VRegs {
    pub fn add_use(&mut self, vreg_id: BOperand, use_op_id: BOperand) {
        let op_id = match_some! {
            target: vreg_id,
            enu: BOperand,
            minor_arms: {
                BOperand::Reg(Reg::Virt(id)) => id,
                BOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [IntImm, FloatImm, Func, Inst, Slot, Data, RoData, BB, Undef],
            uni_arm: return
        };
        let vreg = &mut self[op_id];
        if vreg.uses.contains(&use_op_id) {
            return;
        }
        vreg.uses.push(use_op_id);
    }

    /// op_idx: VReg, use_idx: Inst that uses the VReg.
    pub fn remove_use(&mut self, vreg_id: BOperand, use_op_id: BOperand) {
        let vreg_id = match_some! {
            target: vreg_id,
            enu: BOperand,
            minor_arms: {
                BOperand::Reg(Reg::Virt(id)) => id,
                BOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, BB, Undef],
            uni_arm: return
        };
        let vreg = &mut self[vreg_id];
        if let Some(pos) = vreg.uses.iter().position(|x| *x == use_op_id) {
            vreg.uses.swap_remove(pos);
        } else {
            panic!(
                "Use {:?}: not found in users of op {:?}",
                use_op_id, vreg_id
            );
        }
    }

    pub fn remove_def(&mut self, vreg_id: BOperand, def_op_id: BOperand) {
        let vreg_id = match_some! {
            target: vreg_id,
            enu: BOperand,
            minor_arms: {
                BOperand::Reg(Reg::Virt(id)) => id,
                BOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, BB, Undef],
            uni_arm: return
        };
        let vreg = &mut self[vreg_id];
        if let Some(pos) = vreg.defs.iter().position(|x| *x == def_op_id) {
            vreg.defs.swap_remove(pos);
        } else {
            panic!("Def {:?}: not found in defs of op {:?}", def_op_id, vreg_id);
        }
    }

    pub fn add_def(&mut self, vreg_id: BOperand, def_op_id: BOperand) {
        let vreg_id = match_some! {
            target: vreg_id,
            enu: BOperand,
            minor_arms: {
                BOperand::Reg(Reg::Virt(id)) => id,
                BOperand::Reg(_) => panic!("Expected VirtReg operand, found PhysReg {:?}", vreg_id),
            },
            uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, BB, Undef],
            uni_arm: return
        };
        let vreg = &mut self[vreg_id];
        if vreg.defs.contains(&def_op_id) {
            return;
        }
        vreg.defs.push(def_op_id);
    }
}

impl Index<BOperand> for VRegs {
    type Output = VirtReg;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::Reg(Reg::Virt(id)) => self.get(id).unwrap(),
            _ => panic!(
                "VRegs index: expected BOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}

impl IndexMut<BOperand> for VRegs {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::Reg(Reg::Virt(id)) => self.get_mut(id).unwrap(),
            _ => panic!(
                "VRegs index_mut: expected BOperand::Reg(Reg::Virt), got {:?}",
                index
            ),
        }
    }
}
