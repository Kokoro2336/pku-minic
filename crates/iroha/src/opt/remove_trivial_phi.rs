//! Remove Trivial Phi.

use std::collections::HashSet;

use crate::analysis::{DomAnalysis, DomTree};
use yachiyo::ir::mid::{Attr, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};

enum CheckType {
  Empty,           // No non-phi incoming value. We can replace the phi with undef.
  Single(Operand), // The single non-phi incoming value. We can replace the phi with this value.
  Ignore,          // Multiple or non-phi
}

#[derive(Default)]
pub struct RemoveTrivialPhi<'a> {
  cx: PassContext<'a>,
  phi_ids: Vec<Operand>,

  // Ancillary state fields
  worklist: Vec<(Operand, Operand, CheckType)>, // Vec of (PhiId, BBId, CheckType)
}

impl RemoveTrivialPhi<'_> {
  fn check(program: &IR, current_function: Operand, phi: Operand) -> CheckType {
    let dfg = &program.funcs[current_function].dfg;
    let phi_op = &dfg[phi];
    match &phi_op.data {
      OpData::Phi { incomings } => {
        let mut distinct: Vec<(Operand, Operand)> = vec![];
        for phi_incoming in incomings.iter() {
          let (value, bb_id) = match phi_incoming {
            PhiIncoming::Data { value, bb } => (value, bb),
            PhiIncoming::None => continue,
          };
          // Undef does not constrain the phi result, so only concrete non-self values count.
          if *value == phi || *value == Operand::Undefined {
            continue;
          }

          if distinct.iter().all(|(v, _)| *v != *value) {
            distinct.push((*value, *bb_id));
            if distinct.len() > 1 {
              return CheckType::Ignore;
            }
          }
        }

        if distinct.is_empty() {
          CheckType::Empty
        } else {
          let (value, _) = distinct.pop().unwrap();
          CheckType::Single(value)
        }
      }
      // If it's not a phi, we treat it as multiple to be safe, since we only want to remove trivial phis.
      _ => CheckType::Ignore,
    }
  }

  fn value_available_in_block(&self, value: Operand, block: Operand, dom_tree: &DomTree) -> bool {
    match value {
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Param(_)
      | Operand::Global(_)
      | Operand::Undefined => true,
      Operand::Value(_) => dom_tree.is_dom(self.cx.op_bb(value).get_bb_id(), block.get_bb_id()),
      Operand::BB(_) | Operand::Func(_) => false,
    }
  }

  fn value_available_at_inst(&self, value: Operand, inst: Operand, dom_tree: &DomTree) -> bool {
    match value {
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Param(_)
      | Operand::Global(_)
      | Operand::Undefined => true,
      Operand::Value(_) => {
        let value_bb = self.cx.op_bb(value);
        let inst_bb = self.cx.op_bb(inst);
        if value_bb != inst_bb {
          return dom_tree.is_dom(value_bb.get_bb_id(), inst_bb.get_bb_id());
        }

        let cur = &self.cx.get_bb(inst_bb).cur;
        let value_pos = cur.iter().position(|op| *op == value);
        let inst_pos = cur.iter().position(|op| *op == inst);
        matches!((value_pos, inst_pos), (Some(value_pos), Some(inst_pos)) if value_pos < inst_pos)
      }
      Operand::BB(_) | Operand::Func(_) => false,
    }
  }

  fn can_replace_all_uses_with(&self, phi_id: Operand, value: Operand, dom_tree: &DomTree) -> bool {
    self.cx.users(phi_id).iter().all(|&(user, idx)| {
      if user == phi_id {
        return true;
      }

      let Operand::Value(_) = user else {
        return false;
      };

      if let OpData::Phi { incomings } = self.cx.get_op_data(user) {
        let Some(PhiIncoming::Data { bb, .. }) = incomings.get(idx) else {
          return false;
        };
        self.value_available_in_block(value, *bb, dom_tree)
      } else {
        self.value_available_at_inst(value, user, dom_tree)
      }
    })
  }

  fn is_dead_phi_web(&self, phi_id: Operand, visited: &mut HashSet<Operand>) -> bool {
    if !visited.insert(phi_id) {
      return true;
    }

    self.cx.users(phi_id).iter().all(|&(user, _)| {
      if user == phi_id {
        return true;
      }

      let Operand::Value(_) = user else {
        return false;
      };

      self.cx.get_op_data(user).is(OpType::Phi) && self.is_dead_phi_web(user, visited)
    })
  }

  fn single_replacement(
    &self,
    phi_id: Operand,
    value: Operand,
    dom_tree: &DomTree,
  ) -> Option<Operand> {
    if self.can_replace_all_uses_with(phi_id, value, dom_tree) {
      Some(value)
    } else if self.is_dead_phi_web(phi_id, &mut HashSet::new()) {
      Some(Operand::Undefined)
    } else {
      None
    }
  }

  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));

    self.phi_ids = self.cx.get_all_ops(OpType::Phi);
    self.worklist = self
      .phi_ids
      .iter()
      .map(|phi_id| {
        let check_result = Self::check(self.cx.ir(), func_id, *phi_id);
        (*phi_id, self.cx.op_bb(*phi_id), check_result)
      })
      .collect();
  }

  fn remove_phi(&mut self, dom_tree: &DomTree) {
    // Check whether the phi_ids are valid
    while let Some((phi_id, bb_id, check_result)) = self.worklist.pop() {
      {
        let phi_op = self.cx.get_op_mut(phi_id);
        // Remove OldIdx Attr
        phi_op.attrs.retain(|attr| !matches!(attr, Attr::OldIdx(_)));
      }
      let uses = self.cx.users(phi_id).to_vec();
      let current_function = self.cx.get_current_func_id();
      match check_result {
        CheckType::Empty => {
          self.cx.replace_all_uses(phi_id, Operand::Undefined);
          for (user, _) in uses {
            // Ignore phi itself, since it will be removed later and should not pushed to worklist again.
            if user == phi_id {
              continue;
            }
            let check_result = Self::check(self.cx.ir(), current_function, user);
            if matches!(check_result, CheckType::Empty | CheckType::Single(_)) {
              if let Some((id, bb)) = self
                .phi_ids
                .iter()
                .find(|id| **id == user)
                .map(|id| (*id, self.cx.op_bb(*id)))
              {
                // We should check whether the user phi is already in the worklist to avoid duplicate entries.
                let pos = self.worklist.iter().position(|(w_id, _, _)| *w_id == id);
                if let Some(pos) = pos {
                  self.worklist[pos] = (id, bb, check_result);
                } else {
                  self.worklist.push((id, bb, check_result));
                }
              }
            }
          }
          self.cx.remove_op(phi_id, Some(bb_id));
        }
        CheckType::Single(value) => {
          let Some(value) = self.single_replacement(phi_id, value, dom_tree) else {
            continue;
          };
          self.cx.replace_all_uses(phi_id, value);
          for (user, _) in uses {
            if user == phi_id {
              continue;
            }
            let check_result = Self::check(self.cx.ir(), current_function, user);
            if matches!(check_result, CheckType::Empty | CheckType::Single(_)) {
              if let Some((id, bb)) = self
                .phi_ids
                .iter()
                .find(|id| **id == user)
                .map(|id| (*id, self.cx.op_bb(*id)))
              {
                // We should check whether the user phi is already in the worklist to avoid duplicate entries.
                let pos = self.worklist.iter().position(|(w_id, _, _)| *w_id == id);
                if let Some(pos) = pos {
                  self.worklist[pos] = (id, bb, check_result);
                } else {
                  self.worklist.push((id, bb, check_result));
                }
              }
            }
          }
          self.cx.remove_op(phi_id, Some(bb_id));
        }
        CheckType::Ignore => {}
      }
    }
  }
}

impl<'a> Pass<'a> for RemoveTrivialPhi<'a> {
  fn name(&self) -> &str {
    "RemoveTrivialPhi"
  }

  fn mount(&mut self, program: &'a mut IR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    for idx in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(idx);
      let (dom_tree, _) = &*self.cx.analyze::<DomAnalysis>(self.cx.get_func(func_id));
      self.init(func_id);
      self.remove_phi(dom_tree);
    }
  }
}
