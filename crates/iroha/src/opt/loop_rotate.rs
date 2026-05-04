//! Loop Rotation.

use crate::analysis::{DomAnalysis, DomTree, LoopAnalysis, LoopData};

use yachiyo::analysis::analyze;
use yachiyo::base::Type;
use yachiyo::ir::mid::{
  Builder, BuilderGuard, Function, Op, OpData, OpType, Operand, PhiIncoming, DFG, IR,
};
use yachiyo::pass::Pass;
use yachiyo::utils::r#match::match_some;

use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct LoopRotate<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  /// OpId in the original header -> OpId in the new guard.
  inst_map: FxHashMap<Operand, Operand>,
}

impl LoopRotate<'_> {
  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_func_mut(&mut self, func_id: Operand) -> &mut Function {
    &mut self.ir.as_mut().unwrap().funcs[func_id]
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
  fn replace_op(&mut self, old_op_id: Operand, bb_id: Operand, new_op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_op(&mut self.builder, func_id, old_op_id, bb_id, new_op)
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
  fn get_src_tuple(&mut self, op_id: Operand) -> Vec<(Operand, usize)> {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .get_src_tuple(func_id, op_id)
      .iter()
      .map(|(src_op_id, idx)| (**src_op_id, *idx))
      .collect()
  }

  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
    self.inst_map.clear();
  }

  fn clone_inst(
    &mut self,
    op_id: Operand,
    header_phis: &[Operand],
    pre_header_id: Operand,
    guard_bb_id: Operand,
  ) -> Operand {
    let func_id = self.builder.current_function;
    let mut op = self.get_func(func_id.unwrap()).dfg[op_id].clone();

    // Update the operands.
    let src = DFG::match_src_mut(&mut op.data);
    for src_op_id in src {
      if !matches!(*src_op_id, Operand::Value(_)) {
        continue;
      }

      if let Some(mapped_op_id) = self.inst_map.get(src_op_id) {
        // If the src_op is defined in the header, we replace it with the new op mapped by inst_map.
        *src_op_id = *mapped_op_id;
      } else if let OpData::Phi { incomings } =
        &self.get_func(func_id.unwrap()).dfg[*src_op_id].data
      {
        if !header_phis.contains(src_op_id) {
          // If the src_op is defined by a phi node but not in the header, it must be defined outside the loop. Keep it as is.
          continue;
        }
        // Else if the src_op is a phi node defined in the header, replace it with the initial value comes from the pre-header.
        let init_val = incomings
          .iter()
          .find_map(|incoming| {
            let PhiIncoming::Data { bb, value } = incoming else {
              return None;
            };
            if *bb == pre_header_id {
              Some(value)
            } else {
              None
            }
          })
          .unwrap();

        *src_op_id = *init_val;
      }
    }

    let new_op_id = {
      let mut guard = BuilderGuard::new(&mut self.builder);
      guard.set_current_block(guard_bb_id);
      guard.create(self.ir.as_mut().unwrap(), func_id, op)
    };
    self.inst_map.insert(op_id, new_op_id);
    new_op_id
  }

  fn process_header_op(&mut self, op_id: Operand, latch_id: Operand) {
    let func_id = self.builder.current_function.unwrap();
    let src_tuple_mut = self.get_src_tuple(op_id);
    for (src_op_id, idx) in src_tuple_mut {
      if !matches!(src_op_id, Operand::Value(_)) {
        continue;
      }

      if let OpData::Phi { incomings } = self.get_func(func_id).dfg[src_op_id].data.clone() {
        for incoming in incomings {
          let PhiIncoming::Data { value, bb } = incoming else {
            unreachable!();
          };
          if bb == latch_id {
            let dfg = &mut self.get_func_mut(func_id).dfg;
            dfg.replace_use((op_id, idx), src_op_id, value);
          }
        }
      }
    }
  }

  fn run(&mut self, dom_tree: &DomTree, loops: &mut [LoopData]) {
    let func_id = self.builder.current_function.unwrap();

    for lp_id in (0..loops.len()).rev() {
      let loop_data = &mut loops[lp_id];
      let header_id = loop_data.header;

      let cfg = &self.get_func(func_id).cfg;
      let header = &cfg[header_id];
      let (header_preds, header_succs) = (&header.preds, &header.succs);
      assert!(header_preds.len() == 2);

      let (mut pre_header_id, mut latch_id) = (None, None);
      for (header_pred_id, _) in header_preds.iter() {
        if dom_tree.is_dom(header_id.get_bb_id(), header_pred_id.get_bb_id()) {
          latch_id = Some(*header_pred_id);
        } else {
          pre_header_id = Some(*header_pred_id);
        }
      }
      let (pre_header_id, latch_id) = (pre_header_id.unwrap(), latch_id.unwrap());
      let Some(&(exit_bb_id, _)) = header_succs
        .iter()
        .find(|(succ_id, _)| !loop_data.blocks.contains(succ_id.get_bb_id()))
      else {
        // If the header has no exit path, then the loop should not be rotated.
        continue;
      };

      let pre_header_term_id = *cfg[pre_header_id].cur.last().unwrap();
      let pre_header_term_data = self.get_func(func_id).dfg[pre_header_term_id].data.clone();

      // Create a guard block.
      let guard_bb_id = self
        .builder
        .create_new_block(self.ir.as_mut().unwrap(), self.builder.current_function);

      // Redirect the pre-header's terminator to the guard block.
      match_some! {
        target: pre_header_term_data,
        enu: OpData,
        minor_arms: {
          OpData::Jump { target_bb } => {
            if target_bb == header_id {
              self.replace_op(pre_header_term_id, pre_header_id, Op::new(
                Type::Void,
                vec![],
                OpData::Jump {
                  target_bb: guard_bb_id,
                },
              ));
            } else {
              panic!("LoopSimplify: jump terminator does not target the loop exit");
            }
          }
          OpData::Br { cond, then_bb, else_bb } => {
            if then_bb == header_id {
              self.replace_op(pre_header_term_id, pre_header_id, Op::new(
                Type::Void,
                vec![],
                OpData::Br {
                  cond,
                  then_bb: guard_bb_id,
                  else_bb,
                },
              ));
            } else if else_bb == header_id {
              self.replace_op(pre_header_term_id, pre_header_id, Op::new(
                Type::Void,
                vec![],
                OpData::Br {
                  cond,
                  then_bb,
                  else_bb: guard_bb_id,
                },
              ));
            } else {
              panic!("LoopSimplify: branch terminator does not target the loop exit");
            }
          }
        },
        uni_ops: [GlobalAlloca, Alloca, Load, Store, Call, Ret, AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, Phi, GEP, Declare, Xor, Shl, Sar, Shr, Sitofp, Fptosi, Uitofp, Zext, SNe, SEq, SLe, SLt, SGe, SGt, OEq, OLt, OLe, ONe, OGe, OGt],
        uni_arm: {
          unreachable!("Unexpected terminator op: {:?}", pre_header_term_data);
        }
      }

      let phis = self.get_all_ops_in_block(header_id, OpType::Phi);

      for op_id in self.get_func(func_id).cfg[header_id].cur.clone() {
        let op_data = &self.get_func(func_id).dfg[op_id].data;
        if op_data.is(OpType::Phi) {
          continue;
        }
        // Clone non-phi instruction in the header to the guard block, including the terminator.
        self.clone_inst(op_id, &phis, pre_header_id, guard_bb_id);
        // Update the use in the original header block.
        self.process_header_op(op_id, latch_id);
      }

      // Update the exit blocks' phi nodes.
      let exit_bb_phis = self.get_all_ops_in_block(exit_bb_id, OpType::Phi);
      for phi_id in exit_bb_phis {
        let OpData::Phi { incomings } = self.get_func(func_id).dfg[phi_id].data.clone() else {
          unreachable!()
        };
        for incoming in incomings {
          if let PhiIncoming::Data { bb, value } = incoming {
            if !matches!(value, Operand::Value(_)) || bb != header_id {
              continue;
            }

            let value_op = &self.get_func(func_id).dfg[value].data;
            let mapped_value = *self.inst_map.get(&value).unwrap();

            if value_op.is(OpType::Phi) {
              // If the value is defined by a phi node in the header, we need to update the incoming block to the guard block.
              self.slay_phi_incoming(phi_id, bb);
              self.append_phi_incoming(phi_id, guard_bb_id, mapped_value);
              self.append_phi_incoming(phi_id, header_id, value);
            } else {
              // If the value is a normal instruction, simply append a new incoming from the guard block with the mapped value.
              self.append_phi_incoming(phi_id, guard_bb_id, mapped_value);
            }
          } else {
            unreachable!();
          }
        }
      }

      // Move phi nodes in the header to the loop body.
      let bb = &self.get_func(func_id).cfg[header_id];
      let (body_bb_id, _) = *bb
        .succs
        .iter()
        .find(|(succ_id, _)| loop_data.blocks.contains(succ_id.get_bb_id()))
        .unwrap();
      let body_bb = &self.get_func(func_id).cfg[body_bb_id];
      let body_head_op_id = *body_bb.cur.first().unwrap();

      for phi_id in phis {
        self.move_op_to_bb_at(phi_id, header_id, body_bb_id, Some(body_head_op_id));
        // Refine incoming block of the moved phi nodes.
        let OpData::Phi { incomings } = self.get_func(func_id).dfg[phi_id].data.clone() else {
          unreachable!();
        };
        for incoming in incomings {
          let PhiIncoming::Data { bb, value } = incoming else {
            unreachable!();
          };
          if bb == pre_header_id {
            self.slay_phi_incoming(phi_id, bb);
            self.append_phi_incoming(phi_id, guard_bb_id, value);
          } else {
            self.slay_phi_incoming(phi_id, bb);
            self.append_phi_incoming(phi_id, header_id, value);
          }
        }
      }
    }
  }
}

impl<'a> Pass<'a> for LoopRotate<'a> {
  fn name(&self) -> &'static str {
    "LoopRotate"
  }

  fn mount(&mut self, ir: &'a mut IR) {
    self.ir = Some(ir);
  }

  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      let func = self.get_func(func_id);
      let (mut loops, _) = analyze::<LoopAnalysis>(func);
      let (dom_tree, _) = analyze::<DomAnalysis>(func);
      self.run(&dom_tree, &mut loops);
    }
  }
}
