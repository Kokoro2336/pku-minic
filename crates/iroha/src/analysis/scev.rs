//! Scalar Evolution (SCEV).
//! SCEV never produces a result, it receives an Arena and then update it.

use yachiyo::analysis::{
  AddRecInfo, AffineExpr, Analysis, DomTree, LoopId, Loops, MemLoc, SCEVArena, SCEVExpr, SCEVId,
};
use yachiyo::base::Type;
use yachiyo::ir::mid::{OpData, Operand};
use yachiyo::pass::PassContext;

use crate::opt::CanonicalExpr;
use rustc_hash::FxHashSet;

#[allow(clippy::upper_case_acronyms)]
pub struct SCEV<'a> {
  cx: &'a PassContext<'a>,
  loops: &'a Loops,
  block_to_loop: &'a [Option<LoopId>],
  dom_tree: &'a DomTree,
  arena: SCEVArena,
}

impl SCEV<'_> {
  fn is_operand_loop_invariant(&self, op: Operand, loop_id: LoopId) -> bool {
    let mut visiting = FxHashSet::default();
    self.is_operand_loop_invariant_rec(op, loop_id, &mut visiting)
  }

  fn is_operand_loop_invariant_rec(
    &self,
    op: Operand,
    loop_id: LoopId,
    visiting: &mut FxHashSet<Operand>,
  ) -> bool {
    match op {
      Operand::Global(_)
      | Operand::Param(_)
      | Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Undefined => true,
      Operand::Value(_) => {
        let op_bb = self.cx.op_bb(op);
        let loop_data = &self.loops[loop_id];

        if !loop_data.blocks.contains(op_bb.get_bb_id()) {
          return true;
        }

        let op_data = self.cx.get_op_data(op);
        if matches!(
          op_data,
          OpData::Call { .. } | OpData::Load { .. } | OpData::Store { .. }
        ) {
          return false;
        }

        if !visiting.insert(op) {
          return false;
        }

        let src = self.cx.get_src(op);
        let result = src
          .into_iter()
          .all(|src_op| self.is_operand_loop_invariant_rec(*src_op, loop_id, visiting));

        visiting.remove(&op);
        result
      }
      Operand::Func(_) | Operand::BB(_) => unreachable!(),
    }
  }

  #[allow(unused)]
  fn is_scev_loop_invariant(&self, scev_id: SCEVId, loop_id: LoopId) -> bool {
    let mut visiting = FxHashSet::default();
    self.is_scev_loop_invariant_rec(scev_id, loop_id, &mut visiting)
  }

  fn is_scev_loop_invariant_rec(
    &self,
    scev_id: SCEVId,
    loop_id: LoopId,
    visiting: &mut FxHashSet<SCEVId>,
  ) -> bool {
    if !visiting.insert(scev_id) {
      return false;
    }

    let result = match &self.arena[scev_id] {
      SCEVExpr::Const(_) => true,
      SCEVExpr::Unknown(op) => self.is_operand_loop_invariant(*op, loop_id),
      SCEVExpr::Add(ops) | SCEVExpr::Mul(ops) => ops
        .iter()
        .all(|&op| self.is_scev_loop_invariant_rec(op, loop_id, visiting)),
      SCEVExpr::AddRec {
        loop_id: scev_loop_id,
        ..
      } => {
        if *scev_loop_id == loop_id {
          false
        } else {
          self.loops.is_ancestor(*scev_loop_id, loop_id)
        }
      }
    };

    visiting.remove(&scev_id);
    result
  }

  /// Return a view of AddRecInfo for current loop. Non-AddRec SCEVExpr in this SCEVId should be invariant in the loop.
  pub fn get_add_rec_for_loop(&mut self, scev_id: SCEVId, loop_id: LoopId) -> Option<AddRecInfo> {
    match self.arena[scev_id].clone() {
      SCEVExpr::AddRec {
        loop_id: scev_loop_id,
        start,
        step,
      } => {
        if scev_loop_id == loop_id {
          Some(AddRecInfo { start, step })
        } else {
          None
        }
      }
      SCEVExpr::Add(terms) => self.add_add_rec(terms, loop_id),
      SCEVExpr::Mul(factors) => self.mul_add_rec(factors, loop_id),
      // These two variants couldn't be transformed to AddRecInfo
      SCEVExpr::Const(_) | SCEVExpr::Unknown(_) => None,
    }
  }

  fn add_add_rec(
    &mut self,
    terms: impl IntoIterator<Item = SCEVId>,
    loop_id: LoopId,
  ) -> Option<AddRecInfo> {
    let mut start = vec![];
    let mut step = vec![];
    let mut saw_add_rec = false;

    for term in terms {
      if let Some(add_rec) = self.get_add_rec_for_loop(term, loop_id) {
        saw_add_rec = true;
        start.push(add_rec.start);
        step.push(add_rec.step);
      } else if self.is_scev_loop_invariant(term, loop_id) {
        start.push(term);
      } else {
        return None;
      }
    }

    if !saw_add_rec {
      return None;
    }

    let start_scev = self.arena.add(start.into_iter());
    let step_scev = self.arena.add(step.into_iter());

    if !self.is_scev_loop_invariant(start_scev, loop_id)
      || !self.is_scev_loop_invariant(step_scev, loop_id)
    {
      return None;
    }

    Some(AddRecInfo {
      start: start_scev,
      step: step_scev,
    })
  }

  fn mul_add_rec(
    &mut self,
    factors: impl IntoIterator<Item = SCEVId>,
    loop_id: LoopId,
  ) -> Option<AddRecInfo> {
    let mut invariants = vec![];
    let mut add_rec = None;

    for factor in factors {
      if let Some(add_rec_info) = self.get_add_rec_for_loop(factor, loop_id) {
        // AddRec * AddRec is not AffineExpr
        if add_rec.is_some() {
          return None;
        }
        add_rec = Some(add_rec_info);
      } else if self.is_scev_loop_invariant(factor, loop_id) {
        invariants.push(factor);
      } else {
        return None;
      }
    }

    let ar = add_rec?;
    let start_mul = self
      .arena
      .mul(invariants.clone().into_iter().chain([ar.start]));
    let step_mul = self.arena.mul(invariants.into_iter().chain([ar.step]));

    if !self.is_scev_loop_invariant(start_mul, loop_id)
      || !self.is_scev_loop_invariant(step_mul, loop_id)
    {
      return None;
    }

    Some(AddRecInfo {
      start: start_mul,
      step: step_mul,
    })
  }

  fn match_add_rec(
    &mut self,
    phi_id: Operand,
    loop_id: LoopId,
    trace_visiting: &mut FxHashSet<Operand>,
  ) -> SCEVId {
    let loop_info = &self.loops[loop_id];
    let header_id = loop_info.header;

    let header = &self.cx.get_bb(header_id);
    if !header.cur.contains(&phi_id) {
      return self.arena.dedup(SCEVExpr::Unknown(phi_id));
    }

    let (pre_header_id, latch_id) =
      header
        .preds
        .iter()
        .fold((None, None), |(pre_header_id, latch_id), (pred_id, _)| {
          if self
            .dom_tree
            .is_dom(header_id.get_bb_id(), pred_id.get_bb_id())
          {
            (pre_header_id, Some(*pred_id))
          } else {
            (Some(*pred_id), latch_id)
          }
        });

    let (Some(pre_header_id), Some(latch_id)) = (pre_header_id, latch_id) else {
      return self.arena.dedup(SCEVExpr::Unknown(phi_id));
    };
    let Some(step_op @ Operand::Value(_)) = self.cx.get_phi_incoming_value(phi_id, latch_id) else {
      return self.arena.dedup(SCEVExpr::Unknown(phi_id));
    };
    let Some(start) = self.cx.get_phi_incoming_value(phi_id, pre_header_id) else {
      return self.arena.dedup(SCEVExpr::Unknown(phi_id));
    };

    let step_data = self.cx.get_op_data(step_op);
    match CanonicalExpr::from(step_data) {
      CanonicalExpr::Add(lhs, rhs) => {
        // TODO: Now we only match op whose lhs is exactly the phi.
        if lhs != phi_id {
          return self.arena.dedup(SCEVExpr::Unknown(phi_id));
        }
        if !self.is_operand_loop_invariant(rhs, loop_id) {
          return self.arena.dedup(SCEVExpr::Unknown(phi_id));
        }

        if self.is_operand_loop_invariant(start, loop_id)
          && self.is_operand_loop_invariant(rhs, loop_id)
        {
          let start_scev = self.trace_op_rec(start, trace_visiting);
          let step_scev = self.trace_op_rec(rhs, trace_visiting);
          self.arena.add_rec(loop_id, start_scev, step_scev)
        } else {
          self.arena.dedup(SCEVExpr::Unknown(step_op))
        }
      }
      CanonicalExpr::Sub(lhs, rhs) => {
        // TODO: Now we only match op whose lhs is exactly the phi.
        if lhs != phi_id {
          return self.arena.dedup(SCEVExpr::Unknown(phi_id));
        }
        if !self.is_operand_loop_invariant(rhs, loop_id) {
          return self.arena.dedup(SCEVExpr::Unknown(phi_id));
        }

        if self.is_operand_loop_invariant(start, loop_id)
          && self.is_operand_loop_invariant(rhs, loop_id)
        {
          let start_scev = self.trace_op_rec(start, trace_visiting);
          let step_scev = self.trace_op_rec(rhs, trace_visiting);
          let neg_step_scev = self.arena.neg(step_scev);
          self.arena.add_rec(loop_id, start_scev, neg_step_scev)
        } else {
          self.arena.dedup(SCEVExpr::Unknown(phi_id))
        }
      }
      _ => self.arena.dedup(SCEVExpr::Unknown(phi_id)),
    }
  }

  fn trace_op(&mut self, op: Operand) -> SCEVId {
    let mut visiting = FxHashSet::default();
    self.trace_op_rec(op, &mut visiting)
  }

  fn trace_op_rec(&mut self, op: Operand, visiting: &mut FxHashSet<Operand>) -> SCEVId {
    match op {
      Operand::Int(c) => self.arena.dedup(SCEVExpr::Const(c as i64)),
      Operand::Value(_) => {
        if self.cx.get_op_type(op) != Type::Int {
          return self.arena.dedup(SCEVExpr::Unknown(op));
        }
        if !visiting.insert(op) {
          return self.arena.dedup(SCEVExpr::Unknown(op));
        }

        let result = match CanonicalExpr::from(self.cx.get_op_data(op)) {
          CanonicalExpr::Add(lhs, rhs) => {
            let lhs_scev = self.trace_op_rec(lhs, visiting);
            let rhs_scev = self.trace_op_rec(rhs, visiting);
            self.arena.add([lhs_scev, rhs_scev].into_iter())
          }
          CanonicalExpr::Mul(lhs, rhs) => {
            let lhs_scev = self.trace_op_rec(lhs, visiting);
            let rhs_scev = self.trace_op_rec(rhs, visiting);
            self.arena.mul([lhs_scev, rhs_scev].into_iter())
          }
          CanonicalExpr::Phi(_) => {
            let phi_bb_id = self.cx.op_bb(op);
            if let Some(phi_loop_id) = self.block_to_loop[phi_bb_id.get_bb_id()] {
              self.match_add_rec(op, phi_loop_id, visiting)
            } else {
              self.arena.dedup(SCEVExpr::Unknown(op))
            }
          }
          _ => self.arena.dedup(SCEVExpr::Unknown(op)),
        };

        visiting.remove(&op);
        result
      }
      Operand::Global(_)
      | Operand::Param(_)
      | Operand::Func(_)
      | Operand::BB(_)
      | Operand::Bool(_)
      | Operand::Float(_)
      | Operand::Undefined => self.arena.dedup(SCEVExpr::Unknown(op)),
    }
  }

  fn affine_to_scev_expr(&mut self, affine_expr: &AffineExpr) -> SCEVId {
    match affine_expr {
      AffineExpr::L(linear_expr) => {
        let mut scev_id = self.arena.zero;
        for (op, coeff) in &linear_expr.terms {
          let op_scev = self.trace_op(*op);
          let coeff_scev = self.arena.const_from(*coeff);
          let mul_expr = self.arena.mul([coeff_scev, op_scev].into_iter());
          scev_id = self.arena.add([scev_id, mul_expr].into_iter());
        }
        if linear_expr.constant != 0 {
          let const_scev = self.arena.const_from(linear_expr.constant);
          scev_id = self.arena.add([scev_id, const_scev].into_iter());
        }
        scev_id
      }
      AffineExpr::Unknown(op) => self.arena.dedup(SCEVExpr::Unknown(*op)),
    }
  }

  pub fn get_affine_scev(&mut self, affine_expr: &AffineExpr) -> SCEVId {
    self.affine_to_scev_expr(affine_expr)
  }

  pub fn get_addr_offset_scev(&mut self, operand: Operand) -> SCEVId {
    let MemLoc { offset, .. } = self.cx.compute_mem_loc(operand);
    self.affine_to_scev_expr(&offset)
  }

  pub fn get_op_scev(&mut self, op: Operand) -> SCEVId {
    self.trace_op(op)
  }
}

impl<'a> Analysis for SCEV<'a> {
  type Input = (
    *mut PassContext<'a>,
    &'a Loops,
    &'a [Option<LoopId>],
    &'a DomTree,
  );
  type Output = ();

  fn name() -> &'static str {
    "SCEV"
  }

  fn new((cx, loops, block_to_loop, dom_tree): Self::Input) -> Self {
    Self {
      cx: unsafe { &mut *cx },
      loops,
      block_to_loop,
      dom_tree,
      arena: SCEVArena::default(),
    }
  }

  fn run(&mut self) -> Self::Output { /*SCEV is based on query*/ }
}
