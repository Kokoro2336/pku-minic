//! SSA Updater. Run for a single def each time, refining SSA after new definitions are added.

use crate::analysis::{DomFrontier, DomTree};
use crate::base::Type;
use crate::ir::mid::{
  Builder, BuilderGuard, Function, Op, OpData, OpType, Operand, PhiIncoming, IR,
};
use crate::utils::set::BitSet;
use crate::utils::worklist::Worklist;

use rustc_hash::FxHashMap;

pub struct SSAUpdater<'a> {
  // All of the following fields should be provided by the caller.
  ir: &'a mut IR,
  func_id: Operand,
  inst_id: Operand,
  builder: Builder,

  dom_tree: &'a DomTree,
  dom_frontier: &'a DomFrontier,

  worklist: Worklist<Operand, BitSet>,
  inserted_blocks: BitSet,
  /// BBId -> OpId
  available_defs: FxHashMap<Operand, Operand>,
  new_phis: BitSet,
}

impl<'a> SSAUpdater<'a> {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    ir: &'a mut IR,
    func_id: Operand,
    inst_id: Operand,
    dom_tree: &'a DomTree,
    dom_frontier: &'a DomFrontier,
    worklist: Worklist<Operand, BitSet>,
    inserted_blocks: BitSet,
    available_defs: FxHashMap<Operand, Operand>,
  ) -> Self {
    Self {
      ir,
      func_id,
      inst_id,
      builder: Builder::default(),
      dom_tree,
      dom_frontier,
      worklist,
      inserted_blocks,
      available_defs,
      new_phis: BitSet::new(),
    }
  }

  #[inline(always)]
  fn init(&mut self) {
    self.builder.set_current_func(Some(self.func_id));
    self.new_phis.clear();
  }

  #[inline(always)]
  fn func(&self) -> &Function {
    &self.ir.funcs[self.func_id]
  }

  #[inline(always)]
  fn func_mut(&mut self) -> &mut Function {
    &mut self.ir.funcs[self.func_id]
  }

  #[inline(always)]
  fn get_op_type(&self) -> Type {
    self.ir.funcs[self.func_id].dfg[self.inst_id].typ.clone()
  }

  #[inline(always)]
  fn slay_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand) {
    let func_id = self.builder.current_function;
    self.ir.slay_phi_incoming(func_id, phi_id, bb_id);
  }

  #[inline(always)]
  fn append_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand, value: Operand) {
    let func_id = self.builder.current_function;
    self.ir.append_phi_incoming(func_id, phi_id, value, bb_id);
  }

  #[inline(always)]
  fn get_src_tuple(&mut self, op_id: Operand) -> Vec<(Operand, usize)> {
    let func_id = self.builder.current_function;
    self
      .ir
      .get_src_tuple(func_id, op_id)
      .iter()
      .map(|(src_op_id, idx)| (**src_op_id, *idx))
      .collect()
  }

  fn insert_phi_nodes(&mut self) {
    let typ = self.get_op_type();

    while let Some(bb_id) = self.worklist.pop_front() {
      let frontiers = &self.dom_frontier[bb_id.get_bb_id()];
      for &frontier_bb_id in frontiers {
        if self.inserted_blocks.contains(frontier_bb_id) {
          continue;
        }
        self.inserted_blocks.insert(frontier_bb_id);

        let frontier_bb_id = Operand::BB(frontier_bb_id);
        {
          let preds = self.func().cfg[frontier_bb_id]
            .preds
            .iter()
            .map(|(pred, _)| *pred)
            .collect::<Vec<_>>();

          let mut guard = BuilderGuard::new(&mut self.builder);
          let new_phi_id = {
            guard.set_current_func(Some(self.func_id));
            guard.set_current_block(frontier_bb_id);
            // Create empty phi node.
            guard.create_at_head(
              self.ir,
              Some(self.func_id),
              Op::new(typ.clone(), vec![], OpData::phi_with_undef(&preds)),
            )
          };

          // Update the available definition for the frontier block.
          self.available_defs.insert(frontier_bb_id, new_phi_id);
          self.new_phis.insert(new_phi_id.get_op_id());
        }

        // Add the frontier block to the worklist if it's not already available.
        self.worklist.push_back(frontier_bb_id);
      }
    }
  }

  fn trace_latest_def(&self, bb_id: Operand) -> Operand {
    let mut current_bb_id = Some(bb_id);

    while let Some(bb_id) = current_bb_id {
      if let Some(def) = self.available_defs.get(&bb_id) {
        return *def;
      }
      // Move up the dominator tree.
      current_bb_id = self.dom_tree.get_idom(bb_id.get_bb_id()).map(Operand::BB);
    }

    Operand::Undefined
  }

  fn get_trace_bb_id(&self, op_id: Operand) -> Operand {
    let user_op = &self.func().dfg[op_id];
    let OpData::Phi { incomings } = user_op.data.clone() else {
      // For non-phi users, the trace block is simply the block containing the user.
      return self.func().op_to_bb[op_id];
    };

    // Find the incoming edge corresponding to the current instruction.
    incomings
      .into_iter()
      .find_map(|incoming| {
        let PhiIncoming::Data { value, bb } = incoming else {
          return None;
        };
        (value == self.inst_id).then_some(bb)
      })
      .unwrap()
  }

  fn update_original_users(&mut self) {
    let inst_id = self.inst_id;
    let users = self.ir.users(Some(self.func_id), inst_id);
    for (user, idx) in users {
      let trace_bb_id = self.get_trace_bb_id(user);
      if self.use_available_on_edge(user, inst_id, trace_bb_id) {
        continue;
      }
      // Find the latest definition for the trace block.
      let latest_def = self.trace_latest_def(trace_bb_id);

      // Replace the operand in the user with the latest definition.
      let src_tuple = self.get_src_tuple(user);
      for (src_op_id, src_idx) in src_tuple {
        if src_op_id == inst_id && src_idx == idx {
          self
            .ir
            .replace_use(Some(self.func_id), (user, idx), src_op_id, latest_def);
        }
      }
    }
  }

  /// For each phi, iterate over its incoming blocks,
  /// try to find the latest definition for each incoming block and update the incoming value.
  fn update_new_phis(&mut self) {
    for phi_op_id in std::mem::take(&mut self.new_phis).iter() {
      let phi_op_id = Operand::Value(phi_op_id);
      let phi_op_data = self.func_mut().dfg[phi_op_id].data.clone();
      let OpData::Phi { incomings } = phi_op_data else {
        unreachable!()
      };
      for incoming in incomings {
        let PhiIncoming::Data { bb, value } = incoming else {
          unreachable!()
        };
        if self.use_available_on_edge(phi_op_id, value, bb) {
          continue;
        }
        let latest_def = self.trace_latest_def(bb);
        self.slay_phi_incoming(phi_op_id, bb);
        self.append_phi_incoming(phi_op_id, bb, latest_def);
      }
    }
  }

  fn use_available_on_edge(
    &self,
    user_id: Operand,
    used_id: Operand,
    incoming_bb: Operand,
  ) -> bool {
    if !matches!(user_id, Operand::Value(_)) || !matches!(used_id, Operand::Value(_)) {
      return false;
    }

    let user = &self.func().dfg[user_id];
    if !user.is(OpType::Phi) {
      return false;
    }
    let used = &self.func().dfg[used_id];
    let OpData::Phi { incomings } = used.data.clone() else {
      unreachable!()
    };
    incomings.into_iter().any(|incoming| {
      let PhiIncoming::Data { bb, .. } = incoming else {
        return false;
      };
      bb == incoming_bb
    })
  }

  /// TODO: When CFG is changed, SSAUpdater should be able to slay the dead edge.
  pub fn run(&mut self) {
    self.init();
    // Supply phi nodes at the dominance frontier of the new definition.
    self.insert_phi_nodes();
    // Update all normal users to use the latest definition.
    self.update_original_users();
    // Update the new phi nodes to use the latest definitions for their incoming edges.
    self.update_new_phis();
  }
}

pub fn ssa_updater_params(
  worklist_bbs: Vec<Operand>,
  inserted_bbs: Vec<Operand>,
  available_defs: Vec<(Operand, Operand)>,
) -> (
  Worklist<Operand, BitSet>,
  BitSet,
  FxHashMap<Operand, Operand>,
) {
  let mut worklist = Worklist::new();
  let mut inserted_blocks = BitSet::new();
  let mut available_defs_map = FxHashMap::default();

  for bb in worklist_bbs {
    worklist.push_back(bb);
  }
  for bb in inserted_bbs {
    inserted_blocks.insert(bb.get_bb_id());
  }
  for (bb, def) in available_defs {
    available_defs_map.insert(bb, def);
  }

  (worklist, inserted_blocks, available_defs_map)
}
