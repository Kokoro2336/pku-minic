//! Definiton of dominance analysis.

use std::ops::{Index, IndexMut};

#[derive(Default, Clone)]
/// Vertex number -> its children in the dominator tree
pub struct DomTree {
  /// Father -> Children
  down: Vec<Vec<usize>>,
  /// Child -> Father
  up: Vec<usize>,
  /// Vertex -> Depth
  depth: Vec<usize>,
}

impl DomTree {
  pub fn with_len(len: usize) -> Self {
    Self {
      down: vec![vec![]; len],
      up: (0..len).collect(),
      depth: vec![0; len],
    }
  }

  pub fn set_idom(&mut self, node: usize, idom: usize) {
    self.down[idom].push(node);
    self.up[node] = idom;
  }

  pub fn update_depth(&mut self) {
    self.depth.fill(0);
    for root in 0..self.up.len() {
      if self.up[root] != root {
        continue;
      }

      let mut stack = vec![(root, 0)];
      while let Some((node, depth)) = stack.pop() {
        self.depth[node] = depth;
        stack.extend(self.down[node].iter().map(|child| (*child, depth + 1)));
      }
    }
  }

  pub fn is_dom(&self, dominator: usize, dominatee: usize) -> bool {
    if dominator == dominatee {
      return true;
    }

    if self.depth[dominator] > self.depth[dominatee] {
      return false;
    }

    let mut node = dominatee;
    while self.depth[node] > self.depth[dominator] {
      let idom = self.up[node];
      if idom == node {
        return false;
      }
      node = idom;
    }

    node == dominator
  }

  pub fn get_idom(&self, node: usize) -> Option<usize> {
    let idom = self.up[node];
    (idom != node).then_some(idom)
  }

  pub fn get_depth(&self, v: usize) -> usize {
    self.depth[v]
  }

  /// Return path from `from` to `to`
  pub fn get_path(&self, from: usize, to: usize) -> Vec<usize> {
    let mut node = to;
    let mut path = vec![node];
    loop {
      if node == from {
        path.reverse();
        return path;
      }

      let idom = self.up[node];
      if idom == node {
        return vec![];
      }

      node = idom;
      path.push(node);
    }
  }

  pub fn lca(&self, u: usize, v: usize) -> Option<usize> {
    let mut u = u;
    let mut v = v;

    while self.depth[u] > self.depth[v] {
      let idom = self.up[u];
      if idom == u {
        return None;
      }
      u = idom;
    }

    while self.depth[v] > self.depth[u] {
      let idom = self.up[v];
      if idom == v {
        return None;
      }
      v = idom;
    }

    while u != v {
      let u_idom = self.up[u];
      let v_idom = self.up[v];
      if u_idom == u || v_idom == v {
        return None;
      }
      u = u_idom;
      v = v_idom;
    }

    Some(u)
  }
}

impl Index<usize> for DomTree {
  type Output = Vec<usize>;

  fn index(&self, index: usize) -> &Self::Output {
    &self.down[index]
  }
}

impl IndexMut<usize> for DomTree {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.down[index]
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
