//! TripCount for Identifying Loop Induction Variables.

use yachiyo::analysis::{AddRecInfo, LoopId, SCEVExpr};
use yachiyo::ir::mid::{OpData, Operand};
use yachiyo::pass::PassContext;

use super::CanonicalExpr;
use crate::analysis::SCEV;

/// Canonicalized Loop Trip Count.
/// Only support loops with const start, step and bound for now.
pub struct TripCount {
  pub iv: Operand,
  pub start: SCEVExpr,
  pub step: SCEVExpr,
  pub bound: SCEVExpr,
  pub reverse: bool,
  pub inclusive: bool,
}

impl TripCount {
  pub fn try_build(cx: &PassContext, scev: &mut SCEV, lp_id: LoopId) -> Option<Self> {
    let loop_data = &scev.loops[lp_id];
    let header_id = loop_data.header;
    let header_term = cx.get_term(header_id);
    let header_term_data = cx.get_op_data(header_term);
    let OpData::Br { cond, .. } = header_term_data else {
      return None;
    };

    let cmp_data = cx.get_op_data(*cond);
    let cmp = CanonicalExpr::from(cmp_data);

    match cmp {
      CanonicalExpr::Ne(lhs, rhs) => {
        let (lhs_scev, rhs_scev) = (scev.get_op_scev(lhs), scev.get_op_scev(rhs));
        let (Some(start), Some(step), Some(bound), iv) =
          (if let Some(AddRecInfo { start, step, iv }) = scev.get_add_rec_for_loop(lhs_scev, lp_id)
          {
            (
              scev[start].as_const(),
              scev[step].as_const(),
              scev[rhs_scev].as_const(),
              iv,
            )
          } else if let Some(AddRecInfo { start, step, iv }) =
            scev.get_add_rec_for_loop(rhs_scev, lp_id)
          {
            // We only support induction variables that are compared to loop-invariant bounds for now.
            (
              scev[start].as_const(),
              scev[step].as_const(),
              scev[lhs_scev].as_const(),
              iv,
            )
          } else {
            return None;
          })
        else {
          return None;
        };
        if (start > bound && step < 0) || (start < bound && step > 0) {
          Some(TripCount {
            iv,
            start: SCEVExpr::Const(start),
            step: SCEVExpr::Const(step),
            bound: SCEVExpr::Const(bound),
            reverse: step < 0,
            inclusive: false,
          })
        } else {
          None
        }
      }
      CanonicalExpr::Lt(lhs, rhs) => {
        let (lhs_scev, rhs_scev) = (scev.get_op_scev(lhs), scev.get_op_scev(rhs));
        if let Some(AddRecInfo { start, step, iv }) = scev.get_add_rec_for_loop(lhs_scev, lp_id) {
          // We only support induction variables that are compared to loop-invariant bounds for now.
          let (Some(start), Some(step), Some(bound)) = (
            scev[start].as_const(),
            scev[step].as_const(),
            scev[rhs_scev].as_const(),
          ) else {
            return None;
          };
          if start < bound && step > 0 {
            Some(TripCount {
              iv,
              start: SCEVExpr::Const(start),
              step: SCEVExpr::Const(step),
              bound: SCEVExpr::Const(bound),
              reverse: false,
              inclusive: false,
            })
          } else {
            None
          }
        } else if let Some(AddRecInfo { start, step, iv }) =
          scev.get_add_rec_for_loop(rhs_scev, lp_id)
        {
          // We only support induction variables that are compared to loop-invariant bounds for now.
          let (Some(start), Some(step), Some(bound)) = (
            scev[start].as_const(),
            scev[step].as_const(),
            scev[lhs_scev].as_const(),
          ) else {
            return None;
          };
          if start > bound && step < 0 {
            return Some(TripCount {
              iv,
              start: SCEVExpr::Const(start),
              step: SCEVExpr::Const(step),
              bound: SCEVExpr::Const(bound),
              reverse: true,
              inclusive: false,
            });
          } else {
            return None;
          }
        } else {
          return None;
        }
      }
      CanonicalExpr::Le(lhs, rhs) => {
        let (lhs_scev, rhs_scev) = (scev.get_op_scev(lhs), scev.get_op_scev(rhs));
        if let Some(AddRecInfo { start, step, iv }) = scev.get_add_rec_for_loop(lhs_scev, lp_id) {
          let (Some(start), Some(step), Some(bound)) = (
            scev[start].as_const(),
            scev[step].as_const(),
            scev[rhs_scev].as_const(),
          ) else {
            return None;
          };
          if start <= bound && step > 0 {
            Some(TripCount {
              iv,
              start: SCEVExpr::Const(start),
              step: SCEVExpr::Const(step),
              bound: SCEVExpr::Const(bound),
              reverse: false,
              inclusive: true,
            })
          } else {
            None
          }
        } else if let Some(AddRecInfo { start, step, iv }) =
          scev.get_add_rec_for_loop(rhs_scev, lp_id)
        {
          let (Some(start), Some(step), Some(bound)) = (
            scev[start].as_const(),
            scev[step].as_const(),
            scev[lhs_scev].as_const(),
          ) else {
            return None;
          };
          if start >= bound && step < 0 {
            return Some(TripCount {
              iv,
              start: SCEVExpr::Const(start),
              step: SCEVExpr::Const(step),
              bound: SCEVExpr::Const(bound),
              reverse: true,
              inclusive: true,
            });
          } else {
            return None;
          }
        } else {
          return None;
        }
      }
      // TODO: Eq not supported for now.
      _ => None,
    }
  }

  pub fn get_trip_count(&self) -> i64 {
    let (Some(start), Some(step), Some(bound)) = (
      self.start.as_const(),
      self.step.as_const(),
      self.bound.as_const(),
    ) else {
      unreachable!()
    };
    if self.reverse {
      if self.inclusive {
        (start - bound) / (-step) + 1
      } else {
        (start - bound + (-step) - 1) / (-step)
      }
    } else if self.inclusive {
      (bound - start) / step + 1
    } else {
      (bound - start + step - 1) / step
    }
  }

  pub fn get_final_trip(&self) -> i64 {
    let (Some(start), Some(step), Some(bound)) = (
      self.start.as_const(),
      self.step.as_const(),
      self.bound.as_const(),
    ) else {
      unreachable!()
    };
    if self.reverse {
      if (start - bound) % (-step) == 0 {
        bound
      } else {
        start + ((start - bound) / (-step)) * step
      }
    } else if (bound - start) % step == 0 {
      bound
    } else {
      start + ((bound - start) / step) * step
    }
  }
}
