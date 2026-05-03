//! Loop-closed SSA (LCSSA) transformation.

use yachiyo::pass::Pass;
use yachiyo::ir::mid::{IR, Operand, Builder};

#[allow(clippy::upper_case_acronyms)]
pub struct LCSSA<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
}

impl<'a> Pass<'a> for LCSSA<'a> {
  fn name(&self) -> &'static str {
    "LCSSA"
  }

  fn mount(&mut self, ir: &'a mut IR) {
    self.ir = Some(ir);
  }

  fn run(&mut self) {
    todo!()
  }
}
