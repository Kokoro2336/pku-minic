//! Instruction Selection (ISel).
//! Translating Lower IR to Machine IR.

use yachiyo::pass::BPass;
use yachiyo::ir::back::*;

#[derive(Default)]
pub struct ISel<'a> {
    ir: Option<&'a mut BackIR>,
    builder: BBuilder,
}

impl ISel<'_> {
    pub fn init(&mut self, func_id: usize) {
        self.builder.set_current_func(Some(func_id));
    }

    pub fn select(&mut self) {
        todo!()
    }
}

impl<'a> BPass<'a> for ISel<'a> {
    fn name(&self) -> &str {
        "ISel"
    }

    fn mount(&mut self, program: &'a mut BackIR) {
        self.ir = Some(program);
    }

    fn run(&mut self) {
        for func_id in self.ir.as_ref().unwrap().funcs.ids() {
            self.init(func_id);
            self.select();
        }
    }
}
