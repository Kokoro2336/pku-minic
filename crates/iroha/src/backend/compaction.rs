//! Arena Compaction pass.

use yachiyo::ir::back::BackIR;
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::Arena;

#[derive(Default)]
pub struct BCompaction<'a> {
  cx: BPassContext<'a>,
}

impl<'a> BPass<'a> for BCompaction<'a> {
  fn name(&self) -> &str {
    "BCompaction"
  }

  fn mount(&mut self, program: &'a mut BackIR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    // Clear dead vregs first
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.cx.ir_mut().funcs[func_id].vregs.clear_dead();
    }
    // Garbage collection
    self.cx.ir_mut().funcs.gc();
  }
}
