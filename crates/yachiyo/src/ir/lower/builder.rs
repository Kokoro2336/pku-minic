//! Lower LowerIR Builder definition.

use crate::ir::lower::{LOp, LOperand, LowerIR};

use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct LBuilder {
    /// current function
    pub current_function: Option<usize>,
    /// current basic block
    pub current_block: Option<LOperand>,
    /// insertion point: insert before this instruction; None means append at block end.
    pub current_inst: Option<LOperand>,
}

pub struct LBuilderGuard<'a> {
    pub builder: &'a mut LBuilder,
    current_block: Option<LOperand>,
    current_inst: Option<LOperand>,
}

impl<'a> LBuilderGuard<'a> {
    pub fn new(builder: &'a mut LBuilder) -> Self {
        let current_block = builder.current_block.clone();
        let current_inst = builder.current_inst.clone();
        Self {
            builder,
            current_block,
            current_inst,
        }
    }
}

impl Deref for LBuilderGuard<'_> {
    type Target = LBuilder;

    fn deref(&self) -> &Self::Target {
        self.builder
    }
}

impl DerefMut for LBuilderGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.builder
    }
}

impl Drop for LBuilderGuard<'_> {
    fn drop(&mut self) {
        self.builder.current_block = self.current_block.clone();
        self.builder.current_inst = self.current_inst.clone();
    }
}

impl LBuilder {
    #[inline(always)]
    pub fn set_current_func(&mut self, func_id: Option<usize>) {
        self.current_function = func_id;
        self.current_block = None;
        self.current_inst = None;
    }
    #[inline(always)]
    pub fn set_current_block(&mut self, block_id: LOperand) {
        self.current_block = Some(block_id);
        self.current_inst = None;
    }
    #[inline(always)]
    pub fn set_current_inst(&mut self, inst_id: LOperand) {
        self.current_inst = Some(inst_id);
    }
    // set insertion point before inst
    // None: at the end
    // inst_id must be in current block
    pub fn set_before_inst(
        &mut self,
        program: &mut LowerIR,
        current_function: Option<usize>,
        inst_id: Option<LOperand>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "Builder set_before_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("Builder set_before_inst: current_block is None");
        }

        let current_block = self.current_block.as_ref().unwrap().get_bb_id();
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }
        if bb.cur.contains(&inst_id.clone().unwrap()) {
            self.current_inst = inst_id;
        } else {
            panic!(
                "Builder set_before_inst: inst {:?} not in current_block {:?}",
                inst_id, self.current_block
            );
        }
    }

    pub fn set_after_inst(
        &mut self,
        program: &mut LowerIR,
        current_function: Option<usize>,
        inst_id: Option<LOperand>,
    ) {
        let cfg = program.cfg_mut_or_panic(
            current_function,
            "Builder set_after_inst: no current function",
        );
        if self.current_block.is_none() {
            panic!("Builder set_after_inst: current_block is None");
        }

        let current_block = self.current_block.as_ref().unwrap().get_bb_id();
        let bb = &mut cfg[current_block];
        if inst_id.is_none() {
            self.current_inst = None;
            return;
        }
        if bb.cur.contains(&inst_id.clone().unwrap()) {
            let pos = bb
                .cur
                .iter()
                .position(|id| id == &inst_id.clone().unwrap())
                .unwrap_or_else(|| {
                    panic!(
                        "Builder set_after_inst: inst {:?} not found in current_block {:?}",
                        inst_id, self.current_block
                    )
                });
            if pos + 1 < bb.cur.len() {
                self.current_inst = Some(bb.cur[pos + 1].clone());
            } else {
                self.current_inst = None;
            }
        } else {
            panic!(
                "Builder set_after_inst: inst {:?} not in current_block {:?}",
                inst_id, self.current_block
            );
        }
    }

    pub fn create(
        &mut self,
        program: &mut LowerIR,
        current_function: Option<usize>,
        op: LOp,
    ) -> LOperand {
        program.create(self, current_function, op)
    }

    pub fn create_at_head(
        &mut self,
        program: &mut LowerIR,
        current_function: Option<usize>,
        op: LOp,
    ) -> LOperand {
        program.create_at_head(self, current_function, op)
    }

    pub fn create_new_block(
        &mut self,
        program: &mut LowerIR,
        current_function: Option<usize>,
    ) -> LOperand {
        program.create_new_block(current_function)
    }
}
