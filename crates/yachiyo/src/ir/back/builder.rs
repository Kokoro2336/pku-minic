//! Definition of Machine IR builder.

use crate::ir::back::{BOp, BOperand, BackIR};

use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct BBuilder {
    pub current_function: Option<BOperand>,
    pub current_block: Option<BOperand>,
    pub current_inst: Option<BOperand>,
}

pub struct BBuilderGuard<'a> {
    pub builder: &'a mut BBuilder,
    current_function: Option<BOperand>,
    current_block: Option<BOperand>,
    current_inst: Option<BOperand>,
}

impl<'a> BBuilderGuard<'a> {
    pub fn new(builder: &'a mut BBuilder) -> Self {
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

impl Deref for BBuilderGuard<'_> {
    type Target = BBuilder;

    fn deref(&self) -> &Self::Target {
        self.builder
    }
}

impl DerefMut for BBuilderGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.builder
    }
}

impl Drop for BBuilderGuard<'_> {
    fn drop(&mut self) {
        self.builder.current_function = self.current_function;
        self.builder.current_block = self.current_block;
        self.builder.current_inst = self.current_inst;
    }
}

impl BBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn set_current_func(&mut self, func_id: BOperand) {
        self.current_function = Some(func_id);
        self.current_block = None;
        self.current_inst = None;
    }

    #[inline(always)]
    pub fn set_current_block(&mut self, block_id: BOperand) {
        self.current_block = Some(block_id);
        self.current_inst = None;
    }

    /// Set the insertion point before the given instruction.
    #[inline(always)]
    pub fn set_current_inst(&mut self, inst_id: BOperand) {
        self.current_inst = Some(inst_id);
    }

    pub fn set_before_inst(
        &mut self,
        program: &mut BackIR,
        current_function: Option<BOperand>,
        inst_id: Option<BOperand>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "BBuilder set_before_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("BBuilder set_before_inst: current_block is None");
        }

        let current_block = self.current_block.as_ref().unwrap().get_bb_id();
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }
        if let Some(inst_id) = inst_id {
            if bb.cur.contains(&inst_id) {
                self.current_inst = Some(inst_id);
            } else {
                panic!(
                    "BBuilder set_before_inst: inst {:?} not in current_block {:?}",
                    inst_id, self.current_block
                );
            }
        } else {
            unreachable!("BBuilder set_before_inst: inst_id checked as Some above")
        }
    }

    pub fn set_after_inst(
        &mut self,
        program: &mut BackIR,
        current_function: Option<BOperand>,
        inst_id: Option<BOperand>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "BBuilder set_after_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("BBuilder set_after_inst: current_block is None");
        }

        let current_block = self.current_block.as_ref().unwrap().get_bb_id();
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }

        if let Some(inst_id) = inst_id {
            if bb.cur.contains(&inst_id) {
                let pos = bb
                    .cur
                    .iter()
                    .position(|id| *id == inst_id)
                    .unwrap_or_else(|| {
                        panic!(
                            "BBuilder set_after_inst: inst {:?} not found in current_block {:?}",
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
                    "BBuilder set_after_inst: inst {:?} not in current_block {:?}",
                    inst_id, self.current_block
                );
            }
        } else {
            unreachable!("BBuilder set_after_inst: inst_id checked as Some above")
        }
    }

    pub fn create(
        &mut self,
        program: &mut BackIR,
        current_function: Option<BOperand>,
        op: BOp,
    ) -> BOperand {
        program.create(self, current_function, op)
    }

    pub fn create_at_head(
        &mut self,
        program: &mut BackIR,
        current_function: Option<BOperand>,
        op: BOp,
    ) -> BOperand {
        program.create_at_head(self, current_function, op)
    }

    pub fn create_new_block(
        &mut self,
        program: &mut BackIR,
        current_function: Option<BOperand>,
    ) -> BOperand {
        program.create_new_block(current_function)
    }
}
