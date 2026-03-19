//! Machine IR module definition.

use super::bb::{MBasicBlock, MCFG};
use super::{MBuilder, MBuilderGuard, MOp};
use crate::ir::machine::{DataInfo, FrameInfo, MOperand, RoDataInfo};
use crate::utils::arena::{ArenaItem, IndexedArena};

#[allow(clippy::upper_case_acronyms)]
pub type MDFG = IndexedArena<MOp>;
#[allow(clippy::upper_case_acronyms)]
pub type MCG = IndexedArena<MFunction>;

#[derive(Debug, Clone)]
pub struct MFunction {
    pub name: String,
    pub cfg: MCFG,
    pub dfg: MDFG,
    pub frame_info: FrameInfo,
}

impl MFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            cfg: MCFG::new(),
            dfg: MDFG::new(),
            frame_info: FrameInfo::new(),
        }
    }
}

pub struct MachineIR {
    pub data_info: DataInfo,
    pub rodata_info: RoDataInfo,
    pub funcs: MCG,
}

impl Default for MachineIR {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineIR {
    pub fn new() -> Self {
        Self {
            data_info: DataInfo::new(),
            rodata_info: RoDataInfo::new(),
            funcs: MCG::new(),
        }
    }

    pub(crate) fn cfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> &mut MCFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].cfg
    }

    fn cfg_dfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> (&mut MCFG, &mut MDFG) {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        let func = &mut self.funcs[idx];
        (&mut func.cfg, &mut func.dfg)
    }

    pub fn create(
        &mut self,
        builder: &MBuilder,
        current_function: Option<usize>,
        op: MOp,
    ) -> MOperand {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "MachineIR create: no current function");

        let new_id = dfg.alloc(op);
        let current_block = builder
            .current_block
            .clone()
            .unwrap_or_else(|| panic!("MachineIR create: current_block is None"));
        let bb = &mut cfg[current_block.get_bb_id()];

        if let Some(current_inst) = builder.current_inst.clone() {
            let pos = bb
                .cur
                .iter()
                .position(|id| id.get_inst_id() == current_inst.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "MachineIR create: current_inst {:?} not found in current_block {:?}",
                        current_inst, current_block
                    )
                });
            let op_id = MOperand::Inst(new_id);
            bb.cur.insert(pos, op_id.clone());
            op_id
        } else {
            let op_id = MOperand::Inst(new_id);
            bb.cur.push(op_id.clone());
            op_id
        }
    }

    pub fn create_at_head(
        &mut self,
        builder: &mut MBuilder,
        current_function: Option<usize>,
        op: MOp,
    ) -> MOperand {
        let bb_id = builder
            .current_block
            .clone()
            .unwrap_or_else(|| panic!("MachineIR create_at_head: current_block is None"));

        let inst_id = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR create_at_head: no current function",
            );
            let bb = &cfg[bb_id];
            bb.cur.first().cloned()
        };

        builder.set_before_inst(self, current_function, inst_id);
        self.create(builder, current_function, op)
    }

    pub fn create_new_block(&mut self, current_function: Option<usize>) -> MOperand {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "MachineIR create_new_block: no current function",
        );
        let bb_id = cfg.alloc(MBasicBlock::default());
        MOperand::BB(bb_id)
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<usize>,
        op: MOperand,
        bb: Option<MOperand>,
    ) -> MOp {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "MachineIR remove_op: no current function");

        let op_id = op.get_inst_id();
        let bb_id = bb
            .unwrap_or_else(|| {
                panic!(
                    "MachineIR remove_op: bb is None when removing instruction {:?}",
                    op
                )
            })
            .get_bb_id();
        let bb = &mut cfg[bb_id];

        if let Some(pos) = bb.cur.iter().position(|id| id.get_inst_id() == op_id) {
            bb.cur.remove(pos);
        } else {
            panic!(
                "MachineIR remove_op: instruction {:?} not found in block {:?}",
                op, bb_id
            );
        }

        match std::mem::replace(&mut dfg.storage[op_id], ArenaItem::None) {
            ArenaItem::Data(data) => data,
            _ => panic!("MachineIR remove_op: dfg slot {} is not data", op_id),
        }
    }

    pub fn replace_op(
        &mut self,
        builder: &mut MBuilder,
        current_function: Option<usize>,
        op_id: MOperand,
        bb_id: MOperand,
        new_op: MOp,
    ) -> MOperand {
        let pos = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR replace_op: no current function",
            );
            let bb = &cfg[bb_id.clone()];
            bb.cur
                .iter()
                .position(|id| id.get_inst_id() == op_id.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "MachineIR replace_op: instruction {:?} not found in block {:?}",
                        op_id, bb_id
                    )
                })
        };

        let next_inst = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR replace_op: no current function",
            );
            let bb = &cfg[bb_id.get_bb_id()];
            bb.cur.get(pos + 1).cloned()
        };

        let mut guard = MBuilderGuard::new(builder);
        guard.set_current_block(bb_id.clone());
        self.remove_op(current_function, op_id, Some(bb_id));
        guard.set_before_inst(self, current_function, next_inst);
        self.create(&guard, current_function, new_op)
    }

    pub fn move_op_to_bb_at(
        &mut self,
        current_function: Option<usize>,
        op: MOperand,
        old_bb: MOperand,
        new_bb: MOperand,
        pos: Option<MOperand>,
    ) {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "MachineIR move_op_to_bb_at: no current function",
        );

        let op_id = op.get_inst_id();
        let old_bb_id = old_bb.get_bb_id();

        let old_bb_ref = &mut cfg[old_bb_id];
        if let Some(cur_pos) = old_bb_ref
            .cur
            .iter()
            .position(|id| id.get_inst_id() == op_id)
        {
            old_bb_ref.cur.remove(cur_pos);
        } else {
            panic!(
                "MachineIR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
                op, old_bb
            );
        }

        let new_bb_id = new_bb.get_bb_id();
        let new_bb_ref = &mut cfg[new_bb_id];
        if let Some(pos) = pos {
            let pos_id = pos.get_inst_id();
            if let Some(new_pos) = new_bb_ref
                .cur
                .iter()
                .position(|id| id.get_inst_id() == pos_id)
            {
                new_bb_ref.cur.insert(new_pos, op);
            } else {
                panic!(
                    "MachineIR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
                    pos_id, new_bb
                );
            }
        } else {
            new_bb_ref.cur.push(op);
        }
    }
}
