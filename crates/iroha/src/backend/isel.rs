//! Instruction Selection (ISel).
//! Translating Lower IR to Machine IR.

use yachiyo::ir::back::*;

pub struct ISel {
    ir: BackIR,
    builder: BBuilder,
}

impl ISel {
    pub fn new(ir: BackIR) -> Self {
        Self {
            ir,
            builder: BBuilder::new(),
        }
    }

    pub fn init(&mut self, func_id: usize) {
        self.builder.set_current_func(Some(func_id));
    }

    pub fn select(&mut self) {
        todo!()
    }

    pub fn run(&mut self) -> BackIR {
        todo!()
    }
}
