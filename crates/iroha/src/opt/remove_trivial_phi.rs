//! Remove Trivial Phi.

use yachiyo::ir::mid::{Attr, Builder, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::arena::ArenaItem;

enum CheckType {
  Empty,           // No non-phi incoming value. We can replace the phi with undef.
  Single(Operand), // The single non-phi incoming value. We can replace the phi with this value.
  Ignore,          // Multiple or non-phi
}

#[derive(Default)]
pub struct RemoveTrivialPhi<'a> {
  program: Option<&'a mut IR>,
  builder: Builder,
  phi_ids: Vec<Operand>,

  // Ancillary state fields
  worklist: Vec<(Operand, Operand, CheckType)>, // Vec of (PhiId, BBId, CheckType)
  op_to_bb: Vec<Operand>,                       // Mapping from OpId to BBId
}

impl<'a> RemoveTrivialPhi<'a> {
  fn check(program: &IR, current_function: Operand, phi: Operand) -> CheckType {
    let dfg = &program.funcs[current_function].dfg;
    let phi_op = &dfg[phi.clone()];
    match &phi_op.data {
      OpData::Phi { incomings } => {
        let mut distinct: Vec<(Operand, Operand)> = vec![];
        for phi_incoming in incomings.iter() {
          let (value, bb_id) = match phi_incoming {
            PhiIncoming::Data { value, bb } => (value, bb),
            PhiIncoming::None => continue,
          };
          // Crucial: Treat Undefined as a concrete value, you should not ignore it.
          if *value == phi {
            continue;
          }

          if distinct.iter().all(|(v, _)| *v != *value) {
            distinct.push((value.clone(), bb_id.clone()));
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

  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id.clone()));
    let program = self.program.as_deref_mut().unwrap();
    let func = &program.funcs[func_id.clone()];

    self.op_to_bb.clear();
    self.op_to_bb.resize(func.dfg.storage.len(), Operand::BB(0));
    func
      .cfg
      .storage
      .iter()
      .enumerate()
      .for_each(|(bb_id, item)| {
        if let ArenaItem::Data(bb) = item {
          for op_id in bb.cur.iter() {
            self.op_to_bb[op_id.get_op_id()] = Operand::BB(bb_id);
          }
        }
      });

    self.phi_ids = program.get_all_ops(self.builder.current_function.clone(), OpType::Phi);
    self.worklist = self
      .phi_ids
      .iter()
      .map(|phi_id| {
        let check_result = Self::check(program, func_id.clone(), phi_id.clone());
        (
          phi_id.clone(),
          self.op_to_bb[phi_id.get_op_id()].clone(),
          check_result,
        )
      })
      .collect();
  }

  fn remove_phi(&mut self) {
    // Check whether the phi_ids are valid
    while let Some((phi_id, bb_id, check_result)) = self.worklist.pop() {
      let uses = {
        let func_id = match self.builder.current_function.clone() {
          Some(id) => id,
          None => panic!("RemoveTrivialPhi: no current function"),
        };
        let func = &mut self.program.as_mut().unwrap().funcs[func_id];
        let phi_op = &mut func.dfg[phi_id.clone()];
        // Remove OldIdx Attr
        phi_op.attrs.retain(|attr| !matches!(attr, Attr::OldIdx(_)));
        phi_op.users.clone()
      };
      let current_function = self.builder.current_function.clone().unwrap();
      match check_result {
        CheckType::Empty => {
          self.program.as_deref_mut().unwrap().replace_all_uses(
            self.builder.current_function.clone(),
            phi_id.clone(),
            Operand::Undefined,
          );
          for (user, _) in uses {
            // Ignore phi itself, since it will be removed later and should not pushed to worklist again.
            if user == phi_id {
              continue;
            }
            let check_result = Self::check(
              self.program.as_ref().unwrap(),
              current_function.clone(),
              user.clone(),
            );
            if matches!(check_result, CheckType::Empty | CheckType::Single(_)) {
              if let Some((id, bb)) = self
                .phi_ids
                .iter()
                .find(|id| **id == user)
                .map(|id| (id.clone(), self.op_to_bb[id.get_op_id()].clone()))
              {
                // We should check whether the user phi is already in the worklist to avoid duplicate entries.
                let pos = self.worklist.iter().position(|(w_id, _, _)| *w_id == id);
                if let Some(pos) = pos {
                  self.worklist[pos] = (id.clone(), bb.clone(), check_result);
                } else {
                  self.worklist.push((id.clone(), bb.clone(), check_result));
                }
              }
            }
          }
          self.program.as_deref_mut().unwrap().remove_op(
            self.builder.current_function.clone(),
            phi_id,
            Some(bb_id),
          );
        }
        CheckType::Single(value) => {
          self.program.as_deref_mut().unwrap().replace_all_uses(
            self.builder.current_function.clone(),
            phi_id.clone(),
            value,
          );
          for (user, _) in uses {
            if user == phi_id {
              continue;
            }
            let check_result = Self::check(
              self.program.as_ref().unwrap(),
              current_function.clone(),
              user.clone(),
            );
            if matches!(check_result, CheckType::Empty | CheckType::Single(_)) {
              if let Some((id, bb)) = self
                .phi_ids
                .iter()
                .find(|id| **id == user)
                .map(|id| (id.clone(), self.op_to_bb[id.get_op_id()].clone()))
              {
                // We should check whether the user phi is already in the worklist to avoid duplicate entries.
                let pos = self.worklist.iter().position(|(w_id, _, _)| *w_id == id);
                if let Some(pos) = pos {
                  self.worklist[pos] = (id.clone(), bb.clone(), check_result);
                } else {
                  self.worklist.push((id.clone(), bb.clone(), check_result));
                }
              }
            }
          }
          self.program.as_deref_mut().unwrap().remove_op(
            self.builder.current_function.clone(),
            phi_id,
            Some(bb_id),
          );
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
    self.program = Some(program);
  }

  fn run(&mut self) {
    for idx in self.program.as_ref().unwrap().funcs.collect_internal() {
      self.init(Operand::Func(idx));
      self.remove_phi();
    }
  }
}
