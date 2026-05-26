//! Loop structures shared by loop analyses and transforms.

use std::ops::{Deref, DerefMut, Index, IndexMut};

use crate::ir::mid::Operand;
use crate::utils::BitSet;

pub const INVALID_LOOP_LEVEL: LoopLevel = LoopLevel(0);
pub const MAX_LOOP_LEVEL: LoopLevel = LoopLevel(usize::MAX);

#[derive(Debug, Eq, PartialEq, Clone, Copy, PartialOrd)]
/// Loop level, 0 for top-level loops, 1 for loops nested directly inside them, etc.
pub struct LoopLevel(usize);

impl From<usize> for LoopLevel {
  fn from(value: usize) -> Self {
    Self(value)
  }
}

impl From<LoopLevel> for usize {
  fn from(value: LoopLevel) -> Self {
    value.0
  }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy, Hash)]
/// LoopId
pub struct LoopId(pub usize);

impl From<usize> for LoopId {
  fn from(value: usize) -> Self {
    Self(value)
  }
}

impl From<LoopId> for usize {
  fn from(value: LoopId) -> Self {
    value.0
  }
}

#[derive(Debug, Clone)]
pub struct LoopData {
  pub header: Operand,
  pub parent: Option<LoopId>,
  pub level: LoopLevel,
  /// All the blocks in the loop, including the header and the blocks in its inner loops.
  pub blocks: BitSet,
  /// The blocks that are owned by the loop, excluding the blocks in its inner loops.
  pub owned_blocks: BitSet,
  /// The exit blocks of the loop.
  pub exit_blocks: BitSet,
}

impl LoopData {
  pub fn new(header: Operand) -> Self {
    Self {
      header,
      parent: None,
      level: INVALID_LOOP_LEVEL,
      blocks: BitSet::new(),
      owned_blocks: BitSet::new(),
      exit_blocks: BitSet::new(),
    }
  }

  #[inline(always)]
  pub fn has_invalid_level(&self) -> bool {
    self.level == INVALID_LOOP_LEVEL
  }
}

#[derive(Debug, Default, Clone)]
pub struct Loops(Vec<LoopData>);

impl Loops {
  pub fn include(&self, outer: LoopId, inner: LoopId) -> bool {
    let mut parent_option = Some(inner);
    while let Some(parent) = parent_option {
      if parent == outer {
        return true;
      } else {
        parent_option = self[parent].parent;
      }
    }
    false
  }
  pub fn is_ancestor(&self, ancestor: LoopId, descendant: LoopId) -> bool {
    self.include(ancestor, descendant) && ancestor != descendant
  }
}

impl Index<LoopId> for Loops {
  type Output = LoopData;

  fn index(&self, index: LoopId) -> &Self::Output {
    &self.0[usize::from(index)]
  }
}

impl IndexMut<LoopId> for Loops {
  fn index_mut(&mut self, index: LoopId) -> &mut Self::Output {
    &mut self.0[usize::from(index)]
  }
}

impl Deref for Loops {
  type Target = Vec<LoopData>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for Loops {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}
