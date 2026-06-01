//! Vectorization via SLP.

use yachiyo::analysis::{LoopId, Loops, SCEVExpr};
use yachiyo::ir::mid::{OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};

use super::TripCount;
use crate::analysis::SCEV;

use kaguya::kaguya_hime;

use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct Vectorize<'a> {
  cx: PassContext<'a>,
  slp_sets: FxHashMap<Operand, Vec<Operand>>,
}

impl Vectorize<'_> {
  /// Mark SLP vectorizable Attr on instruction.
  fn try_slp(&self, inst_id: Operand, lp_id: LoopId, scev: &mut SCEV) {
    let Some(iv) = scev.get_main_iv_for_loop(lp_id) else {
      return;
    };

    kaguya_hime!(
      self.cx,
      match inst_id {
        // sum = sum + a[i]
        AddI($phi @ Phi($incoming), Load(GEP(_, [_, $idx] | [$idx]))) | AddI(Load(GEP(_, [_, $idx] | [$idx])), $phi @ Phi($incoming)) => {
          // Check whether current instruction is latch value of iv.
          let has_sum = incoming.iter().any(|incoming| {
            let PhiIncoming::Data { value, .. } = incoming else {
              return false;
            };
            *value == inst_id
          });
          if !has_sum || phi != iv {
            return;
          }

          if idx != iv {
            return;
          }


        },
        // sum = sum + a[i] + b[i]
        AddI(Phi([PhiIncoming($acc, _), ..]), AddI(Load(GEP($base1, [Int(0), $idx1])), Load(GEP($base2, [Int(0), $idx2])))) => {
          if base1 == base2 {
            return;
          }
          if idx1 == idx2 {
            // Found a vectorizable pattern, we can vectorize it.
          }
        },
        // c[i] = a[i] + b[i]
        Store(GEP($base1, [Int(0), $idx1]), AddI(Load(GEP($base2, [Int(0), $idx2])), Load(GEP($base3, [Int(0), $idx3])))) => {
          if base1 == base2 || base1 == base3 || base2 == base3 {
            return;
          }
          if idx1 == idx2 && idx1 == idx3 {
            // Found a vectorizable pattern, we can vectorize it.
          }
        },
        // c[i][j] = a[i][k] + b[k][j]
        Store(GEP($base_1, [Int(0), $i1, $j1]), AddI(Phi([PhiIncoming($iv, _), ..]), MulI(Load(GEP($base_2, [Int(0), $i2, $j2])), Load(GEP($base_3, [Int(0), $i3, $j3]))))) => {
          if base_1 == base_2 || base_1 == base_3 || base_2 == base_3 {
            return;
          }
          if i1 == i2 && j1 == j3 && i3 == j2 && i3 == iv {
            // Found a matrix multiplication pattern, we can vectorize it.
          }
        }
      }
    );
  }
}

impl<'a> Pass<'a> for Vectorize<'a> {
  fn name(&self) -> &str {
    "Vectorize"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {}
}
