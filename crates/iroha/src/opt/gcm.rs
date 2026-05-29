//! Global Code Motion (GCM).
//! Hoist computations to shallower loops and sink them closer to their uses.

use crate::analysis::{DomAnalysis, LoopAnalysis};

use yachiyo::analysis::{analyze, DomTree, LoopData, LoopId, INVALID_LOOP_LEVEL, MAX_LOOP_LEVEL};
use yachiyo::ir::mid::{OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::{BitSet, Worklist};

use rustc_hash::FxHashMap;

#[derive(Default)]
#[allow(clippy::upper_case_acronyms)]
pub struct GCM<'a> {
  cx: PassContext<'a>,
  earliest: Vec<Operand>,
  latest: Vec<Option<Operand>>,
  /// BBId -> OpId that is to be moved to the BBId.
  position: Vec<Vec<Operand>>,
}

impl GCM<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.earliest.clear();
    self.latest.clear();
    self.position.clear();

    let entry = self.cx.get_entry(func_id);
    self.earliest.resize(self.cx.get_dfg().len(), entry);
    self.latest.resize(self.cx.get_dfg().len(), None);
    self.position.resize(self.cx.get_cfg().len(), Vec::new());
  }

  fn unmovable(&self, op_id: Operand) -> bool {
    matches!(
      self.cx.get_op(op_id).data,
      OpData::Call { .. }
        | OpData::Ret { .. }
        | OpData::Br { .. }
        | OpData::Jump { .. }
        | OpData::Phi { .. }
        | OpData::Load { .. }
        | OpData::Store { .. }
        | OpData::Alloca(_)
    )
  }

  fn is_dead(&self, op_id: Operand) -> bool {
    self.cx.get_op(op_id).users.is_empty()
  }

  fn earliest_rec(&mut self, op_id: Operand, dom_tree: &DomTree) -> Operand {
    if self.unmovable(op_id) || self.is_dead(op_id) {
      // For unmovable or dead instructions, we cannot move them, so their earliest position is fixed.
      self.earliest[op_id.get_op_id()] = self.cx.op_bb(op_id);
      return self.earliest[op_id.get_op_id()];
    }

    let src = self
      .cx
      .get_src(op_id)
      .iter()
      .map(|src| **src)
      .collect::<Vec<_>>();

    let entry = self.cx.get_entry(self.cx.get_current_func_id());
    let earliest = src
      .iter()
      .fold(self.earliest[op_id.get_op_id()], |cur, src| {
        let src_earliest = if matches!(src, Operand::Value(_)) {
          self.earliest_rec(*src, dom_tree)
        } else {
          // entry's depth is the smallest
          entry
        };
        let cur_depth = dom_tree.get_depth(cur.get_bb_id());
        let src_depth = dom_tree.get_depth(src_earliest.get_bb_id());
        if src_depth < cur_depth {
          cur
        } else {
          src_earliest
        }
      });
    self.earliest[op_id.get_op_id()] = earliest;
    earliest
  }

  fn compute_earliest(&mut self, dom_tree: &DomTree) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in self.cx.bbs(func_id) {
      for inst_id in self.cx.get_bb(bb_id).cur.clone().into_iter().rev() {
        self.earliest_rec(inst_id, dom_tree);
      }
    }
  }

  fn latest_rec(&mut self, op_id: Operand, dom_tree: &DomTree) -> Option<Operand> {
    if self.unmovable(op_id) || self.is_dead(op_id) {
      // For unmovable or dead instructions, we cannot move them, so their latest position is fixed.
      self.latest[op_id.get_op_id()] = Some(self.cx.op_bb(op_id));
      return self.latest[op_id.get_op_id()];
    }

    let users = self.cx.get_op(op_id).users.clone();

    let latest = users
      .iter()
      .fold(self.latest[op_id.get_op_id()], |cur, (user, idx)| {
        let user_latest = if matches!(user, Operand::Value(_)) {
          let latest = self.latest_rec(*user, dom_tree);
          if let OpData::Phi { incomings } = &self.cx.get_op(*user).data {
            // For phi users, we need to consider the position of the incoming block.
            let PhiIncoming::Data { bb, .. } = incomings[*idx] else {
              unreachable!()
            };
            Some(bb)
          } else {
            latest
          }
        } else {
          None
        };
        if let Some(cur_latest) = cur {
          if let Some(user_latest) = user_latest {
            Some(
              dom_tree
                .lca(cur_latest.get_bb_id(), user_latest.get_bb_id())
                .map(Operand::BB)
                .unwrap_or_else(|| self.cx.op_bb(op_id)),
            )
          } else {
            cur
          }
        } else {
          user_latest
        }
      });
    self.latest[op_id.get_op_id()] = latest;
    latest
  }

  fn compute_latest(&mut self, dom_tree: &DomTree) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in self.cx.bbs(func_id) {
      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        self.latest_rec(inst_id, dom_tree);
      }
    }
  }

  fn compute_position(
    &mut self,
    dom_tree: &DomTree,
    loops: &[LoopData],
    block_to_loop: &[Option<LoopId>],
  ) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in self.cx.bbs(func_id) {
      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        let earliest = self.earliest[inst_id.get_op_id()];
        let latest = self.latest[inst_id.get_op_id()];
        let Some(latest) = latest else {
          // If latest is None, it means the instruction can't be moved, so we keep it in its original position as None.
          continue;
        };
        // Find out blocks with smallest loop level on the path.
        let path = dom_tree.get_path(earliest.get_bb_id(), latest.get_bb_id());
        let mut candidate = vec![];
        let mut smallest_loop_level = MAX_LOOP_LEVEL;

        for bb_id in path {
          let bb_loop_id = block_to_loop[bb_id];
          let loop_level = if let Some(LoopId(bb_loop_id)) = bb_loop_id {
            loops[bb_loop_id].level
          } else {
            INVALID_LOOP_LEVEL
          };

          if loop_level < smallest_loop_level {
            smallest_loop_level = loop_level;
            candidate.clear();
            candidate.push(bb_id);
          } else if loop_level == smallest_loop_level {
            candidate.push(bb_id);
          }
        }

        // If there are multiple candidates, we choose the one with deepest depth in the dominator tree.
        let Some(pos_bb_id) = candidate
          .into_iter()
          .max_by_key(|bb_id| dom_tree.get_depth(*bb_id))
          .map(Operand::BB)
        else {
          continue;
        };
        self.position[pos_bb_id.get_bb_id()].push(inst_id);
      }
    }
  }

  fn rewrite(&mut self) {
    for (bb_id, ops) in std::mem::take(&mut self.position).into_iter().enumerate() {
      let bb_id = Operand::BB(bb_id);

      // Sort the ops in topological order to ensure def-use correctness.
      let mut in_degree = FxHashMap::default();
      for op_id in ops.iter() {
        let users = self.cx.get_op(*op_id).users.iter().map(|(user, _)| *user);
        for user in users {
          *in_degree.entry(user).or_insert(0) += 1;
        }
      }
      let mut sorted_ops: Worklist<Operand, BitSet> = Worklist::new();
      let mut scheduled: bool;
      loop {
        scheduled = false;
        for op in ops.iter() {
          if sorted_ops.contains(op) {
            continue;
          }
          if in_degree.get(op).cloned().unwrap_or(0) == 0 {
            scheduled = true;
            sorted_ops.push_back(*op);
            for user in self.cx.get_op(*op).users.iter().map(|(user, _)| *user) {
              if let Some(count) = in_degree.get_mut(&user) {
                *count -= 1;
              }
            }
          }
        }
        if !scheduled {
          break;
        }
      }

      while let Some(op_id) = sorted_ops.pop_back() {
        if self.unmovable(op_id) || self.is_dead(op_id) {
          // We don't move unmovable or dead instructions, so we skip them.
          continue;
        }

        let cur = &self.cx.get_bb(bb_id).cur;
        let users = self
          .cx
          .get_op(op_id)
          .users
          .iter()
          .map(|(user, _)| *user)
          .collect::<Vec<_>>();
        let pos = cur.iter().find(|inst_id| {
          users.contains(*inst_id) 
          // In self-loop, an instruction and its user phi can be in the same block. We need to avoid this.
          && !self.cx.get_op_data(**inst_id).is(OpType::Phi)
        });

        // If there's a user of the op in target block, we insert the instruction right before terminator.
        // Else we insert it after phi.
        if let Some(&pos) = pos {
          self.cx.move_op_to_bb_at(op_id, bb_id, Some(pos));
        } else {
          let term_id = self.cx.get_term(bb_id);
          self.cx.move_op_to_bb_at(op_id, bb_id, Some(term_id));
        }
      }
    }
  }
}

impl<'a> Pass<'a> for GCM<'a> {
  fn name(&self) -> &str {
    "GCM"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      let (dom_tree, _) = analyze::<DomAnalysis>(&self.cx.extract_cfg());
      self.compute_earliest(&dom_tree);
      self.compute_latest(&dom_tree);
      let (loops, block_to_loop) = analyze::<LoopAnalysis>(&self.cx.extract_cfg());
      self.compute_position(&dom_tree, &loops, &block_to_loop);
      self.rewrite();
    }
  }
}
