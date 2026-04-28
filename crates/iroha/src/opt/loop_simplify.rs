//! Loop Simplification (Canonicalization).
//! This pass only ensure a single pre-header but no single latch.

use crate::analysis::{DomAnalysis, DomTree, LoopAnalysis, LoopData};
use yachiyo::analysis::analyze;
use yachiyo::base::Type;
use yachiyo::ir::mid::{Builder, Function, Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::r#match::match_some;

#[derive(Default)]
pub struct LoopSimplify<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
}

impl LoopSimplify<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
  }

  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
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
  fn create(&mut self, op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self.ir.as_mut().unwrap().create(&self.builder, func_id, op)
  }

  #[inline(always)]
  fn create_at_head(&mut self, op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .create_at_head(&mut self.builder, func_id, op)
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

  fn run(&mut self, dom_tree: &DomTree, loops_data: &Vec<LoopData>) {
    let func_id = self.builder.current_function.unwrap();
    for loop_data in loops_data {
      let header_id = loop_data.header;
      let header = &self.get_func(func_id).cfg[header_id];
      let outer_preds = header
        .preds
        .clone()
        .into_iter()
        .filter(|(pred_id, _)| !dom_tree.is_dom(header_id.get_bb_id(), pred_id.get_bb_id()))
        .map(|(pred_id, _)| pred_id)
        .collect::<Vec<_>>();

      if outer_preds.len() > 1 {
        // If there are multiple outer preds, insert a pre-header.
        let pre_header_id = self
          .builder
          .create_new_block(self.ir.as_mut().unwrap(), Some(func_id));
        self.builder.set_current_block(pre_header_id);
        // Create jump to header_id
        self.create(Op::new(
          Type::Void,
          vec![],
          OpData::Jump {
            target_bb: header_id,
          },
        ));

        // Replace terminator of outer preds
        for &outer_pred_id in outer_preds.iter() {
          let outer_pred = &self.get_func(func_id).cfg[outer_pred_id];
          let term_id = *outer_pred.cur.last().unwrap();
          let term_op_data = self.get_func(func_id).dfg[term_id].data.clone();
          match_some! {
            target: term_op_data,
            enu: OpData,
            minor_arms: {
              OpData::Jump { target_bb } => {
                if target_bb == header_id {
                  self.replace_op(term_id, outer_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Jump {
                      target_bb: pre_header_id,
                    },
                  ));
                } else {
                  panic!("LoopSimplify: jump terminator does not target the loop header");
                }
              }
              OpData::Br { cond, then_bb, else_bb } => {
                if then_bb == header_id {
                  self.replace_op(term_id, outer_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Br {
                      cond,
                      then_bb: pre_header_id,
                      else_bb,
                    },
                  ));
                } else if else_bb == header_id {
                  self.replace_op(term_id, outer_pred_id, Op::new(
                    Type::Void,
                    vec![],
                    OpData::Br {
                      cond,
                      then_bb,
                      else_bb: pre_header_id,
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
        let phis = self.get_all_ops_in_block(header_id, OpType::Phi);
        for phi_id in phis {
          let phi_op_data = self.get_func(func_id).dfg[phi_id].data.clone();
          if let OpData::Phi { incomings } = phi_op_data {
            let pre_header_incomings = incomings
              .iter()
              .filter(|incoming| {
                if let PhiIncoming::Data { value: _, bb } = incoming {
                  outer_preds.contains(bb)
                } else {
                  false
                }
              })
              .cloned()
              .collect::<Vec<_>>();

            // Create phi in pre-header
            let new_phi_id = self.create_at_head(Op::new(
              Type::Void,
              vec![],
              OpData::Phi {
                incomings: pre_header_incomings.clone(),
              },
            ));

            // Update incoming of the original phi
            for incoming in pre_header_incomings {
              if let PhiIncoming::Data { value: _, bb } = incoming {
                self.slay_phi_incoming(phi_id, bb);
              } else {
                unreachable!()
              }
            }
            self.append_phi_incoming(phi_id, pre_header_id, new_phi_id);
          } else {
            unreachable!()
          }
        }
      }
      // if outer_preds.len() <= 1, then the loop already has a single pre-header, no need to insert one.
    }
  }
}

impl<'a> Pass<'a> for LoopSimplify<'a> {
  fn name(&self) -> &'static str {
    "LoopSimplify"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.ir = Some(ir);
  }
  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      let func = self.get_func(func_id);
      let loops_data = analyze::<LoopAnalysis>(func);
      let (dom_tree, _) = analyze::<DomAnalysis>(func);
      self.run(&dom_tree, &loops_data);
    }
  }
}
