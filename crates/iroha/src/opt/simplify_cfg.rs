//! Simplify CFG.

use yachiyo::base::Type;
use yachiyo::ir::mid::{Builder, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::arena::Arena;
use yachiyo::utils::r#match::{match_some, match_src};
use yachiyo::utils::set::BitSet;

use rustc_hash::FxHashSet;

pub struct SimplifyCFG<'a> {
  pub program: Option<&'a mut IR>,
  builder: Builder,
  visited: BitSet,
  current_function: Option<Operand>,
  op_to_bb: Vec<Operand>,
}

impl<'a> SimplifyCFG<'a> {
  pub fn new() -> Self {
    Self {
      program: None,
      builder: Builder::new(),
      visited: BitSet::new(),
      current_function: None,
      op_to_bb: Vec::new(),
    }
  }

  pub fn init(&mut self, func_id: Operand) {
    self.current_function = Some(func_id);
    self.visited.clear();
  }

  // This function is invoked only if the current block has merely one instruction(The terminator).
  fn elim(&mut self, bb_id: Operand) {
    let (succ_id, preds) = {
      let cfg = &self.program.as_ref().unwrap().funcs[self.current_function.unwrap()].cfg;
      let bb = &cfg[bb_id];
      if bb.cur.len() != 1 {
        panic!("SimplifyCFG: The current block should have only one instruction");
      }
      if bb.succs.len() != 1 {
        panic!("SimplifyCFG: The current block should have only one successor");
      }
      (bb.succs[0].0, bb.preds.clone())
    };

    for (pred_id, _) in preds.iter() {
      let pred_last_id = {
        let cfg = &self.program.as_ref().unwrap().funcs[self.current_function.unwrap()].cfg;
        let pred = &cfg[*pred_id];
        match pred.cur.last() {
          Some(inst_id) => *inst_id,
          None => panic!("SimplifyCFG: The predecessor block should not be empty"),
        }
      };

      let updated_pred_last_op = {
        let dfg = &self.program.as_ref().unwrap().funcs[self.current_function.unwrap()].dfg;
        let mut pred_last_op = dfg[pred_last_id].clone();
        // Replace the target block of the predecessor's terminator with the successor block.
        match_some! {
          target: &mut pred_last_op.data,
          enu: OpData,
          minor_arms: {
            OpData::Jump { target_bb } => {
              if *target_bb == bb_id {
                *target_bb = succ_id;
              } else {
                panic!("SimplifyCFG: The predecessor block should jump to the current block");
              }
            }
            OpData::Br {
              then_bb, else_bb, ..
            } => {
              if *then_bb == bb_id {
                *then_bb = succ_id;
              } else if *else_bb == bb_id {
                *else_bb = succ_id;
              } else {
                panic!("SimplifyCFG: The predecessor block should branch to the current block");
              }
            }
          },
          uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, OEq, OGt, OLt, OGe, OLe, ONe, Xor, Shl, Shr, Sar, Sitofp, Fptosi, Zext, Uitofp, Xor, Shr, Sar, Shl, Store, Ret, GEP, Load, Call, Phi, Alloca, GlobalAlloca, Declare],
          uni_arm: {
            panic!(
              "SimplifyCFG: The predecessor block should end with a jump or branch instruction"
            );
          }
        }
        pred_last_op
      };

      // Replace the old terminator with the new one.
      self.program.as_deref_mut().unwrap().replace_op(
        &mut self.builder,
        self.current_function,
        pred_last_id,
        *pred_id,
        updated_pred_last_op,
      );
    }
  }

  pub fn simplify(&mut self, bb_id: usize) {
    if self.visited.contains(bb_id) {
      return;
    }
    self.visited.insert(bb_id);

    let current_function = self.current_function.unwrap();
    let can_merge = {
      let bb = &self.program.as_ref().unwrap().funcs[current_function].cfg[bb_id];
      // We now ignore those
      bb.preds.len() == 1 && bb.succs.len() == 1
    };

    if can_merge {
      let bb = &self.program.as_ref().unwrap().funcs[current_function].cfg[bb_id];
      // Move the instructions in bb to its successor.
      let pred_id = bb.preds[0].0;
      let (cur, succs) = (bb.cur.clone(), bb.succs.clone());
      let pred = &self.program.as_ref().unwrap().funcs[current_function].cfg[pred_id];
      if pred.succs.len() == 1 && pred.succs[0].0 == Operand::BB(bb_id) {
        // Then merge current block into its predecessor.
        let pred_last = match pred.cur.last() {
          Some(inst_id) => *inst_id,
          None => panic!("SimplifyCFG: The predecessor block should not be empty"),
        };
        // Move the instructions, except the terminator.
        // It's impossible that the current block has any phi instruction.
        for inst_id in cur.iter().skip(cur.len() - 1) {
          self.program.as_deref_mut().unwrap().move_op_to_bb_at(
            Some(current_function),
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
            let dfg = &self.program.as_ref().unwrap().funcs[current_function].dfg;
            !dfg[*inst_id].is(OpType::Phi)
          })
          .unwrap_or(0);
        for inst_id in cur.iter().rev().skip(1) {
          self.program.as_deref_mut().unwrap().move_op_to_bb_at(
            Some(current_function),
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
        let bb = &self.program.as_ref().unwrap().funcs[current_function].cfg[bb_id];
        let dfg = &self.program.as_ref().unwrap().funcs[current_function].dfg;
        bb.cur.len() == 1 && dfg[bb.cur[0]].is(OpType::Jump)
      };
      if is_single_jump {
        self.elim(Operand::BB(bb_id));
      }
    }

    let bb = &self.program.as_ref().unwrap().funcs[current_function].cfg[bb_id];
    if !bb.succs.is_empty() {
      for (succ, _) in bb.succs.clone() {
        self.simplify(succ.get_bb_id());
      }
    } else {
      // Check return statement.
      let dfg = &self.program.as_ref().unwrap().funcs[current_function].dfg;
      if bb.cur.is_empty() {
        panic!("SimplifyCFG: The block should not be empty");
      }

      let last = bb.cur.last().unwrap();
      let op = &dfg[*last];
      if !op.is(OpType::Ret) {
        panic!("SimplifyCFG: The last instruction of a block without successor should be a return instruction");
      }

      let func_ret_typ = match &self.program.as_ref().unwrap().funcs[current_function].typ {
        Type::Function { return_type, .. } => (**return_type).clone(),
        _ => panic!("SimplifyCFG: The current function should have a function type"),
      };
      if op.typ != func_ret_typ {
        panic!("SimplifyCFG: The return type of the return instruction should match the function return type");
      }
    }
  }

  pub fn rewrite(&mut self) {
    // Slay the edge of dead block in phi operations.
    let phis = self
      .program
      .as_deref_mut()
      .unwrap()
      .get_all_ops(self.builder.current_function, OpType::Phi);
    for phi_op in &phis {
      let dfg =
        &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].dfg;
      let op = dfg[*phi_op].clone();
      if let OpData::Phi { incomings } = op.data {
        for incoming in incomings.iter() {
          if let PhiIncoming::Data { bb, .. } = incoming {
            if let Operand::BB(bb_id) = bb {
              // Check whether the block is dead or the current block is no longer the successor of the incoming block.
              // If so, we need to slay this incoming edge.
              let current_bb = self.op_to_bb[phi_op.get_op_id()];
              let cfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.unwrap()]
              .cfg;
              let ans_succ = &cfg[*bb_id]
                .succs
                .iter()
                .map(|(succ, _)| *succ)
                .collect::<Vec<Operand>>();

              if !self.visited.contains(*bb_id) || !ans_succ.contains(&current_bb) {
                self.program.as_deref_mut().unwrap().slay_phi_incoming(
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

    let dead_blocks = self.program.as_ref().unwrap().funcs[self.builder.current_function.unwrap()]
      .cfg
      .collect()
      .into_iter()
      .filter(|bb_id| !self.visited.contains(*bb_id))
      .collect::<FxHashSet<usize>>();

    // Phase 1: Isolate the dead blocks, disconnect the edges from live blocks to dead blocks.
    dead_blocks.iter().for_each(|bb_id| {
      let (last, terminator) = {
        let cfg =
          &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].cfg;
        let bb = &cfg[*bb_id];
        let last = match bb.cur.last() {
          Some(last) => *last,
          None => return,
        };
        let data = {
          let dfg =
            &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].dfg;
          dfg[last].data.clone()
        };
        (last, data)
      };
      if matches!(terminator, OpData::Br { .. } | OpData::Jump { .. }) {
        // remove the op
        self.program.as_deref_mut().unwrap().remove_op(
          self.builder.current_function,
          last,
          Some(Operand::BB(*bb_id)),
        );
      }
    });

    // Phase 2: Check users in dead blocks.
    for bb_id in &dead_blocks {
      let cfg =
        &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].cfg;
      let cur = cfg[*bb_id].cur.clone();

      // Split users check and removal due to data dependency.
      for inst in cur.iter().rev() {
        let func_id = self.builder.current_function.unwrap();
        let funcs = &mut self.program.as_mut().unwrap().funcs;
        let dfg = &mut funcs[func_id].dfg;

        // inst can be used by the instructions inside the block, but it cannot be used by instructions outside the block.
        let users = dfg[inst.get_op_id()].users.clone();
        for (user, _) in users {
          let user_bb = self.op_to_bb[user.get_op_id()];
          // The user can be in the same block, or in another dead block. But it cannot be in a live block.
          if dead_blocks.contains(&user_bb.get_bb_id()) {
            // continue. users will be removed later.
            continue;
          }
          panic!(
            "Builder remove_block: instruction {:#?} has user {:#?} outside the block",
            dfg[inst.get_op_id()],
            dfg[user.get_op_id()]
          );
        }

        // Check whether the instruction uses a value outside dead block. If so, remove the use first.
        let data = dfg[*inst].data.clone();
        let op = *inst;
        let is_live_value = |operand: &Operand| match operand {
          Operand::Value(id) => {
            let bb = self.op_to_bb[*id].get_bb_id();
            !dead_blocks.contains(&bb)
          }
          _ => false,
        };

        match_src! {
            target: data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: OpData { lhs, rhs } => {
                if is_live_value(&lhs) {
                    dfg.remove_use(lhs, (op, 0));
                }
                if is_live_value(&rhs) {
                    dfg.remove_use(rhs, (op, 1));
                }
            },
            un_ops: [Sitofp, Fptosi, Zext, Uitofp],
            un_arm: OpData { value } => {
                if is_live_value(&value) {
                    dfg.remove_use(value, (op, 0));
                }
            },
            fallback: {
                OpData::Load { addr } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                    if is_live_value(&addr) {
                        dfg.remove_use(addr, (op, 0));
                    }
                }
                OpData::Store { addr, value } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                    if is_live_value(&addr) {
                        dfg.remove_use(addr, (op, 0));
                    }
                    if is_live_value(&value) {
                        dfg.remove_use(value, (op, 1));
                    }
                }
                OpData::Br { cond, .. } => {
                    if is_live_value(&cond) {
                        dfg.remove_use(cond, (op, 0));
                    }
                }
                OpData::Call { args, .. } => {
                    for (i, arg) in args.iter().enumerate() {
                        if is_live_value(arg) {
                            dfg.remove_use(*arg, (op, i + 1));
                        }
                    }
                }
                OpData::Ret { value } => {
                    if let Some(val) = value {
                        if is_live_value(&val) {
                            dfg.remove_use(val, (op, 0));
                        }
                    }
                }
                OpData::Phi { incomings } => {
                    for (i, phi_incoming) in incomings.iter().enumerate() {
                        if let PhiIncoming::Data { value, .. } = phi_incoming {
                            if is_live_value(value) {
                                dfg.remove_use(*value, (op, i));
                            }
                        }
                    }
                }

                OpData::GEP { base, indices } => {
                    // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                    if is_live_value(&base) {
                        dfg.remove_use(base, (op, 0));
                    }
                    for (i, index) in indices.iter().enumerate() {
                        if is_live_value(index) {
                            dfg.remove_use(*index, (op, i + 1));
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
      let cfg =
        &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].cfg;
      let cur = cfg[*bb_id].cur.clone();
      let dfg =
        &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].dfg;
      for inst in cur.iter().rev() {
        // Remove the uses
        dfg.remove(inst.get_op_id());
      }
    }

    // Phase 4: Remove the blocks directly by cfg.
    for bb_id in dead_blocks {
      // remove the block from cfg
      let cfg =
        &mut self.program.as_mut().unwrap().funcs[self.builder.current_function.unwrap()].cfg;
      cfg.remove(bb_id);
    }
  }
}

impl<'a> Default for SimplifyCFG<'a> {
  fn default() -> Self {
    Self::new()
  }
}

impl<'a> Pass<'a> for SimplifyCFG<'a> {
  fn name(&self) -> &str {
    "SimplifyCFG"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.program = Some(program);
  }
  fn run(&mut self) {
    // We can only simplify CFG at the end of all other optimizations, since it may change the structure of CFG and thus invalidate the assumptions of other optimizations.
    let func_ids = self.program.as_ref().unwrap().funcs.collect_internal();
    for func_id in func_ids {
      self.init(Operand::Func(func_id));
      let entry = match self.program.as_ref().unwrap().funcs[func_id].cfg.entry {
        Some(entry) => entry,
        None => continue,
      };
      self.simplify(entry);
      self.rewrite();
    }
  }
}
