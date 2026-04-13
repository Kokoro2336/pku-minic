//! Builder definition of IR.

use crate::ir::mid::*;

use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone)]
pub struct LoopInfo {
  pub while_entry: Option<Operand>,
  pub end_block: Option<Operand>,
}

#[derive(Default)]
pub struct Builder {
  pub loop_stack: Vec<LoopInfo>,
  pub current_function: Option<Operand>,
  // current basic block
  pub current_block: Option<Operand>,
  // insertion point: insert before this instruction; None means append at block end.
  pub current_inst: Option<Operand>,
}

pub struct BuilderGuard<'a> {
  pub builder: &'a mut Builder,
  loop_stack: Vec<LoopInfo>,
  current_function: Option<Operand>,
  current_block: Option<Operand>,
  current_inst: Option<Operand>,
}

impl<'a> BuilderGuard<'a> {
  pub fn new(builder: &'a mut Builder) -> Self {
    let loop_stack = builder.loop_stack.clone();
    let current_function = builder.current_function.clone();
    let current_block = builder.current_block.clone();
    let current_inst = builder.current_inst.clone();
    Self {
      builder,
      loop_stack,
      current_function,
      current_block,
      current_inst,
    }
  }
}

impl Deref for BuilderGuard<'_> {
  type Target = Builder;

  fn deref(&self) -> &Self::Target {
    self.builder
  }
}

impl DerefMut for BuilderGuard<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.builder
  }
}

impl Drop for BuilderGuard<'_> {
  fn drop(&mut self) {
    self.builder.loop_stack = self.loop_stack.clone();
    self.builder.current_function = self.current_function.clone();
    self.builder.current_block = self.current_block.clone();
    self.builder.current_inst = self.current_inst.clone();
  }
}

#[allow(unused)]
impl Builder {
  pub fn new() -> Self {
    Self {
      loop_stack: vec![],
      current_function: None,
      current_block: None,
      current_inst: None,
    }
  }

  #[inline(always)]
  pub fn set_current_func(&mut self, func_id: Option<Operand>) {
    self.current_function = func_id;
    self.current_block = None;
    self.current_inst = None;
  }

  #[inline(always)]
  pub fn push_loop(&mut self, loop_info: LoopInfo) {
    self.loop_stack.push(loop_info);
  }

  #[inline(always)]
  pub fn pop_loop(&mut self) -> Option<LoopInfo> {
    self.loop_stack.pop()
  }

  #[inline(always)]
  pub fn set_current_block(&mut self, block_id: Operand) {
    self.current_block = Some(block_id);
    self.current_inst = None;
  }

  // set insertion point before inst
  // None: at the end
  // inst_id must be in current block
  pub fn set_before_inst(
    &mut self,
    program: &mut IR,
    current_function: Option<Operand>,
    inst_id: Option<Operand>,
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
    program: &mut IR,
    current_function: Option<Operand>,
    inst_id: Option<Operand>,
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

  pub fn create(&mut self, program: &mut IR, current_function: Option<Operand>, op: Op) -> Operand {
    program.create(self, current_function, op)
  }

  pub fn create_at_head(
    &mut self,
    program: &mut IR,
    current_function: Option<Operand>,
    op: Op,
  ) -> Operand {
    program.create_at_head(self, current_function, op)
  }

  pub fn create_new_block(
    &mut self,
    program: &mut IR,
    current_function: Option<Operand>,
  ) -> Operand {
    program.create_new_block(current_function)
  }
}
