//! Definiton of dominance analysis.

use std::ops::{Index, IndexMut};

#[derive(Default, Clone)]
/// Vertex number -> its children in the dominator tree
pub struct DomTree(Vec<Vec<usize>>);

impl DomTree {
  pub fn with_len(len: usize) -> Self {
    Self(vec![vec![]; len])
  }
  pub fn is_dom(&self, dominator: usize, dominatee: usize) -> bool {
    if dominator == dominatee {
      return true;
    }
    let mut stack = vec![dominator];
    while let Some(node) = stack.pop() {
      if node == dominatee {
        return true;
      }
      stack.extend(self[node].iter().copied());
    }
    false
  }
  pub fn get_idom(&self, node: usize) -> Option<usize> {
    for (idx, children) in self.0.iter().enumerate() {
      if children.contains(&node) {
        return Some(idx);
      }
    }
    None
  }
}

impl Index<usize> for DomTree {
  type Output = Vec<usize>;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for DomTree {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

#[derive(Default, Clone)]
pub struct DomFrontier(Vec<Vec<usize>>);

impl DomFrontier {
  pub fn with_len(len: usize) -> Self {
    Self(vec![vec![]; len])
  }
}

impl Index<usize> for DomFrontier {
  type Output = Vec<usize>;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for DomFrontier {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}
