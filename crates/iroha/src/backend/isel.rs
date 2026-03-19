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

    pub fn init(&mut self, func_id: usize) {
        self.builder.set_current_func(Some(func_id));
    }

    pub fn select(&mut self) {
        todo!()
    }

    pub fn run(&mut self) -> MachineIR {
        // Transport DataInfo and RoDataInfo.
        self.machine_ir.rodata_info = std::mem::take(&mut self.lower_ir.rodata_info);
        self.machine_ir.data_info = std::mem::take(&mut self.lower_ir.data_info);

        // Pre-allocate functions

        std::mem::take(&mut self.machine_ir)
    }
}
