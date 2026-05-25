//! Loop Strength Reduction (LSR).

use yachiyo::analysis::{analyze, Analysis, LoopId, SCEVExpr, SCEVId};
use yachiyo::base::Type;
use yachiyo::ir::mid::{Attr, Op, OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};

use crate::analysis::{DomAnalysis, LoopAnalysis, SCEV};

use rustc_hash::FxHashMap;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LSR<'a> {
  cx: PassContext<'a>,
  /// Old Address Id -> Materialized Address Id(The phi)
  addr_cache: FxHashMap<AddrCacheKey, Operand>,
}

#[derive(PartialEq, Eq, Hash, Clone)]
struct AddrCacheKey {
  loop_id: LoopId,
  base: Operand,
  start: SCEVId,
  step: SCEVId,
  ty: Type,
}

impl LSR<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.addr_cache.clear();
  }

  fn is_valid_at(&self, scev: &SCEV, op: Operand, pre_header_id: Operand) -> bool {
    match op {
      Operand::Int(_)
      | Operand::Global(_)
      | Operand::Param(_)
      | Operand::Bool(_)
      | Operand::Float(_)
      | Operand::Undefined => true,
      Operand::BB(_) | Operand::Func(_) => false,
      Operand::Value(_) => {
        let op_bb_id = self.cx.op_bb(op);
        scev
          .dom_tree
          .is_dom(op_bb_id.get_bb_id(), pre_header_id.get_bb_id())
      }
    }
  }

  fn is_materializable(
    &mut self,
    scev_id: SCEVId,
    scev: &SCEV,
    pre_header_id: Operand,
    loop_id: LoopId,
  ) -> bool {
    match scev[scev_id].clone() {
      SCEVExpr::Const(_) => true,
      SCEVExpr::Unknown(op) => self.is_valid_at(scev, op, pre_header_id),
      SCEVExpr::Add(ops) | SCEVExpr::Mul(ops) => ops
        .iter()
        .all(|&expr_id| self.is_materializable(expr_id, scev, pre_header_id, loop_id)),
      SCEVExpr::AddRec {
        loop_id: lp_id, iv, ..
      } => {
        lp_id != loop_id
          && scev.loops.is_ancestor(lp_id, loop_id)
          && self.is_valid_at(scev, iv, pre_header_id)
      }
    }
  }

  fn materialize(
    &mut self,
    scev_id: SCEVId,
    scev: &SCEV,
    pre_header_id: Operand,
  ) -> Option<Operand> {
    match scev[scev_id].clone() {
      SCEVExpr::Const(c) => Some(Operand::Int(c as i32)),
      SCEVExpr::Unknown(op) => Some(op),
      SCEVExpr::Add(terms) => {
        let mut terms = terms.into_iter();
        let first_term = terms.next()?;
        let mut result = self.materialize(first_term, scev, pre_header_id)?;

        for term in terms {
          let operand = self.materialize(term, scev, pre_header_id)?;
          result = {
            let mut guard = self.cx.guard();
            guard.set_current_block(pre_header_id);
            guard.create_before_term(Op::new(
              Type::Int,
              vec![],
              OpData::AddI {
                lhs: result,
                rhs: operand,
              },
            ))
          }
        }
        Some(result)
      }
      SCEVExpr::Mul(factors) => {
        let mut factors = factors.into_iter();
        let first_factor = factors.next()?;
        let mut result = self.materialize(first_factor, scev, pre_header_id)?;

        for factor in factors {
          let operand = self.materialize(factor, scev, pre_header_id)?;
          result = {
            let mut guard = self.cx.guard();
            guard.set_current_block(pre_header_id);
            guard.create_before_term(Op::new(
              Type::Int,
              vec![],
              OpData::MulI {
                lhs: result,
                rhs: operand,
              },
            ))
          }
        }
        Some(result)
      }
      // This AddRec is not the AddRec of current loop. Since it's accessible, we reuse it directly without materialization.
      SCEVExpr::AddRec { iv, .. } => Some(iv),
    }
  }

  fn run(&mut self, scev: &mut SCEV) {
    for loop_id in (0..scev.loops.len()).rev() {
      let loop_id: LoopId = loop_id.into();
      let to_visit = scev.loops[loop_id].owned_blocks.clone();

      for bb_id in to_visit.iter() {
        let bb_id = Operand::BB(bb_id);

        for inst_id in self.cx.get_bb(bb_id).cur.clone() {
          let op_data = self.cx.get_op(inst_id).data.clone();
          let addr = if let OpData::Load { addr } = op_data {
            Some(addr)
          } else if let OpData::Store { addr, .. } = op_data {
            Some(addr)
          } else {
            None
          };

          let Some(addr) = addr else {
            continue;
          };
          // Create GEP instruction in pre_header to compute the address
          let addr_ty = self.cx.get_op_type(addr);

          let mem_loc = self.cx.compute_mem_loc(addr);
          let header_id = scev.loops[loop_id].header;
          let Some(pre_header_id) = self.cx.get_pre_header_id(header_id, &scev.dom_tree) else {
            unreachable!()
          };

          // Only addr whose base is outside current loop could be rewritten by LSR.
          if !self.is_valid_at(scev, mem_loc.base, pre_header_id) {
            continue;
          }

          let scev_id = scev.get_affine_scev(&mem_loc.offset);
          let Some(ar) = scev.get_add_rec_for_loop(scev_id, loop_id) else {
            continue;
          };

          if !scev.is_scev_loop_invariant(ar.start, loop_id)
            || !scev.is_scev_loop_invariant(ar.step, loop_id)
          {
            continue;
          };

          // Materialize start & step at the end of pre_header
          if !self.is_materializable(ar.start, scev, pre_header_id, loop_id)
            || !self.is_materializable(ar.step, scev, pre_header_id, loop_id)
          {
            continue;
          };

          let addr_cache_key = AddrCacheKey {
            loop_id,
            base: mem_loc.base,
            start: ar.start,
            step: ar.step,
            ty: addr_ty.clone(),
          };

          // Check the cache
          if let Some(addr_phi_id) = self.addr_cache.get(&addr_cache_key) {
            self.cx.replace_all_uses(addr, *addr_phi_id);
            continue;
          }

          let (Some(start_op_id), Some(step_op_id)) = (
            self.materialize(ar.start, scev, pre_header_id),
            self.materialize(ar.step, scev, pre_header_id),
          ) else {
            unreachable!()
          };

          let new_addr = {
            let mut guard = self.cx.guard();
            guard.set_current_block(pre_header_id);
            guard.create_before_term(Op::new(
              addr_ty.clone(),
              vec![Attr::WeakType],
              OpData::GEP {
                base: mem_loc.base,
                // For now there's only one induction variable for one loop, so GEP only has one index.
                indices: vec![start_op_id],
              },
            ))
          };

          // Insert phi operation at header
          let addr_phi_id = {
            let mut guard = self.cx.guard();
            guard.set_current_block(header_id);
            guard.create_at_head(Op::new(
              addr_ty.clone(),
              vec![Attr::WeakType],
              OpData::Phi {
                incomings: vec![PhiIncoming::Data {
                  bb: pre_header_id,
                  value: new_addr,
                }],
              },
            ))
          };

          let Some(latch_id) = self.cx.get_latch_id(header_id, &scev.dom_tree) else {
            unreachable!()
          };

          // Insert addr increment in the latch
          let addr_inc_id = {
            let mut guard = self.cx.guard();
            guard.set_current_block(latch_id);
            guard.create_before_term(Op::new(
              addr_ty.clone(),
              vec![Attr::WeakType],
              OpData::GEP {
                base: addr_phi_id,
                indices: vec![step_op_id],
              },
            ))
          };

          // Fill the incoming value of phi in header from latch
          self
            .cx
            .append_phi_incoming(addr_phi_id, latch_id, addr_inc_id);

          // RAUW old address to new phi
          self.cx.replace_all_uses(addr, addr_phi_id);

          // Cache the IV
          self.addr_cache.insert(addr_cache_key, addr_phi_id);
        }
      }
    }
  }
}

impl<'a> Pass<'a> for LSR<'a> {
  fn name(&self) -> &'static str {
    "LSR"
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
      let mut scev = SCEV::new((&mut self.cx, loops, block_to_loop, dom_tree));
      self.run(&mut scev);
    }
  }
}
