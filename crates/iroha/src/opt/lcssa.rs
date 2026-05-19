//! Loop-closed SSA (LCSSA) Transformation.

use crate::analysis::{DomAnalysis, DomFrontier, DomTree, LoopAnalysis};

use yachiyo::analysis::Loops;
use yachiyo::ir::mid::{Op, OpData, Operand, SSAUpdater, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::BitSet;
use yachiyo::utils::Worklist;

use rustc_hash::FxHashMap;
use std::ops::BitAnd;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LCSSA<'a> {
  cx: PassContext<'a>,
  processed_blocks: BitSet,
}

impl LCSSA<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.processed_blocks.clear();
  }

  fn run(&mut self, dom_tree: &DomTree, dom_frontier: &DomFrontier, loops: &Loops) {
    let func_id = self.cx.get_current_func_id();

    for lp_id in (0..loops.len()).rev() {
      let loop_data = &loops[lp_id.into()];
      let blocks_in_loop = loop_data.blocks.bitand(&!self.processed_blocks.clone());

      for bb_id in blocks_in_loop.iter() {
        let bb_id = Operand::BB(bb_id);
        let cur = self.cx.get_bb(bb_id).cur.clone();
        for inst_id in cur {
          let users = self
            .cx
            .users(inst_id)
            .iter()
            .map(|(user_id, _)| *user_id)
            .collect::<Vec<_>>();
          let typ = self.cx.get_op_type(inst_id);

          if users.iter().any(|user_id| {
            let user_bb = self.cx.op_bb(*user_id);
            // Escaped
            !loop_data.blocks.contains(user_bb.get_bb_id())
          }) {
            let mut available_defs = FxHashMap::default();
            let mut inserted_blocks = BitSet::new();
            let mut worklist: Worklist<Operand, BitSet> = Worklist::new();

            available_defs.insert(bb_id, inst_id);
            inserted_blocks.insert(bb_id.get_bb_id());

            // Insert a empty phi at every exit block of the loop.
            for exit_bb in loop_data.exit_blocks.iter() {
              let exit_bb = Operand::BB(exit_bb);

              let exit_bb_preds = self
                .cx
                .get_bb(exit_bb)
                .preds
                .iter()
                .map(|(pred_id, _)| *pred_id)
                .collect::<Vec<_>>();

              let new_phi_id = self.cx.create_at_head(Op::new(
                typ.clone(),
                vec![],
                OpData::phi_with_undef(&exit_bb_preds),
              ));

              // Update structures to be passed to SSAUpdater.
              available_defs.insert(exit_bb, new_phi_id);
              inserted_blocks.insert(exit_bb.get_bb_id());
              worklist.push_back(exit_bb);
            }

            // Start SSAUpdater
            let mut ssa_updater = SSAUpdater::new(
              self.cx.ir_mut(),
              func_id,
              inst_id,
              dom_tree,
              dom_frontier,
              worklist,
              inserted_blocks,
              available_defs,
            );
            ssa_updater.run();
          }
        }
      }

      // Update the processed blocks.
      self.processed_blocks |= blocks_in_loop;
    }
  }
}

impl<'a> Pass<'a> for LCSSA<'a> {
  fn name(&self) -> &'static str {
    "LCSSA"
  }

  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      let func = self.cx.get_func(func_id);
      let (dom_tree, dom_frontier) = &*self.cx.analyze::<DomAnalysis>(func);
      let (loops, _) = &*self.cx.analyze::<LoopAnalysis>(func);
      self.run(dom_tree, dom_frontier, loops);
    }
  }
}
