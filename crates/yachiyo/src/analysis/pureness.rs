//! Pureness Analysis.

use crate::ir::mid::Operand;

use std::ops::{Deref, DerefMut, Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pureness {
  Pure,
  ReadOnly,
  Impure,
}

#[derive(Default)]
pub struct PurenessResult(Vec<Pureness>);

impl Deref for PurenessResult {
  type Target = Vec<Pureness>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for PurenessResult {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl Index<Operand> for PurenessResult {
  type Output = Pureness;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Func(id) => &self.0[id],
      _ => panic!("PurenessResult can only be indexed by FuncId"),
    }
  }
}

impl IndexMut<Operand> for PurenessResult {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Func(id) => &mut self.0[id],
      _ => panic!("PurenessResult can only be indexed by FuncId"),
    }
  }
}

impl Index<usize> for PurenessResult {
  type Output = Pureness;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for PurenessResult {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}
