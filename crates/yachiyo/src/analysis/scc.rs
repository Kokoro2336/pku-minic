//! Strong Connected Components (SCC) Analysis.

use crate::ir::mid::Operand;
use crate::utils::{BitSet, Worklist};

use std::ops::{Deref, DerefMut, Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SCCId(usize);

impl From<SCCId> for usize {
  fn from(id: SCCId) -> Self {
    id.0
  }
}

impl From<usize> for SCCId {
  fn from(value: usize) -> Self {
    SCCId(value)
  }
}

pub struct SCC(Vec<Operand>);

impl Deref for SCC {
  type Target = Vec<Operand>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for SCC {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Default)]
pub struct SCCS {
  func_to_scc: Vec<SCCId>,
  components: Vec<SCC>,
}

impl SCCS {
  pub fn push_component(&mut self, component: Vec<Operand>) {
    let scc_id = SCCId(self.components.len());
    for &func in &component {
      let func_id = func.get_func_id();
      if func_id >= self.func_to_scc.len() {
        self.func_to_scc.resize(func_id + 1, SCCId(usize::MAX));
      }
      self.func_to_scc[func_id] = scc_id;
    }
    self.components.push(SCC(component));
  }

  pub fn topo(&self, callers: &[Vec<Operand>], callees: &[Vec<Operand>]) -> Vec<Operand> {
    let mut topo = Vec::new();
    let mut worklist: Worklist<SCCId, BitSet> = Worklist::new();

    let mut callers_in_scc = vec![vec![]; self.components.len()];
    let mut callees_in_scc = vec![vec![]; self.components.len()];

    let mut collect = |scc_id: SCCId, collect_caller: bool| {
      let scc = &self[scc_id];
      scc
        .iter()
        .flat_map(|&func| {
          if collect_caller {
            &callers[func.get_func_id()]
          } else {
            &callees[func.get_func_id()]
          }
        })
        .for_each(|&func| {
          let func_scc_id = self.func_to_scc[func.get_func_id()];
          if func_scc_id == scc_id {
            return;
          }

          if collect_caller {
            if !callers_in_scc[scc_id.0].contains(&func_scc_id) {
              callers_in_scc[scc_id.0].push(func_scc_id);
            }
          } else if !callees_in_scc[scc_id.0].contains(&func_scc_id) {
            callees_in_scc[scc_id.0].push(func_scc_id);
          }
        });
    };

    for scc_id in 0..self.components.len() {
      let scc_id = SCCId(scc_id);
      collect(scc_id, true);
      collect(scc_id, false);
    }

    for scc_id in 0..self.components.len() {
      let scc_id = SCCId(scc_id);
      if callers_in_scc[scc_id.0].is_empty() {
        worklist.push_back(scc_id);
      }
    }

    while let Some(scc_id) = worklist.pop_front() {
      topo.extend(self[scc_id].iter().copied());
      for &callee in &callees_in_scc[scc_id.0] {
        // It must be impossible that callee_scc_id == scc_id, because it means there is an edge from a SCC to itself, which contradicts the definition of SCC.
        assert!(callee != scc_id);
        callers_in_scc[callee.0].retain(|&caller| caller != scc_id);
        if callers_in_scc[callee.0].is_empty() {
          worklist.push_back(callee);
        }
      }
    }

    topo
  }
}

impl Deref for SCCS {
  type Target = Vec<SCC>;

  fn deref(&self) -> &Self::Target {
    &self.components
  }
}

impl DerefMut for SCCS {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.components
  }
}

impl Index<SCCId> for SCCS {
  type Output = SCC;

  fn index(&self, index: SCCId) -> &Self::Output {
    &self.components[index.0]
  }
}

impl IndexMut<SCCId> for SCCS {
  fn index_mut(&mut self, index: SCCId) -> &mut Self::Output {
    &mut self.components[index.0]
  }
}

impl Index<Operand> for SCCS {
  type Output = SCC;

  fn index(&self, index: Operand) -> &Self::Output {
    let scc_id = self.func_to_scc[index.get_func_id()];
    &self[scc_id]
  }
}

impl IndexMut<Operand> for SCCS {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    let scc_id = self.func_to_scc[index.get_func_id()];
    &mut self[scc_id]
  }
}
