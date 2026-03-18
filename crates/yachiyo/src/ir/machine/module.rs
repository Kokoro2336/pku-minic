//! Machine IR module definition.

use super::{MBuilder, MBuilderGuard, MOp};
use crate::ir::machine::{DataInfo, FrameInfo, RoDataInfo};
use crate::utils::arena::{ArenaItem, IndexedArena};

#[allow(clippy::upper_case_acronyms)]
pub type MCFG = IndexedArena<MBasicBlock>;
#[allow(clippy::upper_case_acronyms)]
pub type MDFG = IndexedArena<MOp>;
#[allow(clippy::upper_case_acronyms)]
pub type MCG = IndexedArena<MFunction>;

#[derive(Debug, Clone, Default)]
pub struct MBasicBlock {
    pub prev: Vec<usize>,
    pub cur: Vec<usize>,
    pub succs: Vec<usize>,
}

impl MCFG {
    pub fn add_succ(&mut self, bb_idx: usize, succ_idx: usize) {
        if !self[bb_idx].succs.contains(&succ_idx) {
            self[bb_idx].succs.push(succ_idx);
        }
    }

    pub fn add_pred(&mut self, bb_idx: usize, pred_idx: usize) {
        if !self[bb_idx].prev.contains(&pred_idx) {
            self[bb_idx].prev.push(pred_idx);
        }
    }

    pub fn remove_succ(&mut self, bb_idx: usize, succ_idx: usize) {
        if let Some(pos) = self[bb_idx].succs.iter().position(|x| *x == succ_idx) {
            self[bb_idx].succs.swap_remove(pos);
        } else {
            panic!(
                "Remove succ {}: not found in succs of block {}: {:?}",
                succ_idx, bb_idx, self[bb_idx]
            );
        }
    }

    pub fn remove_pred(&mut self, bb_idx: usize, pred_idx: usize) {
        if let Some(pos) = self[bb_idx].prev.iter().position(|x| *x == pred_idx) {
            self[bb_idx].prev.swap_remove(pos);
        } else {
            panic!(
                "Remove pred {}: not found in preds of block {}: {:?}",
                pred_idx, bb_idx, self[bb_idx]
            );
        }
    }
}

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

    fn dfg_mut_or_panic(&mut self, current_function: Option<usize>, msg: &str) -> &mut MDFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].dfg
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
    ) -> usize {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "MachineIR create: no current function");

        let new_id = dfg.alloc(op);
        let current_block = builder
            .current_block
            .unwrap_or_else(|| panic!("MachineIR create: current_block is None"));
        let bb = &mut cfg[current_block];

        if let Some(current_inst) = builder.current_inst {
            let pos = bb
                .cur
                .iter()
                .position(|id| *id == current_inst)
                .unwrap_or_else(|| {
                    panic!(
                        "MachineIR create: current_inst {} not found in current_block {}",
                        current_inst, current_block
                    )
                });
            bb.cur.insert(pos, new_id);
        } else {
            bb.cur.push(new_id);
        }

        new_id
    }

    pub fn create_at_head(
        &mut self,
        builder: &mut MBuilder,
        current_function: Option<usize>,
        op: MOp,
    ) -> usize {
        let bb_id = builder
            .current_block
            .unwrap_or_else(|| panic!("MachineIR create_at_head: current_block is None"));

        let inst_id = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR create_at_head: no current function",
            );
            let bb = &cfg[bb_id];
            bb.cur.first().copied()
        };

        builder.set_before_inst(self, current_function, inst_id);
        self.create(builder, current_function, op)
    }

    pub fn create_new_block(&mut self, current_function: Option<usize>) -> usize {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "MachineIR create_new_block: no current function",
        );
        cfg.alloc(MBasicBlock::default())
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<usize>,
        op_id: usize,
        bb: Option<usize>,
    ) -> MOp {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "MachineIR remove_op: no current function");

        let bb_id =
            bb.unwrap_or_else(|| panic!("MachineIR remove_op: bb is None when removing {}", op_id));
        let bb = &mut cfg[bb_id];

        if let Some(pos) = bb.cur.iter().position(|id| *id == op_id) {
            bb.cur.remove(pos);
        } else {
            panic!(
                "MachineIR remove_op: instruction {} not found in block {}",
                op_id, bb_id
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
        op_id: usize,
        bb_id: usize,
        new_op: MOp,
    ) -> usize {
        let pos = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR replace_op: no current function",
            );
            let bb = &cfg[bb_id];
            bb.cur
                .iter()
                .position(|id| *id == op_id)
                .unwrap_or_else(|| {
                    panic!(
                        "MachineIR replace_op: instruction {} not found in block {}",
                        op_id, bb_id
                    )
                })
        };

        let next_inst = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "MachineIR replace_op: no current function",
            );
            let bb = &cfg[bb_id];
            bb.cur.get(pos + 1).copied()
        };

        let mut guard = MBuilderGuard::new(builder);
        guard.set_current_block(bb_id);
        self.remove_op(current_function, op_id, Some(bb_id));
        guard.set_before_inst(self, current_function, next_inst);
        self.create(&guard, current_function, new_op)
    }

    pub fn move_op_to_bb_at(
        &mut self,
        current_function: Option<usize>,
        op_id: usize,
        old_bb: usize,
        new_bb: usize,
        pos: Option<usize>,
    ) {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "MachineIR move_op_to_bb_at: no current function",
        );

        let old_bb_ref = &mut cfg[old_bb];
        if let Some(cur_pos) = old_bb_ref.cur.iter().position(|id| *id == op_id) {
            old_bb_ref.cur.remove(cur_pos);
        } else {
            panic!(
                "MachineIR move_op_to_bb_at: instruction {} not found in old_bb {}",
                op_id, old_bb
            );
        }

        let new_bb_ref = &mut cfg[new_bb];
        if let Some(pos_id) = pos {
            if let Some(new_pos) = new_bb_ref.cur.iter().position(|id| *id == pos_id) {
                new_bb_ref.cur.insert(new_pos, op_id);
            } else {
                panic!(
                    "MachineIR move_op_to_bb_at: instruction {} not found in new_bb {}",
                    pos_id, new_bb
                );
            }
        } else {
            new_bb_ref.cur.push(op_id);
        }
    }
}
