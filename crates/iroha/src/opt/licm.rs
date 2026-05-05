//! Loop Invariant Code Motion (LICM).

use crate::analysis::{DomAnalysis, DomTree, LoopAnalysis, LoopId, Loops};

use yachiyo::analysis::analyze;
use yachiyo::ir::mid::{Builder, Function, OpType, Operand, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::set::BitSet;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LICM<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  /// LoopId -> OpId -> whether the value produced by the op is an invariant.
  invariants: Vec<BitSet>,
  /// OpId -> BBId
  op_to_bb: Vec<Operand>,
  block_to_loop: Vec<Option<LoopId>>,
}

impl LICM<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand, loop_num: usize, block_to_loop: Vec<Option<LoopId>>) {
    self.builder.set_current_func(Some(func_id));
    self.block_to_loop = block_to_loop;

    self.invariants.clear();
    self.invariants.resize(loop_num, BitSet::new());

    self.op_to_bb.clear();
    self
      .op_to_bb
      .resize(self.get_func(func_id).dfg.len(), Operand::Undefined);
    for bb_id in self.get_func(func_id).cfg.collect() {
      let bb_id = Operand::BB(bb_id);
      let cur = self.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in cur {
        self.op_to_bb[inst_id.get_op_id()] = bb_id;
      }
    }
  }

  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn move_op_to_bb_at(
    &mut self,
    op_id: Operand,
    from_bb: Operand,
    to_bb: Operand,
    before_op: Option<Operand>,
  ) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_deref_mut()
      .unwrap()
      .move_op_to_bb_at(func_id, op_id, from_bb, to_bb, before_op);
    // Remember to update the op_to_bb mapping after moving the op.
    self.op_to_bb[op_id.get_op_id()] = to_bb;
  }

  #[inline(always)]
  fn get_src(&self, op_id: Operand) -> Vec<Operand> {
    let func_id = self.builder.current_function.unwrap();
    self
      .get_func(func_id)
      .get_src(op_id)
      .iter()
      .map(|x| **x)
      .collect()
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
        // TODO: Hoist Load/Store after AA.
        | OpType::Load
        | OpType::Store
    )
  }

  fn is_invariant(&self, lp_id: LoopId, value: Operand) -> bool {
    match value {
      Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) | Operand::Param(_) => return true,
      // Actually, BB is also invariant too. But we won't hoist an op with such operand, so we return false.
      // TODO: Global requires AA to judge whether it's invariant.
      Operand::Global(_) | Operand::Undefined => return false,
      Operand::Value(_) => {}
      Operand::BB(_) | Operand::Func(_) => unreachable!(),
    }

    self.invariants[usize::from(lp_id)].contains(value.get_op_id())
      || self.block_to_loop[self.op_to_bb[value.get_op_id()].get_bb_id()].is_none()
      || self.block_to_loop[self.op_to_bb[value.get_op_id()].get_bb_id()].unwrap() != lp_id
  }

  #[inline(always)]
  fn meet(&self, lp_id: LoopId, operands: &[Operand]) -> bool {
    operands
      .iter()
      .all(|&operand| self.is_invariant(lp_id, operand))
  }

  fn run(&mut self, loops: &Loops, dom_tree: &DomTree) {
    let func_id = self.builder.current_function.unwrap();
    // The loops are naturally in RPO order, so the traverse it in a reverse order.
    let dpo = self.get_func(func_id).cfg.dpo();
    for lp_id in (0..loops.len()).rev() {
      let loop_data = &loops[lp_id.into()];
      // Traverse the blocks in the loop in RPO order.
      for bb_id in dpo.iter().rev() {
        let bb_lp_id_option = self.block_to_loop[bb_id.get_bb_id()];
        // Filter out those blocks that are not in the loop.
        if bb_lp_id_option.is_none() || !loops.include(lp_id.into(), bb_lp_id_option.unwrap()) {
          continue;
        }

        let cur = self.get_func(func_id).cfg[*bb_id].cur.clone();
        // An invariant can be hoisted multiple times through different loops.
        for inst_id in cur {
          let op_typ = OpType::from(&self.get_func(func_id).dfg[inst_id].data);
          if Self::unhoistable(op_typ) {
            continue;
          }

          let src = self.get_src(inst_id);
          if self.meet(lp_id.into(), &src) {
            // Mark the op as an invariant.
            self.invariants[lp_id].insert(inst_id.get_op_id());
            // Move the op to the pre-header block.
            let header_id = loop_data.header;
            let pre_header_id = self.get_func(func_id).cfg[header_id]
              .preds
              .iter()
              .filter(|(pred_id, _)| !dom_tree.is_dom(header_id.get_bb_id(), pred_id.get_bb_id()))
              .map(|(pred_id, _)| *pred_id)
              .collect::<Vec<_>>();

            // There should be only one pre-header block.
            assert!(pre_header_id.len() == 1);

            let pre_header_id = pre_header_id[0];
            let pre_header_term = *self.get_func(func_id).cfg[pre_header_id]
              .cur
              .last()
              .unwrap();

            self.move_op_to_bb_at(inst_id, *bb_id, pre_header_id, Some(pre_header_term));
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
    self.ir = Some(ir);
  }
  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let (loops_data, block_to_loop) = analyze::<LoopAnalysis>(self.get_func(func_id));
      let (dom_tree, _) = analyze::<DomAnalysis>(self.get_func(func_id));
      self.init(func_id, loops_data.len(), block_to_loop);
      self.run(&loops_data, &dom_tree);
    }
  }
}
