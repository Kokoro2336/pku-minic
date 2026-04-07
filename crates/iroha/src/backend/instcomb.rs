//! Instruction Combination (InstComb) .

use yachiyo::ir::back::{BackIR, BBuilder, BOperand};
use yachiyo::pass::BPass;

#[derive(Default)]
pub struct InstComb<'a> {
    ir: Option<&'a mut BackIR>,
    builder: BBuilder,
}

impl InstComb<'_> {
    pub fn init(&mut self, func_id: BOperand) {
        self.builder.set_current_func(func_id);
    }

    pub fn combine(&mut self) {
        
    }
}

impl<'a> BPass<'a> for InstComb<'a> {
    fn name(&self) -> &str {
        "InstComb"
    }

    fn mount(&mut self, ir: &'a mut BackIR) {
        self.ir = Some(ir);
    }

    fn run(&mut self) {
    }
}
