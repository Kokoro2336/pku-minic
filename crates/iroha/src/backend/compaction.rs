//! Arena Compaction pass.

use yachiyo::ir::back::BackIR;
use yachiyo::pass::BPass;
use yachiyo::utils::arena::Arena;

#[derive(Default)]
pub struct BCompaction<'a> {
  ir: Option<&'a mut BackIR>,
}

impl<'a> BPass<'a> for BCompaction<'a> {
  fn name(&self) -> &str {
    "BCompaction"
  }

  fn mount(&mut self, program: &'a mut BackIR) {
    self.ir = Some(program);
  }

  fn run(&mut self) {
    // Clear dead vregs first
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      self.ir.as_mut().unwrap().funcs[func_id].vregs.clear_dead();
    }
    // Garbage collection
    self.ir.as_mut().unwrap().funcs.gc();
  }
}
