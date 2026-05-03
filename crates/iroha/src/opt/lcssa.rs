//! Loop-closed SSA (LCSSA) transformation.

use crate::analysis::{DomAnalysis, DomFrontier, DomTree, LoopAnalysis, Loops};

use yachiyo::analysis::analyze;
use yachiyo::base::Type;
use yachiyo::ir::mid::{Builder, Function, Op, OpData, Operand, SSAUpdater, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

use rustc_hash::FxHashMap;
use std::ops::BitAnd;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct LCSSA<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  processed_blocks: BitSet,
  op_to_bb: Vec<Operand>,
}

impl LCSSA<'_> {
  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
    self.processed_blocks.clear();

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
  fn create_at_head(&mut self, op: Op) -> Operand {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .create_at_head(&mut self.builder, func_id, op)
  }

  #[inline(always)]
  fn get_op_type(&self, inst_id: Operand) -> Type {
    let func_id = self.builder.current_function.unwrap();
    self.get_func(func_id).dfg[inst_id].typ.clone()
  }

  fn run(&mut self, dom_tree: &DomTree, dom_frontier: &DomFrontier, loops: &Loops) {
    let func_id = self.builder.current_function.unwrap();

    for lp_id in (0..loops.len()).rev() {
      let loop_data = &loops[lp_id.into()];
      let blocks_in_loop = loop_data.blocks.bitand(&!self.processed_blocks.clone());

      for bb_id in blocks_in_loop.iter() {
        let bb_id = Operand::BB(bb_id);
        let cur = self.get_func(func_id).cfg[bb_id].cur.clone();
        for inst_id in cur {
          let users = self.get_func(func_id).dfg[inst_id.get_op_id()]
            .users
            .iter()
            .map(|(user_id, _)| *user_id)
            .collect::<Vec<_>>();
          let typ = self.get_op_type(inst_id);

          if users.iter().any(|user_id| {
            let user_bb = self.op_to_bb[user_id.get_op_id()];
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

              let exit_bb_preds = self.get_func(func_id).cfg[exit_bb]
                .preds
                .iter()
                .map(|(pred_id, _)| *pred_id)
                .collect::<Vec<_>>();

              let new_phi_id = self.create_at_head(Op::new(
                typ.clone(),
                vec![],
                OpData::phi_with_undef(&exit_bb_preds),
              ));

              // Update the mapping from op to bb.
              if new_phi_id.get_op_id() >= self.op_to_bb.len() {
                self
                  .op_to_bb
                  .resize(new_phi_id.get_op_id() + 1, Operand::BB(0));
              }
              self.op_to_bb[new_phi_id.get_op_id()] = exit_bb;

              // Update structures to be passed to SSAUpdater.
              available_defs.insert(exit_bb, new_phi_id);
              inserted_blocks.insert(exit_bb.get_bb_id());
              worklist.push_back(exit_bb);
            }

            // Start SSAUpdater
            let mut ssa_updater = SSAUpdater::new(
              self.ir.as_mut().unwrap(),
              func_id,
              inst_id,
              dom_tree,
              dom_frontier,
              worklist,
              inserted_blocks,
              available_defs,
              &mut self.op_to_bb,
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
    self.ir = Some(ir);
  }

  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect() {
      let func_id = Operand::Func(func_id);
      self.init(func_id);
      let func = self.get_func(func_id);
      let (dom_tree, dom_frontier) = analyze::<DomAnalysis>(func);
      let (loops, _) = analyze::<LoopAnalysis>(func);
      self.run(&dom_tree, &dom_frontier, &loops);
    }
  }
}
