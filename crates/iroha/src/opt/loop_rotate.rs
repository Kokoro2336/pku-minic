//! Loop Rotation.

use crate::analysis::{DomAnalysis, DomFrontier, DomTree, LoopAnalysis};

use yachiyo::base::Type;
use yachiyo::ir::mid::{
  ssa_updater_params, Op, OpData, OpType, Operand, PhiIncoming, SSAUpdater, DFG, IR,
};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::match_some;

use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct LoopRotate<'a> {
  cx: PassContext<'a>,
  /// OpId in the original header -> OpId in the new guard.
  inst_map: FxHashMap<Operand, Operand>,
  /// The moved phis in the headers.
  moved_phis: Vec<Operand>,
  /// The updated phis in the exit blocks.
  updated_phis: Vec<Operand>,
}

impl LoopRotate<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.inst_map.clear();
    self.moved_phis.clear();
    self.updated_phis.clear();
  }

  fn clone_inst(
    &mut self,
    op_id: Operand,
    header_phis: &[Operand],
    pre_header_id: Operand,
    guard_bb_id: Operand,
  ) -> Operand {
    let op = self.cx.get_op(op_id);
    let (mut op_data, typ, attrs) = (op.data.clone(), op.typ.clone(), op.attrs.clone());

    // Update the operands.
    let src = DFG::match_src_mut(&mut op_data);
    for src_op_id in src {
      if !matches!(*src_op_id, Operand::Value(_)) {
        continue;
      }

      if let Some(mapped_op_id) = self.inst_map.get(src_op_id) {
        // If the src_op is defined in the header, we replace it with the new op mapped by inst_map.
        *src_op_id = *mapped_op_id;
      } else if let OpData::Phi { incomings } = self.cx.get_op_data(*src_op_id) {
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
      let mut guard = self.cx.guard();
      guard.set_current_block(guard_bb_id);
      guard.create(Op::new(typ, attrs, op_data))
    };
    self.inst_map.insert(op_id, new_op_id);
    new_op_id
  }

  fn process_header_op(&mut self, op_id: Operand, latch_id: Operand) {
    let src_tuple_mut = self.cx.get_src_tuple_owned(op_id);
    for (src_op_id, idx) in src_tuple_mut {
      if !matches!(src_op_id, Operand::Value(_)) {
        continue;
      }

      if let OpData::Phi { incomings } = self.cx.get_op_data(src_op_id).clone() {
        for incoming in incomings {
          let PhiIncoming::Data { value, bb } = incoming else {
            unreachable!();
          };
          if bb == latch_id {
            self.cx.replace_use((op_id, idx), src_op_id, value);
          }
        }
      }
    }
  }

  fn update_moved_phis(
    &mut self,
    func_id: Operand,
    dom_tree: &DomTree,
    dom_frontier: &DomFrontier,
  ) {
    for phi_id in std::mem::take(&mut self.moved_phis) {
      let body_bb_id = self.cx.op_bb(phi_id);
      let mut available_defs = vec![(body_bb_id, phi_id)];
      let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
        unreachable!()
      };
      for incoming in incomings {
        let PhiIncoming::Data { bb, value } = incoming else {
          unreachable!()
        };
        available_defs.push((bb, value));
      }
      let def_blocks = available_defs.iter().map(|(bb, _)| *bb).collect::<Vec<_>>();
      let (worklist, inserted_blocks, available_defs) =
        ssa_updater_params(def_blocks.clone(), def_blocks, available_defs);

      let mut ssa_updater = SSAUpdater::new(
        self.cx.ir_mut(),
        func_id,
        phi_id,
        dom_tree,
        dom_frontier,
        worklist,
        inserted_blocks,
        available_defs,
      );
      ssa_updater.run();
    }
  }

  fn update_normal_insts(
    &mut self,
    func_id: Operand,
    dom_tree: &DomTree,
    dom_frontier: &DomFrontier,
  ) {
    let updated_phis = std::mem::take(&mut self.updated_phis);
    for &phi_id in &updated_phis {
      let bb_id = self.cx.op_bb(phi_id);
      let mut available_defs = vec![(bb_id, phi_id)];
      let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
        unreachable!()
      };
      for incoming in incomings {
        let PhiIncoming::Data { bb, value } = incoming else {
          unreachable!()
        };
        available_defs.push((bb, value));
      }
      let def_blocks = available_defs.iter().map(|(bb, _)| *bb).collect::<Vec<_>>();
      let (worklist, inserted_blocks, available_defs) =
        ssa_updater_params(def_blocks.clone(), def_blocks, available_defs);

      let mut ssa_updater = SSAUpdater::new(
        self.cx.ir_mut(),
        func_id,
        phi_id,
        dom_tree,
        dom_frontier,
        worklist,
        inserted_blocks,
        available_defs,
      );
      ssa_updater.run();
    }

    let inst_map = std::mem::take(&mut self.inst_map);
    for (orig_id, cloned_id) in inst_map {
      if self.cx.get_op(orig_id).typ == Type::Void {
        continue;
      }

      let mut available_defs = vec![
        (self.cx.op_bb(orig_id), orig_id),
        (self.cx.op_bb(cloned_id), cloned_id),
      ];

      for &phi_id in &updated_phis {
        let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
          unreachable!()
        };
        if incomings.iter().any(|incoming| {
          matches!(
            incoming,
            PhiIncoming::Data { value, .. } if *value == orig_id || *value == cloned_id
          )
        }) {
          available_defs.push((self.cx.op_bb(phi_id), phi_id));
        }
      }

      let def_blocks = available_defs.iter().map(|(bb, _)| *bb).collect::<Vec<_>>();
      let (worklist, inserted_blocks, available_defs) =
        ssa_updater_params(def_blocks.clone(), def_blocks, available_defs);

      let mut ssa_updater = SSAUpdater::new(
        self.cx.ir_mut(),
        func_id,
        orig_id,
        dom_tree,
        dom_frontier,
        worklist,
        inserted_blocks,
        available_defs,
      );
      ssa_updater.run();
    }
  }

  fn run(&mut self, dom_tree: &DomTree) {
    let graph = self.cx.extract_cfg();
    let (loops, _) = &mut *self.cx.analyze_mut::<LoopAnalysis>(&graph);

    for lp_id in (0..loops.len()).rev() {
      let graph = self.cx.extract_cfg();
      let (loops, _) = &mut *self.cx.analyze_mut::<LoopAnalysis>(&graph);

      let loop_data = &mut loops[lp_id.into()];
      let header_id = loop_data.header;

      let (header_preds_len, header_succs) = {
        let header = self.cx.get_bb(header_id);
        (header.preds.len(), header.succs.clone())
      };
      assert!(header_preds_len == 2);

      let pre_header_id = self
        .cx
        .get_pre_header_id(header_id, dom_tree)
        .expect("LoopRotate expects loop-simplified pre-header");
      let latch_id = self
        .cx
        .get_latch_id(header_id, dom_tree)
        .expect("LoopRotate expects loop-simplified latch");
      let Some((exit_bb_id, _)) = header_succs
        .iter()
        .copied()
        .find(|(succ_id, _)| !loop_data.blocks.contains(succ_id.get_bb_id()))
      else {
        // If the header has no exit path, then the loop should not be rotated.
        continue;
      };

      let pre_header_term_id = *self.cx.get_bb(pre_header_id).cur.last().unwrap();
      let pre_header_term_data = self.cx.get_op_data(pre_header_term_id).clone();

      // Create a guard block.
      let guard_bb_id = self.cx.create_new_block();

      // Redirect the pre-header's terminator to the guard block.
      match_some! {
        target: pre_header_term_data,
        enu: OpData,
        minor_arms: {
          OpData::Jump { target_bb } => {
            if target_bb == header_id {
              self.cx.replace_op(pre_header_term_id, Op::new(
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
              self.cx.replace_op(pre_header_term_id, Op::new(
                Type::Void,
                vec![],
                OpData::Br {
                  cond,
                  then_bb: guard_bb_id,
                  else_bb,
                },
              ));
            } else if else_bb == header_id {
              self.cx.replace_op(pre_header_term_id, Op::new(
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

      let phis = self.cx.get_all_ops_in_block(header_id, OpType::Phi);

      for op_id in self.cx.get_bb(header_id).cur.clone() {
        let op_data = self.cx.get_op_data(op_id);
        if op_data.is(OpType::Phi) {
          continue;
        }
        // Clone non-phi instruction in the header to the guard block, including the terminator.
        self.clone_inst(op_id, &phis, pre_header_id, guard_bb_id);
        // Update the use in the original header block.
        self.process_header_op(op_id, latch_id);
      }

      // Update the exit blocks' phi nodes.
      let exit_bb_phis = self.cx.get_all_ops_in_block(exit_bb_id, OpType::Phi);
      for phi_id in exit_bb_phis {
        let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
          unreachable!()
        };

        self.updated_phis.push(phi_id);

        for incoming in incomings {
          if let PhiIncoming::Data { bb, value } = incoming {
            if !matches!(value, Operand::Value(_)) || bb != header_id {
              continue;
            }

            let value_op = self.cx.get_op_data(value);
            let mapped_value = *self.inst_map.get(&value).unwrap();

            if value_op.is(OpType::Phi) {
              // If the value is defined by a phi node in the header, we need to update the incoming block to the guard block.
              self.cx.slay_phi_incoming(phi_id, bb);
              self
                .cx
                .append_phi_incoming(phi_id, guard_bb_id, mapped_value);
              self.cx.append_phi_incoming(phi_id, header_id, value);
            } else {
              // If the value is a normal instruction, simply append a new incoming from the guard block with the mapped value.
              self
                .cx
                .append_phi_incoming(phi_id, guard_bb_id, mapped_value);
            }
          } else {
            unreachable!();
          }
        }
      }

      // Move phi nodes in the header to the loop body.
      let (body_bb_id, _) = self
        .cx
        .get_bb(header_id)
        .succs
        .iter()
        .find(|(succ_id, _)| loop_data.blocks.contains(succ_id.get_bb_id()))
        .copied()
        .unwrap();
      let body_head_op_id = *self.cx.get_bb(body_bb_id).cur.first().unwrap();

      for phi_id in phis {
        self
          .cx
          .move_op_to_bb_at(phi_id, body_bb_id, Some(body_head_op_id));
        self.moved_phis.push(phi_id);
        // Refine incoming block of the moved phi nodes.
        let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
          unreachable!();
        };
        for incoming in incomings {
          let PhiIncoming::Data { bb, value } = incoming else {
            unreachable!();
          };
          if bb == pre_header_id {
            self.cx.slay_phi_incoming(phi_id, bb);
            self.cx.append_phi_incoming(phi_id, guard_bb_id, value);
          } else {
            self.cx.slay_phi_incoming(phi_id, bb);
            self.cx.append_phi_incoming(phi_id, header_id, value);
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
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      {
        let graph = self.cx.extract_cfg();
        let (dom_tree, _) = &*self.cx.analyze::<DomAnalysis>(&graph);
        self.run(dom_tree);
      }

      let graph = self.cx.extract_cfg();
      let (dom_tree, dom_frontier) = &*self.cx.analyze::<DomAnalysis>(&graph);
      self.update_moved_phis(func_id, dom_tree, dom_frontier);
      self.update_normal_insts(func_id, dom_tree, dom_frontier);
    }
  }
}
