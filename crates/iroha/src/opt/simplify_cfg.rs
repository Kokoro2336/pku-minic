//! Simplify CFG.

use yachiyo::analysis::analyze;
use yachiyo::ir::mid::{Builder, Function, Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::arena::Arena;
use yachiyo::utils::r#match::match_some;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::WorklistTrait;

use crate::analysis::Reachability;

#[derive(Default)]
pub struct SimplifyCFG<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  op_to_bb: Vec<Operand>,
  /// Processed dead blocks
  processed: BitSet,
}

impl<'a> SimplifyCFG<'a> {
  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
    // Init op_to_bb mapping.
    let (dfg_len, blocks) = {
      let func = self.get_func(func_id);
      let blocks = func
        .cfg
        .ids()
        .map(|bb| {
          let bb_id = Operand::BB(bb);
          let cur = func.cfg[bb_id].cur.clone();
          (bb_id, cur)
        })
        .collect::<Vec<(Operand, Vec<Operand>)>>();
      (func.dfg.len(), blocks)
    };

    self.processed.clear();
    self.op_to_bb.clear();
    self.op_to_bb.resize(dfg_len, Operand::Undefined);

    for (bb_id, cur) in blocks {
      for inst_id in cur {
        self.op_to_bb[inst_id.get_op_id()] = bb_id;
      }
    }
  }

  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_func_mut(&mut self, func_id: Operand) -> &mut Function {
    &mut self.ir.as_deref_mut().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn replace_all_uses(&mut self, old: Operand, new: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_deref_mut()
      .unwrap()
      .replace_all_uses(func_id, old, new);
  }

  #[inline(always)]
  fn get_src_tuple(&self, op_id: Operand) -> Vec<(&Operand, usize)> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_src_tuple(func_id, op_id)
  }

  #[inline(always)]
  fn slay_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_deref_mut()
      .unwrap()
      .slay_phi_incoming(func_id, phi_id, bb_id);
  }

  #[inline(always)]
  fn append_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand, value: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_deref_mut()
      .unwrap()
      .append_phi_incoming(func_id, phi_id, value, bb_id);
  }

  #[inline(always)]
  fn move_op_to_bb_at(
    &mut self,
    op_id: Operand,
    from_bb: Operand,
    to_bb: Operand,
    before_op: Option<Operand>,
  ) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_deref_mut()
      .unwrap()
      .move_op_to_bb_at(func_id, op_id, from_bb, to_bb, before_op);
    // Update op_to_bb mapping.
    self.op_to_bb[op_id.get_op_id()] = to_bb;
  }

  #[inline(always)]
  fn get_all_ops_in_block(&self, bb_id: Operand, op_typ: OpType) -> Vec<Operand> {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_ref()
      .unwrap()
      .get_all_ops_in_block(func_id, bb_id, op_typ)
  }

  #[inline(always)]
  fn replace_op(&mut self, op_id: Operand, bb_id: Operand, new_op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_op(&mut self.builder, func_id, op_id, bb_id, new_op)
  }

  #[inline(always)]
  fn remove_control_flow(&mut self, op_id: Operand, bb_id: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .remove_control_flow(func_id, op_id, bb_id);
  }

  /// Cut off data flow and control flow edges of dead blocks.
  fn isolate_dead(&mut self, bb_id: Operand) {
    let func_id = self.builder.current_function.unwrap();
    let cur = self.get_func(func_id).cfg[bb_id].cur.clone();
    // Remove control flow edges.
    if let Some(last) = cur.last() {
      self.remove_control_flow(*last, bb_id);
    }
    // Remove the uses of normal instructions
    for inst in cur.iter().rev() {
      let src_tuple = self
        .get_src_tuple(*inst)
        .iter()
        .map(|(src, idx)| (**src, *idx))
        .collect::<Vec<_>>();
      let dfg = &mut self.get_func_mut(func_id).dfg;
      for (src, idx) in src_tuple {
        // remove the use of src in inst
        dfg.remove_use(src, (*inst, idx));
      }
      // CAUTION: If the users of the instruction are phi nodes, we need to slay the edge.
      for (user, _) in dfg[*inst].users.clone() {
        let dfg = &self.get_func_mut(func_id).dfg;
        let op_data = dfg[user].data.clone();
        if let OpData::Phi { incomings } = op_data {
          for incoming in incomings {
            if let PhiIncoming::Data { value, .. } = incoming {
              if value == *inst {
                self.slay_phi_incoming(user, bb_id);
              }
            }
          }
        }
      }
      // RAUW the users
      self.replace_all_uses(*inst, Operand::Undefined);
    }
    // Update the block as processed dead.
    self.processed.insert(bb_id.get_bb_id());
  }

  fn process_moved_instructions(
    &mut self,
    op_id: Operand,
    from_bb: Operand,
    to_bb: Operand,
    before_op: Option<Operand>,
  ) {
    self.move_op_to_bb_at(op_id, from_bb, to_bb, before_op);
    // If a user of the moved instruction is a phi node in the original block, we need to update the phi node to point to the new block.
    let func_id = self.builder.current_function.unwrap();
    let users = self.get_func(func_id).dfg[op_id].users.clone();
    for (user, _) in users {
      let user_op_data = self.get_func(func_id).dfg[user].data.clone();
      if let OpData::Phi { incomings } = user_op_data {
        for incoming in incomings {
          if let PhiIncoming::Data { bb, .. } = incoming {
            if bb == from_bb {
              self.slay_phi_incoming(user, from_bb);
              self.append_phi_incoming(user, to_bb, op_id);
            }
          }
        }
      }
    }
  }

  pub fn simplify(&mut self, bb_id: Operand) {
    let func_id = self.builder.current_function.unwrap();
    let mut is_dead = false;

    // Case 1: 1 pred and the pred has only 1 succ. Then merge current block into its predecessor.
    let bb = &self.get_func(func_id).cfg[bb_id];
    if bb.preds.len() == 1 && {
      let pred_id = bb.preds[0].0;
      let pred = &self.get_func(func_id).cfg[pred_id];
      pred.succs.len() == 1 && pred_id != bb_id
    } {
      is_dead = true;

      let bb = &self.get_func(func_id).cfg[bb_id];
      let (pred_id, cur) = (bb.preds[0].0, bb.cur.clone());
      let pred = &self.get_func(func_id).cfg[pred_id];
      let pred_term_id = match pred.cur.last() {
        Some(id) => *id,
        None => unreachable!(),
      };

      // It's impossible that such block contains any phi nodes.
      for inst in cur.iter().take(cur.len() - 1) {
        self.process_moved_instructions(*inst, bb_id, pred_id, Some(pred_term_id));
      }

      // Replace the terminator of the predecessor with the terminator of the current block.
      let bb = &self.get_func(func_id).cfg[bb_id];
      let bb_term_id = match bb.cur.last() {
        Some(id) => *id,
        None => unreachable!(),
      };
      let bb_term_op = {
        let dfg = &self.get_func(func_id).dfg;
        dfg[bb_term_id].clone()
      };
      self.replace_op(pred_term_id, pred_id, bb_term_op);

      // Update downstream's phi nodes
      let bb = &self.get_func(func_id).cfg[bb_id];
      for (succ_id, _) in bb.succs.clone() {
        let succ_phis = self.get_all_ops_in_block(succ_id, OpType::Phi);
        for phi_id in succ_phis {
          let phi_op_data = self.get_func(func_id).dfg[phi_id].data.clone();
          if let OpData::Phi { incomings } = phi_op_data {
            for incoming in incomings {
              if let PhiIncoming::Data { bb, value } = incoming {
                if bb == bb_id {
                  self.slay_phi_incoming(phi_id, bb_id);
                  self.append_phi_incoming(phi_id, pred_id, value);
                }
              }
            }
          } else {
            unreachable!()
          }
        }
      }
    // Case 2: 1 succ and the block has only terminator and phi nodes.
    } else if bb.succs.len() == 1
      && bb.preds.iter().all(|(pred_id, _)| {
        let pred = &self.get_func(func_id).cfg[*pred_id];
        pred.succs.len() == 1 && *pred_id != bb_id
      })
      && bb.cur.iter().all(|inst_id| {
        let dfg = &self.get_func(func_id).dfg;
        let succs = &bb.succs;
        let op = &dfg[*inst_id];
        let (op_data, users) = (&op.data, &op.users);
        if op_data.is_terminator() {
          true
        } else if let OpData::Phi { .. } = op_data {
          users.iter().all(|(user, _)| {
            let bb_id = self.op_to_bb[user.get_op_id()];
            succs[0].0 == bb_id && self.get_func(func_id).dfg[*user].data.is(OpType::Phi)
          })
        } else {
          false
        }
      })
      && bb.succs[0].0 != bb_id
      // Ignore entry block.
      && bb_id != Operand::BB(self.get_func(func_id).cfg.entry.unwrap())
    {
      is_dead = true;

      // Update the terminators of preds.
      let bb = &self.get_func(func_id).cfg[bb_id];
      let succ_id = bb.succs[0].0;
      let preds = bb.preds.clone();

      for (pred_id, _) in preds {
        let cfg = &mut self.get_func_mut(func_id).cfg;
        let pred = &mut cfg[pred_id];
        let pred_term = match pred.cur.last() {
          Some(id) => *id,
          None => unreachable!(),
        };
        let mut pred_term_op = {
          let dfg = &self.get_func(func_id).dfg;
          dfg[pred_term].clone()
        };

        match_some! {
          target: &mut pred_term_op.data,
          enu: OpData,
          minor_arms: {
            OpData::Br { then_bb, else_bb, .. } => {
              if *then_bb == bb_id {
                *then_bb = succ_id;
              }
              if *else_bb == bb_id {
                *else_bb = succ_id;
              }
            },
            OpData::Jump { target_bb } => {
              if *target_bb == bb_id {
                *target_bb = succ_id;
              }
            }
          },
          uni_ops: [Call, Ret, GEP, Load, Store, GlobalAlloca, Alloca, Declare, Sitofp, Fptosi, Zext, Uitofp, AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Phi],
          uni_arm: {
            unreachable!()
          }
        }
        self.replace_op(pred_term, pred_id, pred_term_op);
      }

      // Update the phi nodes in successor block.
      let succ_phis = self.get_all_ops_in_block(succ_id, OpType::Phi);
      for phi_id in succ_phis {
        let phi_op = &self.get_func_mut(func_id).dfg[phi_id];
        if let OpData::Phi { incomings } = phi_op.data.clone() {
          for incoming in incomings {
            if let PhiIncoming::Data {
              bb: incoming_bb,
              value,
            } = incoming
            {
              // Cut off the old edge first.
              if incoming_bb == bb_id {
                self.slay_phi_incoming(phi_id, bb_id);
              }
              // Check whether the value comes from trampoline.
              let new_incomings = if matches!(value, Operand::Value(_))
                && self.op_to_bb[value.get_op_id()] == bb_id
              {
                let tramp_phi = &self.get_func_mut(func_id).dfg[value];
                if let OpData::Phi {
                  incomings: tramp_incomings,
                } = tramp_phi.data.clone()
                {
                  tramp_incomings
                } else {
                  unreachable!()
                }
              } else {
                let bb = &self.get_func(func_id).cfg[bb_id];
                bb.preds
                  .iter()
                  .map(|(pred_id, _)| PhiIncoming::Data {
                    bb: *pred_id,
                    value,
                  })
                  .collect::<Vec<PhiIncoming>>()
              };
              // Append new edges.
              for incoming in new_incomings {
                if let PhiIncoming::Data {
                  bb: incoming_bb,
                  value: incoming_value,
                } = incoming
                {
                  self.append_phi_incoming(phi_id, incoming_bb, incoming_value);
                } else {
                  unreachable!()
                }
              }
            }
          }
        }
      }
    }

    // Process dead on the fly
    if is_dead {
      self.isolate_dead(bb_id);
    }
  }

  fn rewrite(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    // Run reachability analysis
    let visited = analyze::<Reachability>(self.get_func(func_id));
    let dead_blocks = self
      .get_func(func_id)
      .cfg
      .ids()
      .filter(|bb_id| !visited.contains(*bb_id))
      .map(Operand::BB)
      .collect::<Vec<Operand>>();

    // Process unreachable blocks which is not processed in simplify step.
    for bb_id in dead_blocks.iter() {
      if self.processed.contains(bb_id.get_bb_id()) {
        continue;
      }
      self.isolate_dead(*bb_id);
    }

    // Remove the instructions in dead blocks directly by dfg.
    for bb_id in dead_blocks.iter() {
      let cur = self.get_func(func_id).cfg[*bb_id].cur.clone();
      let dfg = &mut self.get_func_mut(func_id).dfg;
      for inst in cur.iter().rev() {
        dfg.remove(inst.get_op_id());
      }
    }

    // Remove the blocks directly by cfg.
    for bb_id in dead_blocks.iter() {
      // remove the block from cfg
      self.get_func_mut(func_id).cfg.remove(bb_id.get_bb_id());
    }
  }
}

impl<'a> Pass<'a> for SimplifyCFG<'a> {
  fn name(&self) -> &str {
    "SimplifyCFG"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.ir = Some(program);
  }
  fn run(&mut self) {
    // We can only simplify CFG at the end of all other optimizations, since it may change the structure of CFG and thus invalidate the assumptions of other optimizations.
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);

      let mut dfs = self.get_func(func_id).dpo();
      // Reverse post order
      while let Some(bb_id) = dfs.pop_back() {
        self.simplify(bb_id);
      }
      self.rewrite();
    }
  }
}
