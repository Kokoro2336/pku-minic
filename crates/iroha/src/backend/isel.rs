//! Instruction Selection (ISel).
//! Translating Lower IR to Machine IR.

use yachiyo::ir::lower::*;
use yachiyo::ir::machine::*;

pub struct ISel {
    lower_ir: LowerIR,
    machine_ir: MachineIR,
    builder: MBuilder,
}

impl ISel {
    pub fn new(lower_ir: LowerIR) -> Self {
        Self {
            lower_ir,
            builder: MBuilder::new(),
            machine_ir: MachineIR::new(),
        }
    }

    pub fn trans_globals(&mut self) {}
}
