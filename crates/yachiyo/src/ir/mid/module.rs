//! Module definition of IR.

use crate::ir::mid::{
  BasicBlock, Builder, BuilderGuard, Op, OpData, OpType, Operand, PhiIncoming, CG, DFG,
};
use crate::utils::arena::{Arena, ArenaItem};
use crate::utils::r#match::match_some;

#[derive(Debug, Clone)]
pub struct IR {
  /// Including:
  /// 1. global variables
  /// 2. SysY library functions
  pub globals: DFG,
  /// global funcs
  pub funcs: CG,
}

impl IR {
  pub fn new() -> Self {
    Self {
      globals: DFG::new(),
      funcs: CG::new(),
    }
  }

  pub fn add_uses(&mut self, current_function: Option<Operand>, op: Operand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<(Operand, usize)>>();
    let current_function = current_function.unwrap();
    let dfg = &mut self.funcs[current_function].dfg;
    for (src, idx) in src_tuples {
      dfg.add_use(src, (op, idx));
    }
  }

  pub fn remove_uses(&mut self, current_function: Option<Operand>, op: Operand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<(Operand, usize)>>();
    let current_function = current_function.unwrap();
    let dfg = &mut self.funcs[current_function].dfg;
    for (src, idx) in src_tuples {
      dfg.remove_use(src, (op, idx));
    }
  }

  pub fn replace_some_uses(
    &mut self,
    current_function: Option<Operand>,
    old: Operand,
    new: Operand,
    user_list: Vec<(Operand, usize)>,
  ) {
    let current_function = current_function.unwrap();
    let dfg = &mut self.funcs[current_function].dfg;
    let users = dfg[old.get_op_id()].users.clone();
    for user in user_list {
      if !users.contains(&user) {
        panic!(
          "IR replace_some_uses: user {:?} not found in users of operand {:?}",
          user, old
        );
      }
      dfg.replace_use(user, old, new);
    }
  }

  pub fn replace_all_uses(
    &mut self,
    current_function: Option<Operand>,
    old: Operand,
    new: Operand,
  ) {
    let current_function = current_function.unwrap();
    let dfg = &mut self.funcs[current_function].dfg;
    let users = dfg[old.get_op_id()].users.clone();
    for use_op in users {
      dfg.replace_use(use_op, old, new);
    }
  }

  pub fn add_control_flow(&mut self, current_function: Option<Operand>, op: Operand, bb: Operand) {
    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);
    let data = dfg[op.get_op_id()].data.clone();

    match_some! {
        target: data,
        enu: OpData,
        minor_arms: {
            OpData::Br {
                then_bb, else_bb, ..
            } => {
                cfg.add_pred(then_bb, (bb, op));
                cfg.add_succ(bb, (then_bb, op));

                cfg.add_pred(else_bb, (bb, op));
                cfg.add_succ(bb, (else_bb, op));
            }
            OpData::Jump { target_bb } => {
                cfg.add_pred(target_bb, (bb, op));
                cfg.add_succ(bb, (target_bb, op));
            }
        },
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
        uni_arm: {}
    }
  }

  pub fn remove_control_flow(
    &mut self,
    current_function: Option<Operand>,
    op: Operand,
    bb: Operand,
  ) {
    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);
    let data = dfg[op.get_op_id()].data.clone();

    match_some! {
        target: data,
        enu: OpData,
        minor_arms: {
            OpData::Br {
                then_bb, else_bb, ..
            } => {
                cfg.remove_pred(then_bb, (bb, op));
                cfg.remove_succ(bb, (then_bb, op));
                cfg.remove_pred(else_bb, (bb, op));
                cfg.remove_succ(bb, (else_bb, op));
            }
            OpData::Jump { target_bb } => {
                cfg.remove_pred(target_bb, (bb, op));
                cfg.remove_succ(bb, (target_bb, op));
            }
        },
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, GlobalAlloca, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Declare],
        uni_arm: {}
    }
  }

  pub fn create(
    &mut self,
    builder: &Builder,
    current_function: Option<Operand>,
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
        uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, Phi, Call, GEP, Sitofp, Fptosi, Uitofp, Zext, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Jump, Br],
        uni_arm: {
          let current_function =
            current_function.unwrap();
          let op_id = {
            let func = &mut self.funcs[current_function];
            let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);

            let new_id = dfg.alloc(op);
            let current_block = if let Some(block) = &builder.current_block {
              block.get_bb_id()
            } else {
              panic!("IR create: current_block is None");
            };
            let bb = &mut cfg[current_block];

            if let Some(current_inst) = &builder.current_inst {
              let pos = bb
                .cur
                .iter()
                .position(|id| id.get_op_id() == current_inst.get_op_id())
                .unwrap();
              let op_id = Operand::Value(new_id);
              bb.cur.insert(pos, op_id);
              op_id
            } else {
              let op_id = Operand::Value(new_id);
              bb.cur.push(op_id);
              op_id
            }
          };

            self.add_uses(Some(current_function), op_id);
            let current_block = builder
                .current_block
                .unwrap();
            self.add_control_flow(Some(current_function), op_id, current_block);
            op_id
        }
    }
  }

  pub fn create_at_head(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    op: Op,
  ) -> Operand {
    let bb_id = match &builder.current_block {
      Some(block) => block.get_bb_id(),
      None => unreachable!(),
    };

    let inst_id = {
      let current_function = current_function.unwrap();
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id];
      if bb.cur.is_empty() {
        None
      } else {
        Some(bb.cur[0])
      }
    };

    builder.set_before_inst(self, current_function, inst_id);
    self.create(builder, current_function, op)
  }

  pub fn create_new_block(&mut self, current_function: Option<Operand>) -> Operand {
    let current_function = current_function.unwrap();
    let cfg = &mut self.funcs[current_function].cfg;
    let bb_id = cfg.alloc(BasicBlock::default());
    Operand::BB(bb_id)
  }

  pub fn get_all_ops(&self, current_function: Option<Operand>, op_typ: OpType) -> Vec<Operand> {
    let dfg = &self.funcs[current_function.unwrap()].dfg;
    dfg
      .storage
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
    &self,
    current_function: Option<Operand>,
    block: Operand,
    op_typ: OpType,
  ) -> Vec<Operand> {
    let (cfg, dfg) = (
      &self.funcs[current_function.unwrap()].cfg,
      &self.funcs[current_function.unwrap()].dfg,
    );

    let bb_id = block.get_bb_id();
    let bb = &cfg[bb_id];

    let mut ops = Vec::new();
    for inst in &bb.cur {
      let data = &dfg[inst.get_op_id()];
      if data.is(op_typ) {
        ops.push(*inst);
      }
    }
    ops
  }

  pub fn get_all_non_phi_in_block(
    &mut self,
    current_function: Option<Operand>,
    block: Operand,
  ) -> Vec<Operand> {
    let current_function = current_function.unwrap();
    let func = &self.funcs[current_function];
    let (cfg, dfg) = (&func.cfg, &func.dfg);

    let bb_id = block.get_bb_id();
    let bb = &cfg[bb_id];

    let mut ops = Vec::new();
    for inst in &bb.cur {
      let data = &dfg[inst.get_op_id()];
      if !data.is(OpType::Phi) {
        ops.push(*inst);
      }
    }
    ops
  }

  pub fn remove_op(
    &mut self,
    current_function: Option<Operand>,
    op: Operand,
    bb: Option<Operand>,
  ) -> Op {
    if matches!(op, Operand::Global(_)) {
      let removed_op = self.globals.remove(op.get_op_id());
      assert!(removed_op.users.is_empty());
      return removed_op;
    }

    self.remove_uses(current_function, op);
    if let Some(bb_id) = bb {
      self.remove_control_flow(current_function, op, bb_id);
    }

    let current_function = current_function.unwrap();
    let func = &mut self.funcs[current_function];
    let (cfg, dfg) = (&mut func.cfg, &mut func.dfg);

    let op_id = op.get_op_id();
    let bb_id = bb.unwrap().get_bb_id();
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
    assert!(removed_op.users.is_empty());
    removed_op
  }

  pub fn replace_op(
    &mut self,
    builder: &mut Builder,
    current_function: Option<Operand>,
    op_id: Operand,
    bb_id: Operand,
    new_op: Op,
  ) -> Operand {
    let pos = {
      let current_function = current_function.unwrap();
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id];
      bb.cur
        .iter()
        .position(|id| id.get_op_id() == op_id.get_op_id())
        .unwrap()
    };

    let next_inst = {
      let current_function = current_function.unwrap();
      let cfg = &mut self.funcs[current_function].cfg;
      let bb = &cfg[bb_id.get_bb_id()];
      bb.cur.get(pos + 1).cloned()
    };

    {
      let mut guard = BuilderGuard::new(builder);
      guard.set_current_block(bb_id);
      // Create new instruction first.
      guard.set_before_inst(self, current_function, next_inst);
      let new_op_id = self.create(&guard, current_function, new_op);
      // RAUW
      self.replace_all_uses(current_function, op_id, new_op_id);
      // Remove old instruction.
      self.remove_op(current_function, op_id, Some(bb_id));
      new_op_id
    }
  }

  pub fn move_op_to_bb_at(
    &mut self,
    current_function: Option<Operand>,
    op: Operand,
    old_bb: Operand,
    new_bb: Operand,
    pos: Option<Operand>,
  ) {
    let current_function = current_function.unwrap();
    let cfg = &mut self.funcs[current_function].cfg;

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
        new_bb_ref.cur.insert(pos, op);
      } else {
        panic!(
          "IR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
          pos, new_bb
        );
      }
    } else {
      new_bb_ref.cur.push(op);
    }
  }

  pub fn get_src_tuple(
    &self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<(&Operand, usize)> {
    let current_function = current_function.unwrap();
    self.funcs[current_function].get_src_tuple(op_id)
  }

  pub fn get_src(&self, current_function: Option<Operand>, op_id: Operand) -> Vec<&Operand> {
    let current_function = current_function.unwrap();
    self.funcs[current_function].get_src(op_id)
  }

  pub fn get_src_tuple_mut(
    &mut self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<(&mut Operand, usize)> {
    let current_function = current_function.unwrap();
    self.funcs[current_function].get_src_tuple_mut(op_id)
  }

  pub fn get_src_mut(
    &mut self,
    current_function: Option<Operand>,
    op_id: Operand,
  ) -> Vec<&mut Operand> {
    let current_function = current_function.unwrap();
    self.funcs[current_function].get_src_mut(op_id)
  }

  /// This function update the phi incoming which has an allocated slot.
  pub fn add_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    idx: usize,
    value: Operand,
    bb: Operand,
  ) {
    let dfg = &mut self.funcs[current_function.unwrap()].dfg;
    let phi_id = phi.get_op_id();

    if let OpData::Phi { incomings } = &mut dfg[phi_id].data {
      incomings[idx] = PhiIncoming::Data { value, bb };
      dfg.add_use(value, (phi, idx));
    } else {
      unreachable!()
    }
  }

  /// This function append a new phi incoming and return the new incoming index.
  pub fn append_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    value: Operand,
    bb: Operand,
  ) {
    let dfg = &mut self.funcs[current_function.unwrap()].dfg;
    let phi_id = phi.get_op_id();

    if let OpData::Phi { incomings } = &mut dfg[phi_id].data {
      incomings.push(PhiIncoming::Data { value, bb });
      let idx = incomings.len() - 1;
      dfg.add_use(value, (phi, idx));
    } else {
      unreachable!()
    }
  }

  /// Set a phi incoming slot to None while preserving arity.
  #[allow(unused)]
  pub fn remove_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    idx: usize,
  ) {
    unimplemented!()
  }

  /// Eliminate the phi edge from the incomings.
  pub fn slay_phi_incoming(
    &mut self,
    current_function: Option<Operand>,
    phi: Operand,
    bb: Operand,
  ) {
    let phi_id = phi.get_op_id();

    let current_function = current_function.unwrap();
    let dfg = &mut self.funcs[current_function].dfg;

    if let OpData::Phi { incomings } = dfg[phi_id].data.clone() {
      if let Some(pos) = incomings.iter().position(|inc| {
        if let PhiIncoming::Data { bb: inc_bb, .. } = inc {
          inc_bb == &bb
        } else {
          false
        }
      }) {
        if let PhiIncoming::Data { value, .. } = &incomings[pos] {
          dfg.remove_use(*value, (phi, pos));
        }

        let updated_incomings = if let OpData::Phi { incomings } = &mut dfg[phi_id].data {
          // DO NOT use swap_remove here.
          incomings.remove(pos);
          incomings.clone()
        } else {
          panic!("IR slay_phi_incoming: not a phi node");
        };

        // Rewrite each shifted incoming's exact use tuple once. A phi may use the
        // same value in multiple incoming slots, so scanning by value alone can
        // accidentally decrement the same use more than once.
        for (new_idx, incoming) in updated_incomings.iter().enumerate().skip(pos) {
          if let PhiIncoming::Data {
            value: Operand::Value(id),
            ..
          } = incoming
          {
            let old_idx = new_idx + 1;
            let uses = &mut dfg[*id].users;
            if let Some((_, use_idx)) = uses
              .iter_mut()
              .find(|(user, use_idx)| user == &phi && *use_idx == old_idx)
            {
              *use_idx = new_idx;
            } else {
              panic!(
                "IR slay_phi_incoming: use tuple ({:?}, {}) not found for {:?}",
                phi, old_idx, incoming
              );
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
