//! Lower IR Builder definition.

use crate::ir::lower::LOperand;

use std::ops::{Deref, DerefMut};

pub struct LBuilder {
    // current basic block
    pub current_block: Option<LOperand>,
    // insertion point: insert before this instruction; None means append at block end.
    pub current_inst: Option<LOperand>,
}

pub struct LBuilderGuard<'a> {
    pub builder: &'a mut LBuilder,
    current_block: Option<LOperand>,
    current_inst: Option<LOperand>,
}

impl<'a> LBuilderGuard<'a> {
    pub fn new(builder: &'a mut LBuilder) -> Self {
        let current_block = builder.current_block;
        let current_inst = builder.current_inst;
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
