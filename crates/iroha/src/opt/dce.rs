//! Dead Code Elimination (DCE).

use yachiyo::analysis::{analyze, Pureness, PurenessResult};
use yachiyo::ir::mid::{OpData, OpType, Operand, PhiIncoming};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::r#match::match_src;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::Worklist;

use crate::analysis::{CallGraphAnalysis, PurenessAnalysis, SCCAnalysis};

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct DCE<'a> {
  pub cx: PassContext<'a>,
  // Worklist of inst
  worklist: Worklist<Operand, BitSet>,
  pureness: PurenessResult,
}

impl DCE<'_> {
  pub fn is_dead(&self, operand: &Operand) -> bool {
    match operand {
      Operand::Value(id) => self.cx.users(Operand::Value(*id)).is_empty(),
      Operand::Global(id) => !self.cx.ir().has_users(None, Operand::Global(*id)),
      _ => panic!("DCE: operand is not a value"),
    }
  }

  pub fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.worklist.clear();

    // Initialize the worklist
    let block_ids = self.cx.get_func(func_id).cfg.collect();
    for block_id in block_ids {
      let block = self.cx.get_bb(Operand::BB(block_id));
      for inst_id in block.cur.iter() {
        let is_impure = self.is_impure(*inst_id);
        if self.is_dead(inst_id) && !is_impure {
          self.worklist.push_back(*inst_id);
        }
      }
    }
  }
}

impl DCE<'_> {
  fn is_impure(&self, operand: Operand) -> bool {
    let op_data = self.cx.get_op(operand).data.clone();
    if let OpData::Call { func, .. } = op_data {
      return self.pureness[func] == Pureness::Impure;
    }
    let op_typ = OpType::from(&op_data);
    matches!(
      op_typ,
      // In DCE, Load is pure.
      OpType::Store
        | OpType::Br
        | OpType::Jump
        | OpType::Ret
        | OpType::Alloca
        | OpType::GlobalAlloca
        | OpType::Declare
    )
  }
}

impl<'a> Pass<'a> for DCE<'a> {
  fn name(&self) -> &str {
    "DCE"
  }
  fn mount(&mut self, program: &'a mut yachiyo::ir::mid::IR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    fn check(this: &mut DCE, operand: &Operand) {
      let program = this.cx.ir();
      match operand {
        Operand::Value(id) => {
          let op_id = *id;
          if this.is_dead(operand) && !this.is_impure(Operand::Value(op_id)) {
            this.worklist.push_back(*operand);
          }
        }
        Operand::Global(id) => {
          let global_id = *id;
          if this.is_dead(operand) && !program.globals[global_id].is_impure() {
            this.worklist.push_back(*operand);
          }
        }
        Operand::Int(_)
        | Operand::Float(_)
        | Operand::Bool(_)
        | Operand::Undefined
        | Operand::Param(_) => { /* do nothing */ }
        Operand::BB(_) | Operand::Func(_) => unreachable!("Unexpected operand: {:?}", operand),
      }
    }

    let call_graph = analyze::<CallGraphAnalysis>(self.cx.ir());
    let sccs = analyze::<SCCAnalysis>(&call_graph);
    let cx_ptr = &mut self.cx as *mut PassContext;
    self.pureness = analyze::<PurenessAnalysis>((unsafe { &mut *cx_ptr }, &call_graph, &sccs));

    let func_ids = self.cx.ir().funcs.collect_internal();
    for func_id in func_ids {
      let func_id = Operand::Func(func_id);
      self.init(func_id);

      while let Some(op_id) = self.worklist.pop_front() {
        let bb_id = match op_id {
          Operand::Value(_) => self.cx.op_bb(op_id),
          Operand::Global(_) => Operand::Undefined,
          _ => unreachable!("DCE: operand is not removable: {:?}", op_id),
        };

        if let Operand::Value(id) = op_id {
          let bb = bb_id.get_bb_id();
          if !self
            .cx
            .get_bb(Operand::BB(bb))
            .cur
            .iter()
            .any(|inst| inst.get_op_id() == id)
          {
            continue;
          }
          self.cx.set_current_block(bb_id);
        }
        let removed_op = match op_id {
          Operand::Global(_) => self.cx.remove_op(op_id, None),
          _ => self.cx.remove_op(op_id, Some(bb_id)),
        };

        // Check the operands of the removed instruction
        match_src! {
            target: removed_op.data.clone(),
            bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: OpData { lhs, rhs } => {
                check(self, &lhs);
                check(self, &rhs);
            },
            un_ops: [Sitofp, Fptosi, Zext, Uitofp],
            un_arm: OpData { value } => {
                check(self, &value);
            },
            fallback: {
                // In DCE, Load is pure.
                OpData::Load { addr } => {
                    check(self, &addr);
                }
                OpData::GEP { base, indices } => {
                    check(self, &base);
                    for index in indices.iter() {
                        check(self, index);
                    }
                }

                OpData::Phi { incomings } => {
                  for phi_incoming in incomings.iter() {
                    if let PhiIncoming::Data { value, bb: _ } = phi_incoming {
                      check(self, value);
                    }
                  }
                }

                OpData::Call { args, .. } => {
                  for arg in args.iter() {
                    check(self, arg);
                  }
                }

                OpData::Store { .. }
                | OpData::Br { .. }
                | OpData::Jump { .. }
                | OpData::Ret { .. }
                | OpData::Alloca(_)
                | OpData::GlobalAlloca(_)
                | OpData::Declare { .. } => {
                    unreachable!(
                        "DCE: impure instruction should not be in the worklist: {:?}",
                        removed_op
                    );
                }
            }
        }
      }
    }
  }
}
