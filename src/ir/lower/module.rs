//! Lower IR module defintion.

use super::{LBuilder, LBuilderGuard, LOp, LOpData, LOperand, LCFG, LCG, LDFG};
use crate::ir::machine::DataInfo;
use crate::utils::arena::ArenaItem;
use crate::utils::r#match::{match_minor, match_ops};

pub struct LowerIR {
    pub data_info: DataInfo,
    pub funcs: LCG,
}

impl Default for LowerIR {
    fn default() -> Self {
        Self::new()
    }
}

impl LowerIR {
    pub fn new() -> Self {
        Self {
            data_info: DataInfo::new(),
            funcs: LCG::new(),
        }
    }

    pub(crate) fn cfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> &mut LCFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].cfg
    }

    fn dfg_mut_or_panic(&mut self, current_function: Option<usize>, msg: &str) -> &mut LDFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].dfg
    }

    fn cfg_dfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> (&mut LCFG, &mut LDFG) {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        let func = &mut self.funcs[idx];
        (&mut func.cfg, &mut func.dfg)
    }

    pub fn add_uses(&mut self, current_function: Option<usize>, op: LOperand) {
        let dfg = self.dfg_mut_or_panic(current_function, "LowerIR add_uses: no current function");
        let data = dfg[op.get_inst_id()].data.clone();

        match_ops! {
            target: data,
            bin_ops: [
                AddI, SubI, MulI, DivI, ModI,
                SNe, SEq, SGt, SLt, SGe, SLe,
                Xor, Shl, Shr, Sar,
                AddF, SubF, MulF, DivF,
                ONe, OEq, OGt, OLt, OGe, OLe
            ],
            bin_arm: LOpData { lhs, rhs } => {
                dfg.add_use(lhs, op.clone());
                dfg.add_use(rhs, op);
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: LOpData { value } => {
                dfg.add_use(value, op);
            },
            fallback: {
                LOpData::Load { addr, .. } => {
                    dfg.add_use(addr, op);
                }
                LOpData::Store { addr, value } => {
                    dfg.add_use(addr, op.clone());
                    dfg.add_use(value, op);
                }
                LOpData::Br { cond, .. } => {
                    dfg.add_use(cond, op);
                }
                LOpData::Move { src, .. } => {
                    dfg.add_use(src, op);
                }
                LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret => {}
            }
        }
    }

    pub fn remove_uses(&mut self, current_function: Option<usize>, op: LOperand) {
        let dfg =
            self.dfg_mut_or_panic(current_function, "LowerIR remove_uses: no current function");
        let data = dfg[op.get_inst_id()].data.clone();

        match_ops! {
            target: data,
            bin_ops: [
                AddI, SubI, MulI, DivI, ModI,
                SNe, SEq, SGt, SLt, SGe, SLe,
                Xor, Shl, Shr, Sar,
                AddF, SubF, MulF, DivF,
                ONe, OEq, OGt, OLt, OGe, OLe
            ],
            bin_arm: LOpData { lhs, rhs } => {
                dfg.remove_use(lhs, op.clone());
                dfg.remove_use(rhs, op);
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: LOpData { value } => {
                dfg.remove_use(value, op);
            },
            fallback: {
                LOpData::Load { addr, .. } => {
                    dfg.remove_use(addr, op);
                }
                LOpData::Store { addr, value } => {
                    dfg.remove_use(addr, op.clone());
                    dfg.remove_use(value, op);
                }
                LOpData::Br { cond, .. } => {
                    dfg.remove_use(cond, op);
                }
                LOpData::Move { src, .. } => {
                    dfg.remove_use(src, op);
                }
                LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret => {}
            }
        }
    }

    pub fn replace_all_uses(
        &mut self,
        current_function: Option<usize>,
        old: LOperand,
        new: LOperand,
    ) {
        let dfg = self.dfg_mut_or_panic(
            current_function,
            "LowerIR replace_all_uses: no current function",
        );
        let uses = dfg[old.get_inst_id()].users.clone();
        for use_op in uses {
            dfg.replace_use(use_op, old.clone(), new.clone());
        }
    }

    pub fn add_control_flow(
        &mut self,
        current_function: Option<usize>,
        op: LOperand,
        bb: LOperand,
    ) {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "LowerIR add_control_flow: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        match_minor! {
            target: data,
            minor_arms: {
                LOpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.add_pred(then_bb.clone(), bb.clone());
                    cfg.add_succ(bb.clone(), then_bb);

                    cfg.add_pred(else_bb.clone(), bb.clone());
                    cfg.add_succ(bb, else_bb);
                }
                LOpData::Jump { target_bb } => {
                    cfg.add_pred(target_bb.clone(), bb.clone());
                    cfg.add_succ(bb, target_bb);
                }
            },
            uni_ops: [
                LOpData::AddI,
                LOpData::SubI,
                LOpData::MulI,
                LOpData::DivI,
                LOpData::ModI,
                LOpData::SNe,
                LOpData::SEq,
                LOpData::SGt,
                LOpData::SLt,
                LOpData::SGe,
                LOpData::SLe,
                LOpData::Xor,
                LOpData::Shl,
                LOpData::Shr,
                LOpData::Sar,
                LOpData::AddF,
                LOpData::SubF,
                LOpData::MulF,
                LOpData::DivF,
                LOpData::ONe,
                LOpData::OEq,
                LOpData::OGt,
                LOpData::OLt,
                LOpData::OGe,
                LOpData::OLe,
                LOpData::Sitofp,
                LOpData::Fptosi,
                LOpData::Move,
                LOpData::Uitofp,
                LOpData::Zext,
                LOpData::Store,
                LOpData::Load,
                LOpData::Call
            ],
            other_patterns: [LOpData::Ret],
            uni_arm: {}
        }
    }

    pub fn remove_control_flow(
        &mut self,
        current_function: Option<usize>,
        op: LOperand,
        bb: LOperand,
    ) {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "LowerIR remove_control_flow: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        match_minor! {
            target: data,
            minor_arms: {
                LOpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.remove_pred(then_bb.clone(), bb.clone());
                    cfg.remove_succ(bb.clone(), then_bb);
                    cfg.remove_pred(else_bb.clone(), bb.clone());
                    cfg.remove_succ(bb, else_bb);
                }
                LOpData::Jump { target_bb } => {
                    cfg.remove_pred(target_bb.clone(), bb.clone());
                    cfg.remove_succ(bb, target_bb);
                }
            },
            uni_ops: [
                LOpData::AddI,
                LOpData::SubI,
                LOpData::MulI,
                LOpData::DivI,
                LOpData::ModI,
                LOpData::SNe,
                LOpData::SEq,
                LOpData::SGt,
                LOpData::SLt,
                LOpData::SGe,
                LOpData::SLe,
                LOpData::Xor,
                LOpData::Shl,
                LOpData::Shr,
                LOpData::Sar,
                LOpData::AddF,
                LOpData::SubF,
                LOpData::MulF,
                LOpData::DivF,
                LOpData::ONe,
                LOpData::OEq,
                LOpData::OGt,
                LOpData::OLt,
                LOpData::OGe,
                LOpData::OLe,
                LOpData::Sitofp,
                LOpData::Fptosi,
                LOpData::Uitofp,
                LOpData::Zext,
                LOpData::Store,
                LOpData::Load,
                LOpData::Move,
                LOpData::Call
            ],
            other_patterns: [LOpData::Ret],
            uni_arm: {}
        }
    }

    pub fn create(
        &mut self,
        builder: &LBuilder,
        current_function: Option<usize>,
        op: LOp,
    ) -> LOperand {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "LowerIR create: no current function");

        let new_id = dfg.alloc(op);
        let current_block = if let Some(block) = &builder.current_block {
            block.get_bb_id()
        } else {
            panic!("LowerIR create: current_block is None");
        };
        let bb = &mut cfg[current_block];

        let op_id = if let Some(current_inst) = &builder.current_inst {
            let pos = bb
                .cur
                .iter()
                .position(|id| id.get_inst_id() == current_inst.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "LowerIR create: current_inst {:?} not found in current_block {:?}",
                        current_inst, builder.current_block
                    )
                });
            let op_id = LOperand::Inst(new_id);
            bb.cur.insert(pos, op_id.clone());
            op_id
        } else {
            let op_id = LOperand::Inst(new_id);
            bb.cur.push(op_id.clone());
            op_id
        };

        self.add_uses(current_function, op_id.clone());
        let current_block = builder
            .current_block
            .clone()
            .unwrap_or_else(|| panic!("LowerIR create: current_block is None"));
        self.add_control_flow(current_function, op_id.clone(), current_block);
        op_id
    }

    pub fn create_at_head(
        &mut self,
        builder: &mut LBuilder,
        current_function: Option<usize>,
        op: LOp,
    ) -> LOperand {
        let bb_id = match &builder.current_block {
            Some(block) => block.get_bb_id(),
            None => panic!("LowerIR create_at_head: current_block is None"),
        };

        let inst_id = {
            let cfg = self.cfg_mut_or_panic(
                current_function,
                "LowerIR create_at_head: no current function",
            );
            let bb = &cfg[bb_id];
            if bb.cur.is_empty() {
                None
            } else {
                Some(bb.cur[0].clone())
            }
        };

        builder.set_before_inst(self, current_function, inst_id);
        self.create(builder, current_function, op)
    }

    pub fn create_new_block(&mut self, current_function: Option<usize>) -> LOperand {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "LowerIR create_new_block: no current function",
        );
        let bb_id = cfg.alloc(super::LBasicBlock::new());
        LOperand::BB(bb_id)
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<usize>,
        op: LOperand,
        bb: Option<LOperand>,
    ) -> LOp {
        self.remove_uses(current_function, op.clone());
        if let Some(bb_id) = bb.clone() {
            self.remove_control_flow(current_function, op.clone(), bb_id);
        }

        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "LowerIR remove_op: no current function");

        let op_id = op.get_inst_id();
        let bb_id = bb
            .unwrap_or_else(|| {
                panic!(
                    "LowerIR remove_op: bb is None when removing instruction {:?}",
                    op
                )
            })
            .get_bb_id();
        let bb = &mut cfg[bb_id];

        if let Some(pos) = bb.cur.iter().position(|id| id.get_inst_id() == op_id) {
            bb.cur.remove(pos);
        } else {
            panic!(
                "LowerIR remove_op: instruction {:?} not found in block {:?}",
                op, bb_id
            );
        }

        let removed_op = match std::mem::replace(&mut dfg.storage[op_id], ArenaItem::None) {
            ArenaItem::Data(data) => data,
            _ => panic!("LowerIR remove_op: dfg slot {} is not data", op_id),
        };
        if !removed_op.users.is_empty() {
            panic!(
                "LowerIR remove_op: instruction still has users after removal: {:#?}",
                removed_op.users
            );
        }
        removed_op
    }

    pub fn replace_op(
        &mut self,
        builder: &mut LBuilder,
        current_function: Option<usize>,
        op_id: LOperand,
        bb_id: LOperand,
        new_op: LOp,
    ) -> LOperand {
        let pos = {
            let cfg =
                self.cfg_mut_or_panic(current_function, "LowerIR replace_op: no current function");
            let bb = &cfg[bb_id.clone()];
            bb.cur
                .iter()
                .position(|id| id.get_inst_id() == op_id.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "LowerIR replace_op: instruction {:?} not found in block {:?}",
                        op_id, bb_id
                    )
                })
        };

        let next_inst = {
            let cfg =
                self.cfg_mut_or_panic(current_function, "LowerIR replace_op: no current function");
            let bb = &cfg[bb_id.get_bb_id()];
            bb.cur.get(pos + 1).cloned()
        };

        let mut guard = LBuilderGuard::new(builder);
        guard.set_current_block(bb_id.clone());
        self.remove_op(current_function, op_id, Some(bb_id));
        guard.set_before_inst(self, current_function, next_inst);
        self.create(&guard, current_function, new_op)
    }

    pub fn move_op_to_bb_at(
        &mut self,
        current_function: Option<usize>,
        op: LOperand,
        old_bb: LOperand,
        new_bb: LOperand,
        pos: Option<LOperand>,
    ) {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "LowerIR move_op_to_bb_at: no current function",
        );

        let op_id = op.get_inst_id();
        let old_bb_id = old_bb.get_bb_id();

        let old_bb_ref = &mut cfg[old_bb_id];
        if let Some(pos) = old_bb_ref
            .cur
            .iter()
            .position(|id| id.get_inst_id() == op_id)
        {
            old_bb_ref.cur.remove(pos);
        } else {
            panic!(
                "LowerIR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
                op, old_bb
            );
        }

        let new_bb_id = new_bb.get_bb_id();
        let new_bb_ref = &mut cfg[new_bb_id];
        if let Some(pos) = pos {
            let pos_id = pos.get_inst_id();
            if let Some(pos) = new_bb_ref
                .cur
                .iter()
                .position(|id| id.get_inst_id() == pos_id)
            {
                new_bb_ref.cur.insert(pos, op);
            } else {
                panic!(
                    "LowerIR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
                    pos, new_bb
                );
            }
        } else {
            new_bb_ref.cur.push(op);
        }
    }
}
