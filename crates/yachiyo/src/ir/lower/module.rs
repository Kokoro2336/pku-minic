//! Lower IR module defintion.

use super::{LBuilder, LBuilderGuard, LOp, LOpData, LCFG, LCG, LDFG};
use crate::ir::machine::MOperand;
use crate::ir::machine::{DataInfo, RoDataInfo};
use crate::utils::arena::ArenaItem;
use crate::utils::r#match::{match_minor, match_ops};

pub struct LowerIR {
    pub data_info: DataInfo,
    pub rodata_info: RoDataInfo,
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
            rodata_info: RoDataInfo::new(),
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

    pub fn add_uses(&mut self, current_function: Option<usize>, op: MOperand) {
        let dfg = self.dfg_mut_or_panic(current_function, "LowerIR add_uses: no current function");
        let data = dfg[op.get_inst_id()].data.clone();

        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
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
                vregs.add_use(lhs, op.clone());
                vregs.add_use(rhs, op);
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: LOpData { value } => {
                vregs.add_use(value, op);
            },
            fallback: {
                LOpData::Load { addr, .. } => {
                    vregs.add_use(addr, op);
                }
                LOpData::Store { addr, value } => {
                    vregs.add_use(addr, op.clone());
                    vregs.add_use(value, op);
                }
                LOpData::Br { cond, .. } => {
                    vregs.add_use(cond, op);
                }
                LOpData::Move { src, .. } => {
                    vregs.add_use(src, op);
                }
                LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret | LOpData::LoadIntImm {..} | LOpData::LoadFloatImm {..} => {}
            }
        }
    }

    /// Remove the use of the operand's vreg.
    pub fn remove_uses(&mut self, current_function: Option<usize>, op: MOperand) {
        let dfg =
            self.dfg_mut_or_panic(current_function, "LowerIR remove_uses: no current function");
        let data = dfg[op.get_inst_id()].data.clone();

        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
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
                vregs.remove_use(lhs, op.clone());
                vregs.remove_use(rhs, op);
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: LOpData { value } => {
                vregs.remove_use(value, op);
            },
            fallback: {
                LOpData::Load { addr, .. } => {
                    vregs.remove_use(addr, op);
                }
                LOpData::Store { addr, value } => {
                    vregs.remove_use(addr, op.clone());
                    vregs.remove_use(value, op);
                }
                LOpData::Br { cond, .. } => {
                    vregs.remove_use(cond, op);
                }
                LOpData::Move { src, .. } => {
                    vregs.remove_use(src, op);
                }
                LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret | LOpData::LoadIntImm {..} | LOpData::LoadFloatImm {..} => {}
            }
        }
    }

    pub fn replace_all_uses(
        &mut self,
        current_function: Option<usize>,
        old: MOperand,
        new: MOperand,
    ) {
        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
        let uses = vregs[old.clone()].uses.clone();

        for use_op in uses {
            let op_id = match_minor! {
                target: use_op,
                minor_arms: {
                    MOperand::Inst(op_id) => op_id,
                },
                uni_ops: [
                    MOperand::Reg,
                    MOperand::IntImm,
                    MOperand::FloatImm,
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

            let dfg = self.dfg_mut_or_panic(
                current_function,
                "LowerIR replace_all_uses: no current function",
            );
            let op = &mut dfg[op_id];
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

                    LOpData::Call { .. } | LOpData::Jump { .. } | LOpData::Ret | LOpData::LoadIntImm {..} | LOpData::LoadFloatImm {..} => {}
                }
            }

            let vregs = &mut self.funcs[current_function.unwrap()].vregs;
            vregs.remove_use(old.clone(), use_op.clone());
            vregs.add_use(new.clone(), use_op);
        }
    }

    pub fn add_control_flow(
        &mut self,
        current_function: Option<usize>,
        op: MOperand,
        bb: MOperand,
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
                LOpData::Call,
                LOpData::LoadIntImm,
                LOpData::LoadFloatImm
            ],
            other_patterns: [LOpData::Ret],
            uni_arm: {}
        }
    }

    pub fn remove_control_flow(
        &mut self,
        current_function: Option<usize>,
        op: MOperand,
        bb: MOperand,
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
                LOpData::Call,
                LOpData::LoadIntImm,
                LOpData::LoadFloatImm
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
    ) -> MOperand {
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
            let op_id = MOperand::Inst(new_id);
            bb.cur.insert(pos, op_id.clone());
            op_id
        } else {
            let op_id = MOperand::Inst(new_id);
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
    ) -> MOperand {
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

    pub fn create_new_block(&mut self, current_function: Option<usize>) -> MOperand {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "LowerIR create_new_block: no current function",
        );
        let bb_id = cfg.alloc(super::LBasicBlock::default());
        MOperand::BB(bb_id)
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<usize>,
        op: MOperand,
        bb: Option<MOperand>,
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
        // We don't check whether the vreg defined by the removed instruction is empty, since we are not in SSA form anymore.
        removed_op
    }

    pub fn replace_op(
        &mut self,
        builder: &mut LBuilder,
        current_function: Option<usize>,
        op_id: MOperand,
        bb_id: MOperand,
        new_op: LOp,
    ) -> MOperand {
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
        op: MOperand,
        old_bb: MOperand,
        new_bb: MOperand,
        pos: Option<MOperand>,
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
