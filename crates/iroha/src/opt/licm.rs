//! Loop Invariant Code Motion (LICM).

use crate::analysis::{
  alias, CallGraphAnalysis, DomAnalysis, DomTree, LoopAnalysis, PurenessAnalysis, SCCAnalysis,
};

use yachiyo::analysis::{
  analyze, AliasResult, CallGraph, LoopData, LoopId, Loops, MemLoc, Pureness, PurenessResult,
};
use yachiyo::ir::mid::{OpData, OpType, Operand, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::BitSet;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LICM<'a> {
  cx: PassContext<'a>,
  /// LoopId -> OpId -> whether the value produced by the op is an invariant.
  invariants: Vec<BitSet>,
}

impl LICM<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand, loop_num: usize) {
    self.cx.set_current_func(Some(func_id));

    self.invariants.clear();
    self.invariants.resize(loop_num, BitSet::new());
  }

  #[inline(always)]
  fn unhoistable(&self, inst_id: Operand, pureness: &PurenessResult) -> bool {
    if let OpData::Call { func, .. } = self.cx.get_op_data(inst_id) {
      // TODO: We can analyze ReadOnly function in the future.
      return pureness[*func] != Pureness::Pure;
    }

    let op_typ = OpType::from(self.cx.get_op_data(inst_id));
    matches!(
      op_typ,
      OpType::GlobalAlloca
        | OpType::Declare
        | OpType::Phi
        | OpType::Br
        | OpType::Jump
        | OpType::Ret
        // TODO: Hoist Store.
        | OpType::Store
    )
  }

  fn is_invariant(
    &self,
    block_to_loop: &[Option<LoopId>],
    lp_id: LoopId,
    lp_data: &LoopData,
    value: Operand,
  ) -> bool {
    match value {
      Operand::Int(_)
      | Operand::Float(_)
      | Operand::Bool(_)
      | Operand::Param(_)
      | Operand::Global(_) => return true,
      // Actually, BB is also invariant too. But we won't hoist an op with such operand, so we return false.
      Operand::Undefined => return false,
      Operand::Value(_) => {}
      Operand::BB(_) | Operand::Func(_) => unreachable!(),
    }

    let value_bb_id = self.cx.op_bb(value).get_bb_id();
    self.invariants[usize::from(lp_id)].contains(value.get_op_id())
      || block_to_loop[value_bb_id].is_none()
      || !lp_data.blocks.contains(value_bb_id)
  }

  /// Only for address.
  /// For Load:
  /// - The MemLoc's base and offset should also be invariant.
  /// - The MemLoc should not alias with any store in the loop.
  fn is_mem_loc_invariant(
    &mut self,
    block_to_loop: &[Option<LoopId>],
    call_graph: &CallGraph,
    lp_id: LoopId,
    lp_data: &LoopData,
    addr: Operand,
  ) -> bool {
    if !self.is_invariant(block_to_loop, lp_id, lp_data, addr) {
      return false;
    }

    let mem_loc = self.cx.compute_mem_loc(addr);

    self.mem_loc_operands_invariant(block_to_loop, lp_id, lp_data, &mem_loc)
      && !self.loop_may_clobber_mem_loc(call_graph, lp_data, addr)
  }

  fn mem_loc_operands_invariant(
    &self,
    block_to_loop: &[Option<LoopId>],
    lp_id: LoopId,
    lp_data: &LoopData,
    mem_loc: &MemLoc,
  ) -> bool {
    let Some(offset_keys) = mem_loc.offset.get_keys() else {
      return false;
    };

    std::iter::once(mem_loc.base)
      .chain(offset_keys.cloned())
      .all(|operand| self.is_invariant(block_to_loop, lp_id, lp_data, operand))
  }

  fn loop_may_clobber_mem_loc(
    &mut self,
    call_graph: &CallGraph,
    lp_data: &LoopData,
    addr: Operand,
  ) -> bool {
    for bb_id in lp_data.blocks.iter() {
      let bb_id = Operand::BB(bb_id);
      let cur = self.cx.get_bb(bb_id).cur.clone();

      for inst_id in cur {
        if self.inst_may_clobber_mem_loc(call_graph, inst_id, addr) {
          return true;
        }
      }
    }

    false
  }

  fn inst_may_clobber_mem_loc(
    &mut self,
    call_graph: &CallGraph,
    inst_id: Operand,
    addr: Operand,
  ) -> bool {
    let op_data = self.cx.get_op(inst_id).data.clone();

    match op_data {
      OpData::Store {
        addr: store_addr, ..
      } => alias(&mut self.cx, addr, store_addr, call_graph) != AliasResult::NoAlias,

      _ => matches!(OpType::from(&op_data), OpType::Call),
    }
  }

  #[inline(always)]
  fn meet(
    &self,
    block_to_loop: &[Option<LoopId>],
    lp_id: LoopId,
    lp_data: &LoopData,
    operands: &[Operand],
  ) -> bool {
    operands
      .iter()
      .all(|&operand| self.is_invariant(block_to_loop, lp_id, lp_data, operand))
  }

  fn run(
    &mut self,
    loops: &Loops,
    block_to_loop: &[Option<LoopId>],
    dom_tree: &DomTree,
    call_graph: &CallGraph,
    pureness: &PurenessResult,
  ) {
    let func_id = self.cx.get_current_func_id();
    // The loops are naturally in RPO order, so the traverse it in a reverse order.
    let dpo = self.cx.get_func(func_id).cfg.dpo();
    for lp_id in (0..loops.len()).rev() {
      let loop_data = &loops[lp_id.into()];
      // Traverse the blocks in the loop in RPO order.
      for bb_id in dpo.iter().rev() {
        let bb_lp_id_option = block_to_loop[bb_id.get_bb_id()];
        // Filter out those blocks that are not in the loop.
        if bb_lp_id_option.is_none() || !loops.include(lp_id.into(), bb_lp_id_option.unwrap()) {
          continue;
        }

        let cur = self.cx.get_bb(*bb_id).cur.clone();
        // An invariant can be hoisted multiple times through different loops.
        for inst_id in cur {
          if self.unhoistable(inst_id, pureness) {
            continue;
          }

          let src = self.cx.get_src_owned(inst_id);
          let is_invariant = if let OpData::Load { addr, .. } = self.cx.get_op_data(inst_id) {
            // Check Load individually.
            self.is_invariant(block_to_loop, lp_id.into(), loop_data, *addr)
              && self.is_mem_loc_invariant(
                block_to_loop,
                call_graph,
                lp_id.into(),
                loop_data,
                *addr,
              )
          } else {
            self.meet(block_to_loop, lp_id.into(), loop_data, &src)
          };

          if is_invariant {
            // Mark the op as an invariant.
            self.invariants[lp_id].insert(inst_id.get_op_id());
            // Move the op to the pre-header block.
            let header_id = loop_data.header;
            let pre_header_id = self
              .cx
              .get_pre_header_id(header_id, dom_tree)
              .expect("LICM expects loop-simplified pre-header");
            let pre_header_term = *self.cx.get_bb(pre_header_id).cur.last().unwrap();

            self
              .cx
              .move_op_to_bb_at(inst_id, *bb_id, pre_header_id, Some(pre_header_term));
            // We dont' need to add inst_id to the outer scope's invariants,
            // since the pass is run with a strict inner-to-outer order.
            // So just leave it as it is!
          }
        }
      }
    }
  }
}

impl<'a> Pass<'a> for LICM<'a> {
  fn name(&self) -> &'static str {
    "LICM"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    let call_graph = &*self.cx.analyze::<CallGraphAnalysis>(self.cx.ir());
    let sccs = &*self.cx.analyze::<SCCAnalysis>(call_graph);
    let cx_ptr = &mut self.cx as *mut PassContext<'_>;
    let pureness = analyze::<PurenessAnalysis>((unsafe { &mut *cx_ptr }, call_graph, sccs));

    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.cx.set_current_func(Some(func_id));
      let graph = self.cx.extract_cfg();
      let (loops_data, block_to_loop) = &*self.cx.analyze::<LoopAnalysis>(&graph);
      let (dom_tree, _) = &*self.cx.analyze::<DomAnalysis>(&graph);
      self.init(func_id, loops_data.len());
      self.run(loops_data, block_to_loop, dom_tree, call_graph, &pureness);
    }
  }
}
