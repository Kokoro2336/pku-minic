//! Arena Compaction Pass.

use yachiyo::ir::mid::IR;
use yachiyo::pass::{Pass, PassContext};

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
    self.cx.ir_mut().gc();
  }
}
