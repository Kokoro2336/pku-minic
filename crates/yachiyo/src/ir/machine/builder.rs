//! Definition of Machine IR builder.

use crate::ir::machine::{MOp, MachineIR};

use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct MBuilder {
    pub current_function: Option<usize>,
    pub current_block: Option<usize>,
    pub current_inst: Option<usize>,
}

pub struct MBuilderGuard<'a> {
    pub builder: &'a mut MBuilder,
    current_function: Option<usize>,
    current_block: Option<usize>,
    current_inst: Option<usize>,
}

impl<'a> MBuilderGuard<'a> {
    pub fn new(builder: &'a mut MBuilder) -> Self {
        let current_function = builder.current_function;
        let current_block = builder.current_block;
        let current_inst = builder.current_inst;
        Self {
            builder,
            current_function,
            current_block,
            current_inst,
        }
    }
}

impl Deref for MBuilderGuard<'_> {
    type Target = MBuilder;

    fn deref(&self) -> &Self::Target {
        self.builder
    }
}

impl DerefMut for MBuilderGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.builder
    }
}

impl Drop for MBuilderGuard<'_> {
    fn drop(&mut self) {
        self.builder.current_function = self.current_function;
        self.builder.current_block = self.current_block;
        self.builder.current_inst = self.current_inst;
    }
}

impl MBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn set_current_func(&mut self, func_id: Option<usize>) {
        self.current_function = func_id;
        self.current_block = None;
        self.current_inst = None;
    }

    #[inline(always)]
    pub fn set_current_block(&mut self, block_id: usize) {
        self.current_block = Some(block_id);
        self.current_inst = None;
    }

    #[inline(always)]
    pub fn set_current_inst(&mut self, inst_id: usize) {
        self.current_inst = Some(inst_id);
    }

    pub fn set_before_inst(
        &mut self,
        program: &mut MachineIR,
        current_function: Option<usize>,
        inst_id: Option<usize>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "MBuilder set_before_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("MBuilder set_before_inst: current_block is None");
        }

        let current_block = self
            .current_block
            .unwrap_or_else(|| panic!("MBuilder set_before_inst: current_block is None"));
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }
        if bb.cur.contains(&inst_id.unwrap_or_default()) {
            self.current_inst = inst_id;
        } else {
            panic!(
                "MBuilder set_before_inst: inst {:?} not in current_block {:?}",
                inst_id, self.current_block
            );
        }
    }

    pub fn set_after_inst(
        &mut self,
        program: &mut MachineIR,
        current_function: Option<usize>,
        inst_id: Option<usize>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "MBuilder set_after_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("MBuilder set_after_inst: current_block is None");
        }

        let current_block = self
            .current_block
            .unwrap_or_else(|| panic!("MBuilder set_after_inst: current_block is None"));
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }

        let inst_id = inst_id.unwrap_or_default();
        if bb.cur.contains(&inst_id) {
            let pos = bb
                .cur
                .iter()
                .position(|id| *id == inst_id)
                .unwrap_or_else(|| {
                    panic!(
                        "MBuilder set_after_inst: inst {:?} not found in current_block {:?}",
                        inst_id, self.current_block
                    )
                });
            if pos + 1 < bb.cur.len() {
                self.current_inst = Some(bb.cur[pos + 1]);
            } else {
                self.current_inst = None;
            }
        } else {
            panic!(
                "MBuilder set_after_inst: inst {:?} not in current_block {:?}",
                inst_id, self.current_block
            );
        }
    }

    pub fn create(
        &mut self,
        program: &mut MachineIR,
        current_function: Option<usize>,
        op: MOp,
    ) -> usize {
        program.create(self, current_function, op)
    }

    pub fn create_at_head(
        &mut self,
        program: &mut MachineIR,
        current_function: Option<usize>,
        op: MOp,
    ) -> usize {
        program.create_at_head(self, current_function, op)
    }

    pub fn create_new_block(
        &mut self,
        program: &mut MachineIR,
        current_function: Option<usize>,
    ) -> usize {
        program.create_new_block(current_function)
    }
}
