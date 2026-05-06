//! Arena Compaction Pass.

use yachiyo::ir::mid::IR;
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::arena::Arena;

#[derive(Default)]
pub struct Compaction<'a> {
  cx: PassContext<'a>,
}

impl<'a> Pass<'a> for Compaction<'a> {
  fn name(&self) -> &str {
    "Compaction"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    self.cx.ir_mut().funcs.gc();
  }
}
