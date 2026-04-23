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
      .append_phi_incoming(func_id, phi_id, bb_id, value);
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
  fn remove_op(&mut self, op_id: Operand, bb_id: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .remove_op(func_id, op_id, Some(bb_id));
  }

  /// Cut off data flow and control flow edges of dead blocks.
  fn process_dead(&mut self, bb_id: Operand) {
    let func_id = self.builder.current_function.unwrap();
    let cur = self.get_func(func_id).cfg[bb_id].cur.clone();

    // Remove the uses of normal instructions
    for inst in cur.iter().rev().skip(1) {
      let src_tuple = self
        .get_src_tuple(*inst)
        .iter()
        .map(|(src, idx)| (**src, *idx))
        .collect::<Vec<_>>();
      let dfg = &mut self.get_func_mut(func_id).dfg;
      for (src, idx) in src_tuple {
        dfg.remove_use(src, (*inst, idx));
      }
    }
    // Remove terminator directly.
    if let Some(last) = cur.last() {
      let dfg = &mut self.get_func_mut(func_id).dfg;
      let last_op_data = &dfg[*last].data;
      assert!(last_op_data.is_terminator());
      self.remove_op(*last, bb_id);
    }
    // Update the block as processed dead.
    self.processed.insert(bb_id.get_bb_id());
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

      for inst in cur.iter().rev().skip(1) {
        self.move_op_to_bb_at(*inst, bb_id, pred_id, Some(pred_term_id));
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
          let phi_op = &mut self.get_func_mut(func_id).dfg[phi_id];
          if let OpData::Phi { incomings } = &mut phi_op.data {
            for incoming in incomings.iter_mut() {
              if let PhiIncoming::Data { bb, .. } = incoming {
                if *bb == bb_id {
                  *bb = pred_id;
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
      && bb.cur.iter().all(|inst_id| {
        let dfg = &self.get_func(func_id).dfg;
        let op_data = &dfg[*inst_id].data;
        op_data.is_terminator() || op_data.is(OpType::Phi)
      })
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
              let new_incomings = if self.op_to_bb[value.get_op_id()] == bb_id {
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
      self.process_dead(bb_id);
    }
  }

  pub fn rewrite(&mut self) {
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
      self.process_dead(*bb_id);
    }

    // Remove the instructions in dead blocks directly by dfg.
    for bb_id in dead_blocks.iter() {
      let cur = self.get_func(func_id).cfg[*bb_id].cur.clone();
      let dfg = &mut self.get_func_mut(func_id).dfg;
      for inst in cur.iter().rev() {
        // Remove the uses
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
