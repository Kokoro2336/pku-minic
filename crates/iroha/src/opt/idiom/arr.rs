//! Hoist Local Arrays to Global.
//! If any array initialization with a loop is detected, we will replace the loop with global declaration.

use crate::analysis::{DomAnalysis, LoopAnalysis, SCEV};
use crate::opt::TripCount;

use yachiyo::analysis::{analyze, Analysis, LoopId, MemLoc, SCEVExpr};
use yachiyo::ast::Literal;
use yachiyo::ir::mid::{Attr, Op, OpData, Operand, IR};
use yachiyo::pass::{Pass, PassContext};

use kaguya::kaguya_hime;

#[derive(Default)]
pub struct HoistArray<'a> {
  cx: PassContext<'a>,
}

impl HoistArray<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn hoistable_array_name(&self, base: Operand) -> Option<String> {
    let func = self.cx.current_func();
    if func.name != "main" {
      return None;
    }

    let name = self.cx.get_attrs(base).iter().find_map(|attr| {
      if let Attr::Name(name) = attr {
        Some(name.clone())
      } else if let Attr::GlobalArray { name, .. } = attr {
        Some(name.clone())
      } else {
        None
      }
    })?;

    match base {
      Operand::Value(id) => {
        if !matches!(self.cx.get_op_data(base), OpData::Alloca(_)) {
          return None;
        }
        let entry = Operand::BB(self.cx.get_cfg().entry?);
        if self.cx.op_bb(base) != entry {
          return None;
        }
        Some(format!("__hoist_{}_{}_{}", func.name, name, id))
      }
      Operand::Global(_) => Some(name),
      _ => None,
    }
  }

  fn is_unconditional_loop_store(&self, store_id: Operand, lp_id: LoopId, scev: &SCEV) -> bool {
    let Some(latch_id) = self
      .cx
      .get_latch_id(scev.loops[lp_id].header, &scev.dom_tree)
    else {
      return false;
    };
    self.cx.op_bb(store_id) == latch_id
  }

  fn folded_array_length(
    &self,
    indices: &[Operand],
    mut lp_id: LoopId,
    scev: &mut SCEV,
  ) -> Option<i64> {
    let mut arr_length = 1;
    let mut consumed = 0;

    for index in indices.iter().rev() {
      let Some(trip_count @ TripCount { iv, .. }) = TripCount::try_build(&self.cx, scev, lp_id)
      else {
        break;
      };
      if *index != iv {
        break;
      }

      arr_length *= trip_count.get_trip_count();
      consumed += 1;

      let Some(parent) = scev.loops[lp_id].parent else {
        break;
      };
      lp_id = parent;
    }

    if consumed == 0 {
      return None;
    }

    indices
      .iter()
      .take(indices.len() - consumed)
      .all(|index| *index == Operand::Int(0))
      .then_some(arr_length)
  }

  fn run(&mut self, mut scev: SCEV) {
    for lp_id in (0..scev.loops.len()).rev() {
      let lp_id: LoopId = lp_id.into();
      let Some(
        trip_count @ TripCount {
          iv,
          start: SCEVExpr::Const(start),
          step: SCEVExpr::Const(step),
          ..
        },
      ) = TripCount::try_build(&self.cx, &mut scev, lp_id)
      else {
        continue;
      };

      // Try to find store whose value is only relative to iv, and then replace initialization logic.
      let to_visit = scev.loops[lp_id].owned_blocks.clone();
      for bb_id in to_visit.iter() {
        let bb_id = Operand::BB(bb_id);
        for inst_id in self.cx.get_bb(bb_id).cur.clone() {
          kaguya_hime!(
            self.cx,
            match inst_id {
              // arr[i][j]... = const
              Store($addr @ GEP(_, $indices), Int($c)) => {
                if !self.is_unconditional_loop_store(inst_id, lp_id, &scev) {
                  continue;
                }

                let indices = indices.clone();

                // Array to be hoisted should have Alloca base.
                let MemLoc {
                  base: base @ (Operand::Value(_) | Operand::Global(_)),
                  ..
                } = self.cx.compute_mem_loc(addr)
                else {
                  continue;
                };

                let Some(name) = self.hoistable_array_name(base) else {
                  continue;
                };

                let Some(arr_length) = self.folded_array_length(&indices, lp_id, &mut scev) else {
                  continue;
                };

                // Create global array
                let global_arr_id = {
                  let typ = self.cx.get_op_type(base);
                  let values = if c == 0 {
                    None
                  } else {
                    Some(vec![Literal::Int(c); arr_length as usize])
                  };

                  let mut guard = self.cx.guard();
                  guard.set_current_func(None);
                  guard.create(Op::new(
                    typ.clone(),
                    vec![Attr::GlobalArray {
                      name,
                      mutable: true,
                      typ: typ.unwrap_ptr(),
                      values,
                    }],
                    OpData::GlobalAlloca(typ.unwrap_ptr()),
                  ))
                };
                match base {
                  Operand::Value(_) => {
                    // RAUW
                    self.cx.replace_all_uses(base, global_arr_id);
                    // Remove the original alloca and store instructions.
                    self.cx.remove_op(base);
                  }
                  Operand::Global(_) => {
                    let mut guard = self.cx.guard();
                    guard.set_current_func(None);
                    guard.replace_all_uses(base, global_arr_id);
                    guard.remove_op(base);
                  }
                  _ => unreachable!(),
                }
                // Mark Store as dead
                self.cx.add_attr(inst_id, Attr::Dead);
              },

              // arr[i] = i + const
              Store($addr, $value) => {
                if !self.is_unconditional_loop_store(inst_id, lp_id, &scev) {
                  continue;
                }

                let value_scev_id = scev.get_op_scev(value);

                // Array to be hoisted should have Alloca base.
                let MemLoc {
                  base: base @ (Operand::Value(_) | Operand::Global(_)),
                  offset,
                  ..
                } = self.cx.compute_mem_loc(addr)
                else {
                  continue;
                };

                let Some(name) = self.hoistable_array_name(base) else {
                  continue;
                };

                let addr_scev_id = scev.get_affine_scev(&offset);
                let iv_scev_id = scev.get_op_scev(iv);

                // For now, we only detect arr[i] = i + const.
                if !scev.depends_only_on(addr_scev_id, iv_scev_id)
                  || !scev.depends_only_on(value_scev_id, iv_scev_id)
                {
                  continue;
                }
                let c = scev.compute_const(value_scev_id);

                // Create global array
                let global_arr_id = {
                  let typ = self.cx.get_op_type(base);
                  let mut values = Vec::new();
                  for i in 0..trip_count.get_trip_count() {
                    values.push(Literal::Int((start + i * step + c) as i32));
                  }

                  let mut guard = self.cx.guard();
                  guard.set_current_func(None);
                  guard.create(Op::new(
                    typ.clone(),
                    vec![Attr::GlobalArray {
                      name,
                      mutable: true,
                      typ: typ.unwrap_ptr(),
                      values: Some(values),
                    }],
                    OpData::GlobalAlloca(typ.unwrap_ptr()),
                  ))
                };
                match base {
                  Operand::Value(_) => {
                    // RAUW
                    self.cx.replace_all_uses(base, global_arr_id);
                    // Remove the original alloca and store instructions.
                    self.cx.remove_op(base);
                  }
                  Operand::Global(_) => {
                    let mut guard = self.cx.guard();
                    guard.set_current_func(None);
                    guard.replace_all_uses(base, global_arr_id);
                    guard.remove_op(base);
                  }
                  _ => unreachable!(),
                }
                // Mark Store as dead
                self.cx.add_attr(inst_id, Attr::Dead);
              },
            }
          );
        }
      }
    }
  }
}

impl<'a> Pass<'a> for HoistArray<'a> {
  fn name(&self) -> &str {
    "HoistArray"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      let graph = self.cx.extract_cfg();
      let (dom_tree, _) = analyze::<DomAnalysis>(&graph);
      let (loops, block_to_loop) = analyze::<LoopAnalysis>(&graph);
      let scev = <SCEV as Analysis>::new((&mut self.cx, loops, block_to_loop, dom_tree));
      self.run(scev);
    }
  }
}
