//! Loop Simplification (Canonicalization).
//! Ensuring that every loop in the IR only has a single pre-header and a single latch, with dedicated exits.

use crate::analysis::{DomAnalysis, DomTree, LoopAnalysis};
use yachiyo::analysis::LoopData;
use yachiyo::base::Type;
use yachiyo::ir::mid::{Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::match_some;
use yachiyo::utils::BitSet;

#[derive(Default)]
pub struct LoopSimplify<'a> {
  cx: PassContext<'a>,
}

impl LoopSimplify<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn process_preds(&mut self, header_id: Operand, preds: Vec<Operand>) {
    if preds.len() <= 1 {
      // if outer_preds.len() <= 1, then the loop already has a single pre-header/latch, no need to insert one.
      return;
    }

    // If there are multiple outer preds, insert a pre-header.
    let new_bb_id = self.cx.create_new_block();
    self.cx.set_current_block(new_bb_id);
    // Create jump to header_id
    self.cx.create(Op::new(
      Type::Void,
      vec![],
      OpData::Jump {
        target_bb: header_id,
      },
    ));

    // Replace terminator of preds
    for &pred_id in preds.iter() {
      let pred = self.cx.get_bb(pred_id);
      let term_id = *pred.cur.last().unwrap();
      let term_op_data = self.cx.get_op_data(term_id).clone();
      match_some! {
        target: term_op_data,
        enu: OpData,
        minor_arms: {
          OpData::Jump { target_bb } => {
            if target_bb == header_id {
              self.cx.replace_op(term_id, pred_id, Op::new(
                Type::Void,
                vec![],
                OpData::Jump {
                  target_bb: new_bb_id,
                },
              ));
            } else {
              panic!("LoopSimplify: jump terminator does not target the loop header");
            }
          }
          OpData::Br { cond, then_bb, else_bb } => {
            if then_bb == header_id {
              self.cx.replace_op(term_id, pred_id, Op::new(
                Type::Void,
                vec![],
                OpData::Br {
                  cond,
                  then_bb: new_bb_id,
                  else_bb,
                },
              ));
            } else if else_bb == header_id {
              self.cx.replace_op(term_id, pred_id, Op::new(
                Type::Void,
                vec![],
                OpData::Br {
                  cond,
                  then_bb,
                  else_bb: new_bb_id,
                },
              ));
            } else {
              panic!("LoopSimplify: branch terminator does not target the loop header");
            }
          }
        },
        uni_ops: [GlobalAlloca, Alloca, Load, Store, Call, Ret, AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, Phi, GEP, SEq, SLt, SLe, SNe, SGe, SGt, OEq, OLt, OLe, ONe, OGe, OGt, Declare, Xor, Shl, Sar, Shr, Sitofp, Fptosi, Uitofp, Zext],
        uni_arm: {
          unreachable!("Unexpected terminator op: {:?}", term_op_data);
        }
      }
    }

    // Process phi nodes in the header block.
    let phis = self.cx.get_all_ops_in_block(header_id, OpType::Phi);
    for phi_id in phis {
      let phi_op_data = self.cx.get_op_data(phi_id).clone();
      let phi_typ = self.cx.get_op_type(phi_id);
      if let OpData::Phi { incomings } = phi_op_data {
        let pred_incomings = incomings
          .iter()
          .filter(|incoming| {
            if let PhiIncoming::Data { value: _, bb } = incoming {
              preds.contains(bb)
            } else {
              false
            }
          })
          .cloned()
          .collect::<Vec<_>>();

        // Create phi in pre-header
        let new_phi_id =
          self
            .cx
            .create_at_head(Op::new(phi_typ, vec![], OpData::Phi { incomings: vec![] }));

        // Add edges
        for pred_incoming in pred_incomings.iter() {
          if let PhiIncoming::Data { value, bb } = pred_incoming {
            self.cx.append_phi_incoming(new_phi_id, *bb, *value);
          } else {
            unreachable!()
          }
        }

        // Update incoming of the original phi
        for incoming in pred_incomings {
          if let PhiIncoming::Data { value: _, bb } = incoming {
            self.cx.slay_phi_incoming(phi_id, bb);
          } else {
            unreachable!()
          }
        }
        self.cx.append_phi_incoming(phi_id, new_bb_id, new_phi_id);
      } else {
        unreachable!()
      }
    }
  }

  /// For simplicity, we insert one dedicated exiting block.
  fn dedicated_exits(&mut self, loop_data: &mut LoopData) {
    let mut deprecated_exits = BitSet::new();
    let mut new_exits = BitSet::new();

    // Find out all exit blocks of the loop, and for each exit block.
    for exit_bb_id in loop_data.exit_blocks.iter() {
      let exit_bb_id = Operand::BB(exit_bb_id);
      let exit_bb_preds = self.cx.get_bb(exit_bb_id).preds.clone();
      let should_insert_dedicated = exit_bb_preds
        .iter()
        .any(|(pred_id, _)| !loop_data.blocks.contains(pred_id.get_bb_id()));
      if !should_insert_dedicated {
        continue;
      }

      // Set as deprecated.
      deprecated_exits.insert(exit_bb_id.get_bb_id());

      for (exit_bb_pred_id, _) in exit_bb_preds.iter() {
        if loop_data.blocks.contains(exit_bb_pred_id.get_bb_id()) {
          // If the exit block has preds from the loop, insert a dedicated exit block.
          let new_exit_bb_id = self.cx.create_new_block();
          self.cx.set_current_block(new_exit_bb_id);

          // Mark the new exit as the new exit block.
          new_exits.insert(new_exit_bb_id.get_bb_id());

          // Create jump to exit_bb_id
          self.cx.create(Op::new(
            Type::Void,
            vec![],
            OpData::Jump {
              target_bb: exit_bb_id,
            },
          ));

          // Replace terminator of exit_bb_pred_id
          let pred = self.cx.get_bb(*exit_bb_pred_id);
          let term_id = *pred.cur.last().unwrap();
          let term_op_data = self.cx.get_op_data(term_id).clone();
          match_some! {
            target: term_op_data,
            enu: OpData,
            minor_arms: {
              OpData::Jump { target_bb } => {
                if target_bb == exit_bb_id {
                  self.cx.replace_op(term_id, *exit_bb_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Jump {
                      target_bb: new_exit_bb_id,
                    },
                  ));
                } else {
                  panic!("LoopSimplify: jump terminator does not target the loop exit");
                }
              }
              OpData::Br { cond, then_bb, else_bb } => {
                if then_bb == exit_bb_id {
                  self.cx.replace_op(term_id, *exit_bb_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Br {
                      cond,
                      then_bb: new_exit_bb_id,
                      else_bb,
                    },
                  ));
                } else if else_bb == exit_bb_id {
                  self.cx.replace_op(term_id, *exit_bb_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Br {
                      cond,
                      then_bb,
                      else_bb: new_exit_bb_id,
                    },
                  ));
                } else {
                  panic!("LoopSimplify: branch terminator does not target the loop exit");
                }
              }
            },
            uni_ops: [GlobalAlloca, Alloca, Load, Store, Call, Ret, AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, Phi, GEP, Declare, Xor, Shl, Sar, Shr, Sitofp, Fptosi, Uitofp, Zext, SNe, SEq, SLe, SLt, SGe, SGt, OEq, OLt, OLe, ONe, OGe, OGt],
            uni_arm: {
              unreachable!("Unexpected terminator op: {:?}", term_op_data);
            }
          }

          // Process phi nodes in the exit block.
          let phis = self.cx.get_all_ops_in_block(exit_bb_id, OpType::Phi);
          for phi_id in phis {
            let phi_op_data = self.cx.get_op_data(phi_id).clone();
            if let OpData::Phi { incomings } = phi_op_data {
              for incoming in incomings {
                if let PhiIncoming::Data { value, bb } = incoming {
                  if bb == *exit_bb_pred_id {
                    self.cx.slay_phi_incoming(phi_id, bb);
                    self.cx.append_phi_incoming(phi_id, new_exit_bb_id, value);
                  }
                } else {
                  unreachable!()
                }
              }
            } else {
              unreachable!()
            }
          }
        }
      }
    }

    // Update exit_blocks.
    loop_data.exit_blocks &= !deprecated_exits;
    loop_data.exit_blocks |= new_exits;
  }

  fn run(&mut self, dom_tree: &DomTree, loops_data: &mut Vec<LoopData>) {
    for loop_data in loops_data {
      // Ensure dedicated exits first.
      self.dedicated_exits(loop_data);

      // Processing headers and latches.
      let header_id = loop_data.header;
      let header = self.cx.get_bb(header_id);
      let (mut pre_header_preds, mut latch_preds) = (vec![], vec![]);
      for (pred_id, _) in header.preds.iter() {
        if dom_tree.is_dom(header_id.get_bb_id(), pred_id.get_bb_id()) {
          latch_preds.push(*pred_id);
        } else {
          pre_header_preds.push(*pred_id);
        }
      }
      self.process_preds(header_id, pre_header_preds);
      self.process_preds(header_id, latch_preds);
    }
  }
}

impl<'a> Pass<'a> for LoopSimplify<'a> {
  fn name(&self) -> &'static str {
    "LoopSimplify"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      let func = self.cx.get_func(func_id);
      let (loops_data, _) = &mut *self.cx.analyze_mut::<LoopAnalysis>(func);
      let (dom_tree, _) = &*self.cx.analyze::<DomAnalysis>(func);
      self.run(dom_tree, loops_data);
    }
  }
}
