//! Branch Folding, including:
//! - Trampoline Forwarding
//! - Adjacent Blocks Fallthrough

use yachiyo::ir::back::{BOpData, BOperand, BackIR, MOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::Arena;
use yachiyo::utils::BitSet;

#[derive(Default)]
pub struct BranchFolding<'a> {
  cx: BPassContext<'a>,
  trampoline: BitSet,
}

impl BranchFolding<'_> {
  fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
    self.trampoline.clear();
  }

  fn is_trampoline(&self, bb_id: BOperand) -> bool {
    let bb = self.cx.get_bb(bb_id);
    bb.preds.len() == 1
      && bb.succs.len() == 1
      && bb.preds[0].0 != bb_id
      && bb.succs[0].0 != bb_id
      && bb.cur.len() == 1
      && matches!(
        self.cx.get_op(bb.cur[0]).data.clone().into(),
        MOpData::J { .. }
      )
  }

  fn trampoline_forward(&mut self) {
    let func_id = self.cx.get_current_func_id();

    for bb_id in self.cx.bbs(func_id) {
      if self.is_trampoline(bb_id) {
        self.trampoline.insert(bb_id.get_bb_id());
        let (pred_bb_id, target_bb_id) = {
          let bb = self.cx.get_bb(bb_id);
          let (succ_bb_id, _) = bb.succs[0];
          let (pred_bb_id, _) = bb.preds[0];
          (pred_bb_id, succ_bb_id)
        };
        self.cx.redirect_bb(pred_bb_id, bb_id, target_bb_id);
      }
    }
  }

  fn fallthrough(&mut self) {
    let func_id = self.cx.get_current_func_id();

    for bb_id in self.cx.bbs(func_id) {
      let next_valid = self.cx.next_valid(bb_id);

      let cur = &self.cx.get_bb(bb_id).cur;
      let branch_id = if cur.len() >= 2 {
        cur[cur.len() - 2]
      } else {
        continue;
      };

      let jump_id = *self.cx.get_bb(bb_id).cur.last().unwrap();
      let BOpData::M(MOpData::J {
        target: jump_target,
      }) = self.cx.get_op(jump_id).data.clone()
      else {
        // For Ret, skip.
        continue;
      };

      if next_valid.is_some_and(|next_valid_id| next_valid_id == jump_target) {
        self.cx.remove_op(jump_id, Some(bb_id));
        continue;
      }

      let branch_op = self.cx.get_op(branch_id).clone();
      let BOpData::M(branch_data) = branch_op.data.clone() else {
        continue;
      };
      let branch_target = match branch_data.clone() {
        MOpData::Bnez { target, .. } | MOpData::Beqz { target, .. } => target,
        MOpData::Beq { offset, .. }
        | MOpData::Bne { offset, .. }
        | MOpData::Blt { offset, .. }
        | MOpData::Bge { offset, .. }
        | MOpData::Bltu { offset, .. }
        | MOpData::Bgeu { offset, .. } => offset,
        _ => continue,
      };

      if next_valid.is_some_and(|next_valid_id| next_valid_id == branch_target) {
        let inverted_data = match branch_data {
          MOpData::Bnez { rs, .. } => MOpData::Beqz {
            rs,
            target: jump_target,
          },
          MOpData::Beqz { rs, .. } => MOpData::Bnez {
            rs,
            target: jump_target,
          },
          MOpData::Beq { rs1, rs2, .. } => MOpData::Bne {
            rs1,
            rs2,
            offset: jump_target,
          },
          MOpData::Bne { rs1, rs2, .. } => MOpData::Beq {
            rs1,
            rs2,
            offset: jump_target,
          },
          MOpData::Blt { rs1, rs2, .. } => MOpData::Bge {
            rs1,
            rs2,
            offset: jump_target,
          },
          MOpData::Bge { rs1, rs2, .. } => MOpData::Blt {
            rs1,
            rs2,
            offset: jump_target,
          },
          MOpData::Bltu { rs1, rs2, .. } => MOpData::Bgeu {
            rs1,
            rs2,
            offset: jump_target,
          },
          MOpData::Bgeu { rs1, rs2, .. } => MOpData::Bltu {
            rs1,
            rs2,
            offset: jump_target,
          },
          _ => unreachable!(),
        };
        let mut new_branch_op = branch_op;
        new_branch_op.data = inverted_data.into();
        self.cx.replace_op(branch_id, bb_id, new_branch_op);
        self.cx.remove_op(jump_id, Some(bb_id));
      }
    }
  }

  // FIXME: For simplicity, we use CFG API to remove it directly.
  fn clean_up(&mut self) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in std::mem::take(&mut self.trampoline).iter() {
      let bb_id = BOperand::BB(bb_id);
      // Remove terminator
      let jump_id = *self.cx.get_bb(bb_id).cur.last().unwrap();
      self.cx.remove_op(jump_id, Some(bb_id));
      // Remove the block
      let cfg = &mut self.cx.get_func_mut(func_id).cfg;
      cfg.remove(bb_id.get_bb_id());
    }
  }
}

impl<'a> BPass<'a> for BranchFolding<'a> {
  fn name(&self) -> &str {
    "BranchFolding"
  }

  fn mount(&mut self, ir: &'a mut BackIR) {
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.funcs() {
      self.init(func_id);
      self.trampoline_forward();
      self.clean_up();
      self.fallthrough();
    }
  }
}
