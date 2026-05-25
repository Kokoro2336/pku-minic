//! Branch Folding, including:
//! - Trampoline Forwarding
//! - Adjacent Blocks Fallthrough

use yachiyo::ir::back::{BOperand, BackIR, MOpData};
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
      let jump_id = *self.cx.get_bb(bb_id).cur.last().unwrap();
      let MOpData::J { target } = self.cx.get_op(jump_id).data.clone().into() else {
        // For Ret, skip.
        continue;
      };

      if next_valid.is_some_and(|next_valid_id| next_valid_id == target) {
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
