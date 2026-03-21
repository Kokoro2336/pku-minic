//! Garbage collection and reference rewriting support for Arena.

use yachiyo::pass::Pass;
use yachiyo::ir::mid::IR;
use yachiyo::utils::arena::Arena;

#[derive(Default)]
pub struct Compaction<'a> {
    program: Option<&'a mut IR>,
}

impl<'a> Pass<'a> for Compaction<'a> {
    fn name(&self) -> &str {
        "Compaction"
    }
    fn mount(&mut self, ir: &'a mut IR) {
        self.program = Some(ir);
    }
    fn run(&mut self) {
        self.program.as_mut().unwrap().funcs.gc();
    }
}
