//! Loop Invariant Code Motion (LICM).

use crate::analysis::{alias, DomAnalysis, DomTree, LoopAnalysis, LoopData, LoopId, Loops};

use yachiyo::analysis::{analyze, AliasResult};
use yachiyo::ir::mid::{OpData, OpType, Operand, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::set::BitSet;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LICM<'a> {
  cx: PassContext<'a>,
  /// LoopId -> OpId -> whether the value produced by the op is an invariant.
  invariants: Vec<BitSet>,
  block_to_loop: Vec<Option<LoopId>>,
}

impl LICM<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand, loop_num: usize, block_to_loop: Vec<Option<LoopId>>) {
    self.cx.set_current_func(Some(func_id));
    self.block_to_loop = block_to_loop;

    self.invariants.clear();
    self.invariants.resize(loop_num, BitSet::new());
  }

  #[inline(always)]
  fn unhoistable(op_typ: OpType) -> bool {
    matches!(
      op_typ,
      // TODO: Pureness analysis. Call is potential to be hoisted.
      OpType::Call
        | OpType::GlobalAlloca
        | OpType::Declare
        | OpType::Phi
        | OpType::Br
        | OpType::Jump
        | OpType::Ret
        // TODO: Hoist Store.
        | OpType::Store
    )
  }

  fn is_invariant(&self, lp_id: LoopId, lp_data: &LoopData, value: Operand) -> bool {
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
      || self.block_to_loop[value_bb_id].is_none()
      || !lp_data.blocks.contains(value_bb_id)
  }

  /// Only for address.
  /// For Load:
  /// - The MemLoc's base and offset should also be invariant.
  /// - The MemLoc should not alias with any store in the loop.
  fn is_mem_loc_invariant(&self, lp_id: LoopId, lp_data: &LoopData, value: Operand) -> bool {
    let mem_loc = self.cx.compute_mem_loc(value);
    let Some(offset_keys) = mem_loc.offset.get_keys() else {
      // If the offset is not affine, we conservatively assume it's variant.
      return false;
    };
    let mut checklist = std::iter::once(mem_loc.base).chain(offset_keys.cloned());

    checklist.all(|operand| self.is_invariant(lp_id, lp_data, operand))
      && lp_data.blocks.iter().all(|bb_id| {
        let bb_id = Operand::BB(bb_id);
        self.cx.get_bb(bb_id).cur.iter().all(|&inst_id| {
          let op = self.cx.get_op(inst_id);
          if let OpData::Store { addr, .. } = &op.data {
            alias(&self.cx, value, *addr) == AliasResult::NoAlias
          } else {
            // TODO: for now, we conservatively assume that any call in the loop will clobber the memory.
            !matches!(OpType::from(&op.data), OpType::Call)
          }
        })
      })
  }

  #[inline(always)]
  fn meet(&self, lp_id: LoopId, lp_data: &LoopData, operands: &[Operand]) -> bool {
    operands
      .iter()
      .all(|&operand| self.is_invariant(lp_id, lp_data, operand))
  }

  fn run(&mut self, loops: &Loops, dom_tree: &DomTree) {
    let func_id = self.cx.current_func();
    // The loops are naturally in RPO order, so the traverse it in a reverse order.
    let dpo = self.cx.get_func(func_id).cfg.dpo();
    for lp_id in (0..loops.len()).rev() {
      let loop_data = &loops[lp_id.into()];
      // Traverse the blocks in the loop in RPO order.
      for bb_id in dpo.iter().rev() {
        let bb_lp_id_option = self.block_to_loop[bb_id.get_bb_id()];
        // Filter out those blocks that are not in the loop.
        if bb_lp_id_option.is_none() || !loops.include(lp_id.into(), bb_lp_id_option.unwrap()) {
          continue;
        }

        let cur = self.cx.get_func(func_id).cfg[*bb_id].cur.clone();
        // An invariant can be hoisted multiple times through different loops.
        for inst_id in cur {
          let op_typ = OpType::from(&self.cx.get_func(func_id).dfg[inst_id].data);
          if Self::unhoistable(op_typ) {
            continue;
          }

          let src = self.cx.get_src_owned(inst_id);
          let is_invariant = if let OpData::Load { addr, .. } = &self.cx.get_op(inst_id).data {
            // Check Load individually.
            self.is_invariant(lp_id.into(), loop_data, *addr)
              && self.is_mem_loc_invariant(lp_id.into(), loop_data, *addr)
          } else {
            self.meet(lp_id.into(), loop_data, &src)
          };

          if is_invariant {
            // Mark the op as an invariant.
            self.invariants[lp_id].insert(inst_id.get_op_id());
            // Move the op to the pre-header block.
            let header_id = loop_data.header;
            let pre_header_id = self.cx.get_func(func_id).cfg[header_id]
              .preds
              .iter()
              .filter(|(pred_id, _)| !dom_tree.is_dom(header_id.get_bb_id(), pred_id.get_bb_id()))
              .map(|(pred_id, _)| *pred_id)
              .collect::<Vec<_>>();

            // There should be only one pre-header block.
            assert!(pre_header_id.len() == 1);

            let pre_header_id = pre_header_id[0];
            let pre_header_term = *self.cx.get_func(func_id).cfg[pre_header_id]
              .cur
              .last()
              .unwrap();

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
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let (loops_data, block_to_loop) = analyze::<LoopAnalysis>(self.cx.get_func(func_id));
      let (dom_tree, _) = analyze::<DomAnalysis>(self.cx.get_func(func_id));
      self.init(func_id, loops_data.len(), block_to_loop);
      self.run(&loops_data, &dom_tree);
    }
  }
}
