/// Garbage collection and reference rewriting support for Arena.
use crate::base::Pass;
use crate::ir::mid::MidIR;
use crate::utils::arena::Arena;

pub struct Compaction<'a> {
    program: Option<&'a mut MidIR>,
}

impl<'a> Compaction<'a> {
    pub fn new() -> Self {
        Self { program: None }
    }
}

impl<'a> Pass<'a> for Compaction<'a> {
    fn name(&self) -> &str {
        "Compaction"
    }
    fn set_program(&mut self, ir: &'a mut MidIR) {
        self.program = Some(ir);
    }
    fn run(&mut self) {
        self.program.as_mut().unwrap().funcs.gc();
    }
}
