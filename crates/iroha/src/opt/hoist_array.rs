//! Hoist Local Arrays to Global.
//! If any array initialization with a loop is detected, we will replace the loop with global declaration.

use super::TripCount;
use crate::analysis::{DomAnalysis, LoopAnalysis, SCEV};

use yachiyo::analysis::{analyze, Analysis, LoopId, MemLoc, SCEVExpr};
use yachiyo::ast::Literal;
use yachiyo::ir::mid::{Attr, Op, OpData, Operand, IR};
use yachiyo::pass::{Pass, PassContext};

#[derive(Default)]
pub struct HoistArray<'a> {
  cx: PassContext<'a>,
}

impl HoistArray<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
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
          let op = self.cx.get_op(inst_id);
          let op_data = &op.data;
          if let OpData::Store { addr, value } = op_data {
            let value_scev_id = scev.get_op_scev(*value);

            // Array to be hoisted should have Alloca base.
            let MemLoc {
              base: base @ (Operand::Value(_) | Operand::Global(_)),
              offset,
              ..
            } = self.cx.compute_mem_loc(*addr)
            else {
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
              let name = self.cx.get_attrs(base).iter().find_map(|attr| {
                if let Attr::Name(name) = attr {
                  Some(name.clone())
                } else if let Attr::GlobalArray { name, .. } = attr {
                  Some(name.clone())
                } else {
                  None
                }
              });

              let mut guard = self.cx.guard();
              guard.set_current_func(None);
              guard.create(Op::new(
                typ.clone(),
                vec![Attr::GlobalArray {
                  name: name.unwrap(),
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
                self.cx.remove_op(base, Some(bb_id));
              }
              Operand::Global(_) => {
                let mut guard = self.cx.guard();
                guard.set_current_func(None);
                guard.replace_all_uses(base, global_arr_id);
                guard.remove_op(base, None);
              }
              _ => unreachable!(),
            }
            // Mark Store as dead
            self.cx.add_attr(inst_id, Attr::Dead);
          }
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
