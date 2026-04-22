//! Simplify CFG.

use yachiyo::base::Type;
use yachiyo::ir::mid::{Builder, Function, Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::arena::Arena;
use yachiyo::utils::r#match::{match_some, match_src};
use yachiyo::utils::set::BitSet;

use rustc_hash::FxHashSet;

#[derive(Default)]
pub struct SimplifyCFG<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  visited: BitSet,
  op_to_bb: Vec<Operand>,
}

impl<'a> SimplifyCFG<'a> {
  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
    self.visited.clear();
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
  fn replace_op(&mut self, op_id: Operand, bb_id: Operand, new_op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_op(&mut self.builder, func_id, op_id, bb_id, new_op)
  }

  // This function is called only if the current block has merely one instruction(The terminator).
  fn elim(&mut self, bb_id: Operand) {
    let func_id = self.builder.current_function.unwrap();
    let (succ_id, preds) = {
      let cfg = &self.get_func(func_id).cfg;
      let bb = &cfg[bb_id];

      assert!(bb.cur.len() != 1);
      assert!(bb.succs.len() != 1);
      (bb.succs[0].0, bb.preds.clone())
    };

    for (pred_id, _) in preds.iter() {
      let pred_last_id = {
        let cfg = &self.get_func(func_id).cfg;
        let pred = &cfg[*pred_id];
        match pred.cur.last() {
          Some(inst_id) => *inst_id,
          None => unreachable!(),
        }
      };

      let updated_pred_last_op = {
        let dfg = &self.get_func(func_id).dfg;
        let mut pred_last_op = dfg[pred_last_id].clone();

        // Replace the target block of the predecessor's terminator with the successor block.
        match_some! {
          target: &mut pred_last_op.data,
          enu: OpData,
          minor_arms: {
            OpData::Jump { target_bb } => {
              assert!(*target_bb == bb_id);
              *target_bb = succ_id;
            }
            OpData::Br {
              then_bb, else_bb, ..
            } => {
              if *then_bb == bb_id {
                *then_bb = succ_id;
              } else if *else_bb == bb_id {
                *else_bb = succ_id;
              } else {
                unreachable!()
              }
            }
          },
          uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, OEq, OGt, OLt, OGe, OLe, ONe, Xor, Shl, Shr, Sar, Sitofp, Fptosi, Zext, Uitofp, Xor, Shr, Sar, Shl, Store, Ret, GEP, Load, Call, Phi, Alloca, GlobalAlloca, Declare],
          uni_arm: {
            unreachable!()
          }
        }
        pred_last_op
      };

      // Replace the old terminator with the new one.
      self.replace_op(pred_last_id, *pred_id, updated_pred_last_op);

      // Update downstream's phi node
    }
  }

  pub fn simplify(&mut self, bb_id: usize) {
    if self.visited.contains(bb_id) {
      return;
    }
    self.visited.insert(bb_id);

    let func_id = self.builder.current_function.unwrap();
    let can_merge = {
      let bb = &self.get_func(func_id).cfg[bb_id];
      // We now ignore those
      bb.preds.len() == 1 && bb.succs.len() == 1
    };

    if can_merge {
      let bb = &self.get_func(func_id).cfg[bb_id];
      // Move the instructions in bb to its successor.
      let pred_id = bb.preds[0].0;
      let (cur, succs) = (bb.cur.clone(), bb.succs.clone());
      let pred = &self.get_func(func_id).cfg[pred_id];
      if pred.succs.len() == 1 && pred.succs[0].0 == Operand::BB(bb_id) {
        // Then merge current block into its predecessor.
        let pred_last = match pred.cur.last() {
          Some(inst_id) => *inst_id,
          None => panic!("SimplifyCFG: The predecessor block should not be empty"),
        };
        // Move the instructions, except the terminator.
        // It's impossible that the current block has any phi instruction.
        for inst_id in cur.iter().skip(cur.len() - 1) {
          self.ir.as_deref_mut().unwrap().move_op_to_bb_at(
            Some(func_id),
            *inst_id,
            Operand::BB(bb_id),
            pred_id,
            Some(pred_last),
          );
        }
      } else {
        // Else move the instructions in current block to its successor.
        let succ_non_phi_pos = cur
          .iter()
          .position(|inst_id| {
            let dfg = &self.get_func(func_id).dfg;
            !dfg[*inst_id].is(OpType::Phi)
          })
          .unwrap_or(0);
        for inst_id in cur.iter().rev().skip(1) {
          self.ir.as_deref_mut().unwrap().move_op_to_bb_at(
            Some(func_id),
            *inst_id,
            Operand::BB(bb_id),
            succs[0].0,
            Some(cur[succ_non_phi_pos]),
          );
        }
      }
      self.elim(Operand::BB(bb_id));
    } else {
      let is_single_jump = {
        let bb = &self.get_func(func_id).cfg[bb_id];
        let dfg = &self.get_func(func_id).dfg;
        bb.cur.len() == 1 && dfg[bb.cur[0]].is(OpType::Jump)
      };
      if is_single_jump {
        self.elim(Operand::BB(bb_id));
      }
    }

    let bb = &self.get_func(func_id).cfg[bb_id];
    if !bb.succs.is_empty() {
      for (succ, _) in bb.succs.clone() {
        self.simplify(succ.get_bb_id());
      }
    } else {
      // Check return statement.
      let dfg = &self.get_func(func_id).dfg;
      if bb.cur.is_empty() {
        panic!("SimplifyCFG: The block should not be empty");
      }

      let last = bb.cur.last().unwrap();
      let op = &dfg[*last];
      if !op.is(OpType::Ret) {
        panic!("SimplifyCFG: The last instruction of a block without successor should be a return instruction");
      }

      let func_ret_typ = match &self.get_func(func_id).typ {
        Type::Function { return_type, .. } => (**return_type).clone(),
        _ => panic!("SimplifyCFG: The current function should have a function type"),
      };
      if op.typ != func_ret_typ {
        panic!("SimplifyCFG: The return type of the return instruction should match the function return type");
      }
    }
  }

  pub fn rewrite(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    // Slay the edge of dead block in phi operations.
    let phis = self
      .ir
      .as_deref_mut()
      .unwrap()
      .get_all_ops(self.builder.current_function, OpType::Phi);
    for phi_op in &phis {
      let op = {
        let dfg = &self.get_func(func_id).dfg;
        dfg[*phi_op].clone()
      };
      if let OpData::Phi { incomings } = op.data {
        for incoming in incomings.iter() {
          if let PhiIncoming::Data { bb, .. } = incoming {
            if let Operand::BB(bb_id) = bb {
              // Check whether the block is dead or the current block is no longer the successor of the incoming block.
              // If so, we need to slay this incoming edge.
              let current_bb = self.op_to_bb[phi_op.get_op_id()];
              let ans_succ = {
                let cfg = &self.get_func(func_id).cfg;
                cfg[*bb_id]
                  .succs
                  .iter()
                  .map(|(succ, _)| *succ)
                  .collect::<Vec<Operand>>()
              };

              if !self.visited.contains(*bb_id) || !ans_succ.contains(&current_bb) {
                self.ir.as_deref_mut().unwrap().slay_phi_incoming(
                  self.builder.current_function,
                  *phi_op,
                  *bb,
                );
              }
            } else {
              panic!("SCCP rewrite: phi incoming bb is not a BB operand");
            }
          }
        }
      } else {
        panic!("SCCP rewrite: op is not a phi node");
      }
    }

    let dead_blocks = self
      .get_func(func_id)
      .cfg
      .collect()
      .into_iter()
      .filter(|bb_id| !self.visited.contains(*bb_id))
      .collect::<FxHashSet<usize>>();

    // Phase 1: Isolate the dead blocks, disconnect the edges from live blocks to dead blocks.
    dead_blocks.iter().for_each(|bb_id| {
      let (last, terminator) = {
        let func = self.get_func(func_id);
        let bb = &func.cfg[*bb_id];
        let last = match bb.cur.last() {
          Some(last) => *last,
          None => return,
        };
        let data = func.dfg[last].data.clone();
        (last, data)
      };
      if matches!(terminator, OpData::Br { .. } | OpData::Jump { .. }) {
        // remove the op
        self.ir.as_deref_mut().unwrap().remove_op(
          self.builder.current_function,
          last,
          Some(Operand::BB(*bb_id)),
        );
      }
    });

    // Phase 2: Check users in dead blocks.
    for bb_id in &dead_blocks {
      let cur = self.get_func(func_id).cfg[*bb_id].cur.clone();

      // Split users check and removal due to data dependency.
      for inst in cur.iter().rev() {
        // inst can be used by the instructions inside the block, but it cannot be used by instructions outside the block.
        let users = {
          let dfg = &self.get_func(func_id).dfg;
          dfg[inst.get_op_id()].users.clone()
        };
        for (user, _) in users {
          let user_bb = self.op_to_bb[user.get_op_id()];
          // The user can be in the same block, or in another dead block. But it cannot be in a live block.
          if dead_blocks.contains(&user_bb.get_bb_id()) {
            // continue. users will be removed later.
            continue;
          }
          let dfg = &self.get_func(func_id).dfg;
          panic!(
            "Builder remove_block: instruction {:#?} has user {:#?} outside the block",
            dfg[inst.get_op_id()],
            dfg[user.get_op_id()]
          );
        }

        // Check whether the instruction uses a value outside dead block. If so, remove the use first.
        let data = {
          let dfg = &self.get_func(func_id).dfg;
          dfg[*inst].data.clone()
        };
        let op = *inst;
        let is_live_value = |operand: &Operand, op_to_bb: &[Operand]| match operand {
          Operand::Value(id) => {
            let bb = op_to_bb[*id].get_bb_id();
            !dead_blocks.contains(&bb)
          }
          _ => false,
        };

        match_src! {
            target: data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: OpData { lhs, rhs } => {
              if is_live_value(&lhs, &self.op_to_bb) {
                self.get_func_mut(func_id).dfg.remove_use(lhs, (op, 0));
                }
              if is_live_value(&rhs, &self.op_to_bb) {
                self.get_func_mut(func_id).dfg.remove_use(rhs, (op, 1));
                }
            },
            un_ops: [Sitofp, Fptosi, Zext, Uitofp],
            un_arm: OpData { value } => {
              if is_live_value(&value, &self.op_to_bb) {
                self.get_func_mut(func_id).dfg.remove_use(value, (op, 0));
                }
            },
            fallback: {
                OpData::Load { addr } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                if is_live_value(&addr, &self.op_to_bb) {
                  self.get_func_mut(func_id).dfg.remove_use(addr, (op, 0));
                    }
                }
                OpData::Store { addr, value } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                if is_live_value(&addr, &self.op_to_bb) {
                  self.get_func_mut(func_id).dfg.remove_use(addr, (op, 0));
                    }
                if is_live_value(&value, &self.op_to_bb) {
                  self.get_func_mut(func_id).dfg.remove_use(value, (op, 1));
                    }
                }
                OpData::Br { cond, .. } => {
                if is_live_value(&cond, &self.op_to_bb) {
                  self.get_func_mut(func_id).dfg.remove_use(cond, (op, 0));
                    }
                }
                OpData::Call { args, .. } => {
                    for (i, arg) in args.iter().enumerate() {
                  if is_live_value(arg, &self.op_to_bb) {
                    self.get_func_mut(func_id).dfg.remove_use(*arg, (op, i + 1));
                        }
                    }
                }
                OpData::Ret { value } => {
                    if let Some(val) = value {
                  if is_live_value(&val, &self.op_to_bb) {
                    self.get_func_mut(func_id).dfg.remove_use(val, (op, 0));
                        }
                    }
                }
                OpData::Phi { incomings } => {
                    for (i, phi_incoming) in incomings.iter().enumerate() {
                        if let PhiIncoming::Data { value, .. } = phi_incoming {
                    if is_live_value(value, &self.op_to_bb) {
                      self.get_func_mut(func_id).dfg.remove_use(*value, (op, i));
                            }
                        }
                    }
                }

                OpData::GEP { base, indices } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                if is_live_value(&base, &self.op_to_bb) {
                  self.get_func_mut(func_id).dfg.remove_use(base, (op, 0));
                    }
                    for (i, index) in indices.iter().enumerate() {
                  if is_live_value(index, &self.op_to_bb) {
                    self.get_func_mut(func_id).dfg.remove_use(*index, (op, i + 1));
                        }
                    }
                }

                OpData::GlobalAlloca(_)
                | OpData::Alloca(_)
                | OpData::Jump { .. }
                | OpData::Declare { .. } => {}
            }
        }
      }
    }

    // Phase 3: Remove the instructions in dead blocks directly by dfg.
    for bb_id in &dead_blocks {
      let cur = self.get_func(func_id).cfg[*bb_id].cur.clone();
      let dfg = &mut self.get_func_mut(func_id).dfg;
      for inst in cur.iter().rev() {
        // Remove the uses
        dfg.remove(inst.get_op_id());
      }
    }

    // Phase 4: Remove the blocks directly by cfg.
    for bb_id in dead_blocks {
      // remove the block from cfg
      self.get_func_mut(func_id).cfg.remove(bb_id);
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
    for func_id in self.ir.as_ref().unwrap().funcs.ids() {
      let func_op = Operand::Func(func_id);
      self.init(func_op);
      let entry = match self.get_func(func_op).cfg.entry {
        Some(entry) => entry,
        None => continue,
      };
      self.simplify(entry);
      self.rewrite();
    }
  }
}
