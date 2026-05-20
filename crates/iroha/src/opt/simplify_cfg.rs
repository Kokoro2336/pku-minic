//! Simplify CFG.

use yachiyo::ir::mid::{OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::match_some;
use yachiyo::utils::Arena;
use yachiyo::utils::BitSet;

use crate::analysis::Reachability;

#[derive(Default)]
pub struct SimplifyCFG<'a> {
  cx: PassContext<'a>,
  /// Processed dead blocks
  processed: BitSet,
}

impl SimplifyCFG<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.processed.clear();
  }

  /// Cut off data flow and control flow edges of dead blocks.
  fn isolate_dead(&mut self, bb_id: Operand) {
    let func_id = self.cx.current_func();
    let cur = self.cx.get_bb(bb_id).cur.clone();
    let succs = self.cx.get_bb(bb_id).succs.clone();
    self.cx.clear_uses();
    let blocks = self
      .cx
      .get_func(func_id)
      .cfg
      .collect()
      .into_iter()
      .map(|bb| self.cx.get_bb(Operand::BB(bb)).cur.clone())
      .collect::<Vec<_>>();
    for ops in blocks {
      for op_id in ops {
        self.cx.add_uses(op_id);
      }
    }

    for inst in cur.iter().rev() {
      let src_tuple = self
        .cx
        .get_src_tuple(*inst)
        .iter()
        .map(|(src, idx)| (**src, *idx))
        .collect::<Vec<_>>();
      for (src, idx) in src_tuple {
        self.cx.remove_use(src, (*inst, idx));
      }
      let users = self.cx.users(*inst).to_vec();
      for (user, _) in users {
        let op_data = self.cx.get_op_data(user).clone();
        if let OpData::Phi { incomings } = op_data {
          for incoming in incomings {
            if let PhiIncoming::Data {
              value,
              bb: incoming_bb,
            } = incoming
            {
              if value == *inst {
                self.cx.slay_phi_incoming(user, incoming_bb);
              }
            }
          }
        }
      }
      self.cx.replace_all_uses(*inst, Operand::Undefined);
    }

    for (succ_id, _) in succs {
      let succ_phis = self.cx.get_all_ops_in_block(succ_id, OpType::Phi);
      for phi_id in succ_phis {
        loop {
          let op_data = self.cx.get_op_data(phi_id).clone();
          let has_incoming = if let OpData::Phi { incomings } = op_data {
            incomings
              .iter()
              .any(|incoming| matches!(incoming, PhiIncoming::Data { bb, .. } if *bb == bb_id))
          } else {
            false
          };
          if !has_incoming {
            break;
          }
          self.cx.slay_phi_incoming(phi_id, bb_id);
        }
      }
    }

    if let Some(last) = cur.last() {
      self.cx.remove_control_flow(*last, bb_id);
    }

    self.processed.insert(bb_id.get_bb_id());
  }

  fn process_moved_instructions(
    &mut self,
    op_id: Operand,
    from_bb: Operand,
    to_bb: Operand,
    before_op: Option<Operand>,
  ) {
    self.cx.move_op_to_bb_at(op_id, from_bb, to_bb, before_op);
    // If a user of the moved instruction is a phi node in the original block, we need to update the phi node to point to the new block.
    let users = self.cx.users(op_id).to_vec();
    for (user, _) in users {
      let user_op_data = self.cx.get_op_data(user).clone();
      if let OpData::Phi { incomings } = user_op_data {
        for incoming in incomings {
          if let PhiIncoming::Data { bb, .. } = incoming {
            if bb == from_bb {
              self.cx.slay_phi_incoming(user, from_bb);
              self.cx.append_phi_incoming(user, to_bb, op_id);
            }
          }
        }
      }
    }
  }

  pub fn simplify(&mut self, bb_id: Operand) {
    let func_id = self.cx.current_func();
    let mut is_dead = false;

    // Case 1: 1 pred and the pred has only 1 succ. Then merge current block into its predecessor.
    let bb = self.cx.get_bb(bb_id);
    if bb.preds.len() == 1 && {
      let pred_id = bb.preds[0].0;
      let pred = self.cx.get_bb(pred_id);
      pred.succs.len() == 1 && pred_id != bb_id
    } {
      is_dead = true;

      let bb = self.cx.get_bb(bb_id);
      let (pred_id, cur) = (bb.preds[0].0, bb.cur.clone());
      let pred = self.cx.get_bb(pred_id);
      let pred_term_id = match pred.cur.last() {
        Some(id) => *id,
        None => unreachable!(),
      };

      // It's impossible that such block contains any phi nodes.
      for inst in cur.iter().take(cur.len() - 1) {
        self.process_moved_instructions(*inst, bb_id, pred_id, Some(pred_term_id));
      }

      // Replace the terminator of the predecessor with the terminator of the current block.
      let bb = self.cx.get_bb(bb_id);
      let bb_term_id = match bb.cur.last() {
        Some(id) => *id,
        None => unreachable!(),
      };
      let bb_term_op = self.cx.get_op(bb_term_id).clone();
      self.cx.replace_op(pred_term_id, pred_id, bb_term_op);

      // Update downstream's phi nodes
      let bb = self.cx.get_bb(bb_id);
      for (succ_id, _) in bb.succs.clone() {
        let succ_phis = self.cx.get_all_ops_in_block(succ_id, OpType::Phi);
        for phi_id in succ_phis {
          let phi_op_data = self.cx.get_op_data(phi_id).clone();
          if let OpData::Phi { incomings } = phi_op_data {
            for incoming in incomings {
              if let PhiIncoming::Data { bb, value } = incoming {
                if bb != bb_id {
                  continue;
                }
                self.cx.slay_phi_incoming(phi_id, bb_id);
                self.cx.append_phi_incoming(phi_id, pred_id, value);
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
        let pred = self.cx.get_bb(*pred_id);
        pred.succs.len() == 1 && *pred_id != bb_id
      })
      && bb.cur.iter().all(|inst_id| {
        let succs = &bb.succs;
        let op = self.cx.get_op(*inst_id);
        let op_data = &op.data;
        let users = self.cx.users(*inst_id);
        if op_data.is_terminator() {
          true
        } else if let OpData::Phi { .. } = op_data {
          users.iter().all(|(user, _)| {
            let bb_id = self.cx.op_bb(*user);
            succs[0].0 == bb_id && self.cx.get_op_data(*user).is(OpType::Phi)
          })
        } else {
          false
        }
      })
      && bb.succs[0].0 != bb_id
      // Ignore entry block.
      && bb_id != Operand::BB(self.cx.get_func(func_id).cfg.entry.unwrap())
    {
      is_dead = true;

      // Update the terminators of preds.
      let bb = self.cx.get_bb(bb_id);
      let succ_id = bb.succs[0].0;
      let preds = bb.preds.clone();

      for (pred_id, _) in preds.iter() {
        let pred = self.cx.get_bb(*pred_id);
        let pred_term = match pred.cur.last() {
          Some(id) => *id,
          None => unreachable!(),
        };
        let mut pred_term_op = self.cx.get_op(pred_term).clone();

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
        self.cx.replace_op(pred_term, *pred_id, pred_term_op);
      }

      // Update the phi nodes in successor block.
      let succ_phis = self.cx.get_all_ops_in_block(succ_id, OpType::Phi);
      for phi_id in succ_phis {
        if let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() {
          for incoming in incomings {
            if let PhiIncoming::Data {
              bb: incoming_bb,
              value,
            } = incoming
            {
              if incoming_bb != bb_id {
                continue;
              }
              self.cx.slay_phi_incoming(phi_id, bb_id);
              let new_incomings =
                if matches!(value, Operand::Value(_)) && self.cx.op_bb(value) == bb_id {
                  if let OpData::Phi {
                    incomings: tramp_incomings,
                  } = self.cx.get_op_data(value).clone()
                  {
                    tramp_incomings
                  } else {
                    unreachable!()
                  }
                } else {
                  preds
                    .iter()
                    .map(|(pred_id, _)| PhiIncoming::Data {
                      bb: *pred_id,
                      value,
                    })
                    .collect::<Vec<PhiIncoming>>()
                };
              for incoming in new_incomings {
                if let PhiIncoming::Data {
                  bb: incoming_bb,
                  value: incoming_value,
                } = incoming
                {
                  self
                    .cx
                    .append_phi_incoming(phi_id, incoming_bb, incoming_value);
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
    let func_id = self.cx.current_func();
    let visited = &*self.cx.analyze::<Reachability>(self.cx.get_func(func_id));
    let dead_blocks = self
      .cx
      .get_func(func_id)
      .cfg
      .collect()
      .into_iter()
      .filter(|bb_id| !visited.contains(*bb_id))
      .map(Operand::BB)
      .collect::<Vec<Operand>>();

    for bb_id in dead_blocks.iter() {
      if self.processed.contains(bb_id.get_bb_id()) {
        continue;
      }
      self.isolate_dead(*bb_id);
    }

    for bb_id in dead_blocks.iter() {
      let cur = self.cx.get_bb(*bb_id).cur.clone();
      let func = self.cx.get_func_mut(func_id);
      for inst in cur.iter().rev() {
        func.op_to_bb[*inst] = Operand::default();
        func.dfg.remove(inst.get_op_id());
      }
    }

    for bb_id in dead_blocks.iter() {
      self.cx.get_func_mut(func_id).cfg.remove(bb_id.get_bb_id());
    }

    self.cx.clear_uses();
    let blocks = self
      .cx
      .get_func(func_id)
      .cfg
      .collect()
      .into_iter()
      .map(|bb| self.cx.get_bb(Operand::BB(bb)).cur.clone())
      .collect::<Vec<_>>();
    for ops in blocks {
      for op_id in ops {
        self.cx.add_uses(op_id);
      }
    }
  }
}

impl<'a> Pass<'a> for SimplifyCFG<'a> {
  fn name(&self) -> &str {
    "SimplifyCFG"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.cx.mount(program);
  }
  fn run(&mut self) {
    // We can only simplify CFG at the end of all other optimizations, since it may change the structure of CFG and thus invalidate the assumptions of other optimizations.
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);

      // TODO: fixed point iteration.
      let dfs = self.cx.get_func(func_id).cfg.dpo();
      // Reverse post order
      for bb_id in dfs.into_iter().rev() {
        self.simplify(bb_id);
      }
      self.rewrite();
    }
  }
}
