//! Definition of IR module, including global variables and functions.

use crate::ir::mid::{
    BasicBlock, Builder, BuilderGuard, Op, OpData, OpType, Operand, PhiIncoming, CFG, CG, DFG,
};
use crate::utils::arena::{Arena, ArenaItem};
use crate::utils::r#match::{match_some, match_src};

#[derive(Debug, Clone)]
pub struct IR {
    // Including:
    // 1. global variables
    // 2. SysY library functions
    pub globals: DFG,
    // global funcs
    pub funcs: CG,
}

impl IR {
    pub fn new() -> Self {
        Self {
            globals: DFG::new(),
            funcs: CG::new(),
        }
    }

    pub(crate) fn cfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> &mut CFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].cfg
    }

    fn dfg_mut_or_panic(&mut self, current_function: Option<usize>, msg: &str) -> &mut DFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].dfg
    }

    fn cfg_dfg_mut_or_panic(
        &mut self,
        current_function: Option<usize>,
        msg: &str,
    ) -> (&mut CFG, &mut DFG) {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        let func = &mut self.funcs[idx];
        (&mut func.cfg, &mut func.dfg)
    }

    pub fn add_uses(&mut self, current_function: Option<usize>, op: Operand) {
        let dfg = self.dfg_mut_or_panic(current_function, "IR add_uses: no current function");
        let data = dfg[op.get_op_id()].data.clone();

        match_src! {
            target: data,
            bin_ops: [
                AddI, SubI, MulI, DivI, ModI,
                SNe, SEq, SGt, SLt, SGe, SLe,
                Xor, Shl, Shr, Sar,
                AddF, SubF, MulF, DivF,
                ONe, OEq, OGt, OLt, OGe, OLe
            ],
            bin_arm: OpData { lhs, rhs } => {
                dfg.add_use(lhs, (op.clone(), 0));
                dfg.add_use(rhs, (op, 1));
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: OpData { value } => {
                dfg.add_use(value, (op, 0));
            },
            fallback: {
                OpData::Load { addr } => {
                    if matches!(addr, Operand::Global(_)) {
                    } else if matches!(addr, Operand::Value(_)) {
                        dfg.add_use(addr, (op, 0));
                    } else {
                        panic!("IR add_uses: Load address operand is not Value or Global");
                    }
                }
                OpData::Store { addr, value } => {
                    if matches!(addr, Operand::Global(_)) {
                    } else if matches!(addr, Operand::Value(_)) {
                        dfg.add_use(addr, (op.clone(), 0));
                    } else {
                        panic!("IR add_uses: Store address operand is not Value or Global");
                    }
                    dfg.add_use(value, (op, 1));
                }
                OpData::Br { cond, .. } => {
                    dfg.add_use(cond, (op, 0));
                }
                OpData::Call { args, .. } => {
                    for (i, arg) in args.iter().enumerate() {
                        // Func is considered operand index 0.
                        dfg.add_use(arg.clone(), (op.clone(), i + 1));
                    }
                }
                OpData::Ret { value } => {
                    if let Some(val) = value {
                        dfg.add_use(val, (op, 0));
                    }
                }
                OpData::Phi { incomings } => {
                    for (i, phi_incoming) in incomings.iter().enumerate() {
                        if let PhiIncoming::Data { value, .. } = phi_incoming {
                            dfg.add_use(value.clone(), (op.clone(), i));
                        }
                    }
                }
                OpData::GEP { base, indices } => {
                    if matches!(base, Operand::Global(_)) {
                        // TODO
                    } else if matches!(base, Operand::Value(_)) {
                        dfg.add_use(base, (op.clone(), 0));
                    } else {
                        panic!("IR add_uses: GEP base operand is not Value or Global");
                    }
                    for (i, index) in indices.iter().enumerate() {
                        // We start from 1 because the base operand is considered index 0.
                        dfg.add_use(index.clone(), (op.clone(), i + 1));
                    }
                }
                OpData::GlobalAlloca(_)
                | OpData::Alloca(_)
                | OpData::Jump { .. }
                | OpData::Declare { .. } => {}
            }
        }
    }

    pub fn remove_uses(&mut self, current_function: Option<usize>, op: Operand) {
        let dfg = self.dfg_mut_or_panic(current_function, "IR remove_uses: no current function");
        let data = dfg[op.get_op_id()].data.clone();

        match_src! {
            target: data,
            bin_ops: [
                AddI, SubI, MulI, DivI, ModI,
                SNe, SEq, SGt, SLt, SGe, SLe,
                Xor, Shl, Shr, Sar,
                AddF, SubF, MulF, DivF,
                ONe, OEq, OGt, OLt, OGe, OLe
            ],
            bin_arm: OpData { lhs, rhs } => {
                dfg.remove_use(lhs, (op.clone(), 0));
                dfg.remove_use(rhs, (op, 1));
            },
            un_ops: [Sitofp, Fptosi, Uitofp, Zext],
            un_arm: OpData { value } => {
                dfg.remove_use(value, (op, 0));
            },
            fallback: {
                OpData::Load { addr } => {
                    if matches!(addr, Operand::Global(_)) {
                        // TODO
                    } else if matches!(addr, Operand::Value(_)) {
                        dfg.remove_use(addr, (op, 0));
                    } else {
                        panic!("IR remove_uses: Load address operand is not Value or Global");
                    }
                }
                OpData::Store { addr, value } => {
                    if matches!(addr, Operand::Global(_)) {
                        // TODO
                    } else if matches!(addr, Operand::Value(_)) {
                        dfg.remove_use(addr, (op.clone(), 0));
                    } else {
                        panic!("IR remove_uses: Store address operand is not Value or Global");
                    }
                    dfg.remove_use(value, (op, 1));
                }
                OpData::Br { cond, .. } => {
                    dfg.remove_use(cond, (op, 0));
                }
                OpData::Call { args, .. } => {
                    for (i, arg) in args.iter().enumerate() {
                        dfg.remove_use(arg.clone(), (op.clone(), i + 1));
                    }
                }
                OpData::Ret { value } => {
                    if let Some(val) = value {
                        dfg.remove_use(val, (op, 0));
                    }
                }
                OpData::Phi { incomings } => {
                    for (i, phi_incoming) in incomings.iter().enumerate() {
                        if let PhiIncoming::Data { value, .. } = phi_incoming {
                            dfg.remove_use(value.clone(), (op.clone(), i));
                        }
                    }
                }
                OpData::GEP { base, indices } => {
                    if matches!(base, Operand::Global(_)) {
                        // TODO
                    } else if matches!(base, Operand::Value(_)) {
                        dfg.remove_use(base, (op.clone(), 0));
                    } else {
                        panic!("IR remove_uses: GEP base operand is not Value or Global");
                    }
                    for (i, index) in indices.iter().enumerate() {
                        dfg.remove_use(index.clone(), (op.clone(), i + 1));
                    }
                }
                OpData::GlobalAlloca(_)
                | OpData::Alloca(_)
                | OpData::Jump { .. }
                | OpData::Declare { .. } => {}
            }
        }
    }

    pub fn replace_all_uses(
        &mut self,
        current_function: Option<usize>,
        old: Operand,
        new: Operand,
    ) {
        let dfg =
            self.dfg_mut_or_panic(current_function, "IR replace_all_uses: no current function");
        let uses = dfg[old.get_op_id()].users.clone();
        for use_op in uses {
            dfg.replace_use(use_op, old.clone(), new.clone());
        }
    }

    pub fn add_control_flow(&mut self, current_function: Option<usize>, op: Operand, bb: Operand) {
        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "IR add_control_flow: no current function");
        let data = dfg[op.get_op_id()].data.clone();

        match_some! {
            target: data,
            enu: OpData,
            minor_arms: {
                OpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.add_pred(then_bb.clone(), bb.clone());
                    cfg.add_succ(bb.clone(), then_bb);

                    cfg.add_pred(else_bb.clone(), bb.clone());
                    cfg.add_succ(bb, else_bb);
                }
                OpData::Jump { target_bb } => {
                    cfg.add_pred(target_bb.clone(), bb.clone());
                    cfg.add_succ(bb, target_bb);
                }
            },
            uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
            uni_arm: {}
        }
    }

    pub fn remove_control_flow(
        &mut self,
        current_function: Option<usize>,
        op: Operand,
        bb: Operand,
    ) {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "IR remove_control_flow: no current function",
        );
        let data = dfg[op.get_op_id()].data.clone();

        match_some! {
            target: data,
            enu: OpData,
            minor_arms: {
                OpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.remove_pred(then_bb.clone(), bb.clone());
                    cfg.remove_succ(bb.clone(), then_bb);
                    cfg.remove_pred(else_bb.clone(), bb.clone());
                    cfg.remove_succ(bb, else_bb);
                }
                OpData::Jump { target_bb } => {
                    cfg.remove_pred(target_bb.clone(), bb.clone());
                    cfg.remove_succ(bb, target_bb);
                }
            },
            uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
            uni_arm: {}
        }
    }

    pub fn create(
        &mut self,
        builder: &Builder,
        current_function: Option<usize>,
        op: Op,
    ) -> Operand {
        match_some! {
            target: op.data,
            enu: OpData,
            minor_arms: {
                OpData::GlobalAlloca(_) | OpData::Declare { .. } => {
                    let op_id = self.globals.alloc(op);
                    Operand::Global(op_id)
                }
            },
            uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare, Jump, Br],
            uni_arm: {
                let (cfg, dfg) =
                    self.cfg_dfg_mut_or_panic(current_function, "IR create: no current function");

                let new_id = dfg.alloc(op);
                let current_block = if let Some(block) = &builder.current_block {
                    block.get_bb_id()
                } else {
                    panic!("IR create: current_block is None");
                };
                let bb = &mut cfg[current_block];

                let op_id = if let Some(current_inst) = &builder.current_inst {
                    let pos = bb
                        .cur
                        .iter()
                        .position(|id| id.get_op_id() == current_inst.get_op_id())
                        .unwrap_or_else(|| {
                            panic!(
                                "IR create: current_inst {:?} not found in current_block {:?}",
                                current_inst, builder.current_block
                            )
                        });
                    let op_id = Operand::Value(new_id);
                    bb.cur.insert(pos, op_id.clone());
                    op_id
                } else {
                    let op_id = Operand::Value(new_id);
                    bb.cur.push(op_id.clone());
                    op_id
                };

                self.add_uses(current_function, op_id.clone());
                let current_block = builder
                    .current_block
                    .clone()
                    .unwrap_or_else(|| panic!("IR create: current_block is None"));
                self.add_control_flow(current_function, op_id.clone(), current_block);
                op_id
            }
        }
    }

    pub fn create_at_head(
        &mut self,
        builder: &mut Builder,
        current_function: Option<usize>,
        op: Op,
    ) -> Operand {
        let bb_id = match &builder.current_block {
            Some(block) => block.get_bb_id(),
            None => panic!("IR create_at_head: current_block is None"),
        };

        let inst_id = {
            let cfg =
                self.cfg_mut_or_panic(current_function, "IR create_at_head: no current function");
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

    pub fn create_new_block(&mut self, current_function: Option<usize>) -> Operand {
        let cfg =
            self.cfg_mut_or_panic(current_function, "IR create_new_block: no current function");
        let bb_id = cfg.alloc(BasicBlock::default());
        Operand::BB(bb_id)
    }

    pub fn get_all_ops(&mut self, current_function: Option<usize>, op_typ: OpType) -> Vec<Operand> {
        let dfg = self.dfg_mut_or_panic(current_function, "IR get_all_ops: no current function");
        dfg.storage
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                if let ArenaItem::Data(node) = item {
                    if node.is(op_typ) {
                        Some(Operand::Value(idx))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<Operand>>()
    }

    pub fn get_all_ops_in_block(
        &mut self,
        current_function: Option<usize>,
        block: Operand,
        op_typ: OpType,
    ) -> Vec<Operand> {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "IR get_all_ops_in_block: no current function",
        );

        let bb_id = block.get_bb_id();
        let bb = &cfg[bb_id];

        let mut ops = Vec::new();
        for inst in &bb.cur {
            let data = &dfg[inst.get_op_id()];
            if data.is(op_typ) {
                ops.push(inst.clone());
            }
        }
        ops
    }

    pub fn get_all_non_phi_in_block(
        &mut self,
        current_function: Option<usize>,
        block: Operand,
    ) -> Vec<Operand> {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "IR get_all_non_phi_in_block: no current function",
        );

        let bb_id = block.get_bb_id();
        let bb = &cfg[bb_id];

        let mut ops = Vec::new();
        for inst in &bb.cur {
            let data = &dfg[inst.get_op_id()];
            if !data.is(OpType::Phi) {
                ops.push(inst.clone());
            }
        }
        ops
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<usize>,
        op: Operand,
        bb: Option<Operand>,
    ) -> Op {
        if matches!(op, Operand::Global(_)) {
            let removed_op = self.globals.remove(op.get_op_id());
            if !removed_op.users.is_empty() {
                panic!(
                    "IR remove_op: global instruction still has users after removal: {:#?}",
                    removed_op.users
                );
            }
            return removed_op;
        }

        self.remove_uses(current_function, op.clone());
        if let Some(bb_id) = bb.clone() {
            self.remove_control_flow(current_function, op.clone(), bb_id);
        }

        let (cfg, dfg) =
            self.cfg_dfg_mut_or_panic(current_function, "IR remove_op: no current function");

        let op_id = op.get_op_id();
        let bb_id = bb
            .unwrap_or_else(|| {
                panic!(
                    "IR remove_op: bb is None when removing instruction {:?}",
                    op
                )
            })
            .get_bb_id();
        let bb = &mut cfg[bb_id];

        if let Some(pos) = bb.cur.iter().position(|id| id.get_op_id() == op_id) {
            bb.cur.remove(pos);
        } else {
            panic!(
                "IR remove_op: instruction {:?} not found in block {:?}",
                op, bb_id
            );
        }

        let removed_op = dfg.remove(op_id);
        if !removed_op.users.is_empty() {
            panic!(
                "IR remove_op: instruction still has users after removal: {:#?}",
                removed_op.users
            );
        }
        removed_op
    }

    pub fn replace_op(
        &mut self,
        builder: &mut Builder,
        current_function: Option<usize>,
        op_id: Operand,
        bb_id: Operand,
        new_op: Op,
    ) -> Operand {
        let pos = {
            let cfg = self.cfg_mut_or_panic(current_function, "IR replace_op: no current function");
            let bb = &cfg[bb_id.clone()];
            bb.cur
                .iter()
                .position(|id| id.get_op_id() == op_id.get_op_id())
                .unwrap_or_else(|| {
                    panic!(
                        "IR replace_op: instruction {:?} not found in block {:?}",
                        op_id, bb_id
                    )
                })
        };

        let next_inst = {
            let cfg = self.cfg_mut_or_panic(current_function, "IR replace_op: no current function");
            let bb = &cfg[bb_id.get_bb_id()];
            bb.cur.get(pos + 1).cloned()
        };

        {
            let mut guard = BuilderGuard::new(builder);
            guard.set_current_block(bb_id.clone());
            // Create new instruction first.
            guard.set_before_inst(self, current_function, next_inst);
            let new_op_id = self.create(&guard, current_function, new_op);
            // RAUW
            self.replace_all_uses(current_function, op_id.clone(), new_op_id.clone());
            // Remove old instruction.
            self.remove_op(current_function, op_id, Some(bb_id.clone()));
            new_op_id
        }
    }

    pub fn move_op_to_bb_at(
        &mut self,
        current_function: Option<usize>,
        op: Operand,
        old_bb: Operand,
        new_bb: Operand,
        pos: Option<Operand>,
    ) {
        let cfg =
            self.cfg_mut_or_panic(current_function, "IR move_op_to_bb_at: no current function");

        let op_id = op.get_op_id();
        let old_bb_id = old_bb.get_bb_id();

        let old_bb_ref = &mut cfg[old_bb_id];
        if let Some(pos) = old_bb_ref.cur.iter().position(|id| id.get_op_id() == op_id) {
            old_bb_ref.cur.remove(pos);
        } else {
            panic!(
                "IR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
                op, old_bb
            );
        }

        let new_bb_id = new_bb.get_bb_id();
        let new_bb_ref = &mut cfg[new_bb_id];
        if let Some(pos) = pos {
            let pos_id = pos.get_op_id();
            if let Some(pos) = new_bb_ref
                .cur
                .iter()
                .position(|id| id.get_op_id() == pos_id)
            {
                new_bb_ref.cur.insert(pos, op.clone());
            } else {
                panic!(
                    "IR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
                    pos, new_bb
                );
            }
        } else {
            new_bb_ref.cur.push(op.clone());
        }
    }

    pub fn add_phi_incoming(
        &mut self,
        current_function: Option<usize>,
        phi: Operand,
        idx: usize,
        value: Operand,
        bb: Operand,
    ) {
        let dfg =
            self.dfg_mut_or_panic(current_function, "IR add_phi_incoming: no current function");
        let phi_id = phi.get_op_id();

        if let OpData::Phi { incomings } = &mut dfg[phi_id].data {
            incomings[idx] = PhiIncoming::Data {
                value: value.clone(),
                bb,
            };
            dfg.add_use(value, (phi.clone(), idx));
        } else {
            panic!("IR add_phi_incoming: not a phi node");
        }
    }

    /// Set a phi incoming slot to None while preserving arity.
    #[allow(unused)]
    pub fn remove_phi_incoming(
        &mut self,
        current_function: Option<usize>,
        phi: Operand,
        idx: usize,
    ) {
        unimplemented!()
    }

    /// Eliminate the phi edge from the incomings.
    pub fn slay_phi_incoming(
        &mut self,
        current_function: Option<usize>,
        phi: Operand,
        bb: Operand,
    ) {
        let phi_id = phi.get_op_id();

        let dfg = self.dfg_mut_or_panic(
            current_function,
            "IR slay_phi_incoming: no current function",
        );

        if let OpData::Phi { incomings } = dfg[phi_id].data.clone() {
            if let Some(pos) = incomings.iter().position(|inc| {
                if let PhiIncoming::Data { bb: inc_bb, .. } = inc {
                    inc_bb == &bb
                } else {
                    false
                }
            }) {
                if let PhiIncoming::Data { value, .. } = &incomings[pos] {
                    dfg.remove_use(value.clone(), (phi.clone(), pos));
                }

                let updated_incomings = if let OpData::Phi { incomings } = &mut dfg[phi_id].data {
                    // DO NOT use swap_remove here.
                    incomings.remove(pos);
                    incomings.clone()
                } else {
                    panic!("IR slay_phi_incoming: not a phi node");
                };

                // Rewrite the operand index of subsequent incomings in all the used operations' users.
                for (_, incoming) in updated_incomings.iter().enumerate().skip(pos) {
                    if let PhiIncoming::Data {
                        value: Operand::Value(id),
                        ..
                    } = incoming
                    {
                        let uses = &mut dfg[*id].users;
                        for (user, use_idx) in uses.iter_mut() {
                            if user == &phi && *use_idx > pos {
                                // Emm...I know this is fragile, but anyway it's simple and stupid.
                                *use_idx -= 1;
                            }
                        }
                    }
                }
            } else {
                panic!(
                    "IR slay_phi_incoming: no incoming edge from bb {:?} to phi {:?}",
                    bb, phi
                );
            }
        } else {
            panic!("IR slay_phi_incoming: not a phi node");
        }
    }
}

impl Default for IR {
    fn default() -> Self {
        Self::new()
    }
}
