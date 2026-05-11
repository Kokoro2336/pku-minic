//! Call Graph Analysis.

use crate::ir::mid::Operand;
use crate::utils::arena::IndexedArena;

use rustc_hash::FxHashMap;
use std::ops::{Deref, DerefMut, Index, IndexMut};

#[derive(Default)]
pub struct CallGraph {
  /// Callers of each function.
  pub callers: Vec<Vec<Operand>>,
  /// Callees of each function.
  pub callees: Vec<Vec<Operand>>,
  /// Callsite information
  pub call_site_infos: IndexedArena<CallSiteInfo>,
  /// Caller -> CallSiteInfo
  pub caller_to_info: FxHashMap<Operand, Vec<CallSiteInfoId>>,
  /// Callee -> CallSiteInfo
  pub callee_to_info: FxHashMap<Operand, Vec<CallSiteInfoId>>,
}

impl CallGraph {
  pub fn get_call_sites_by_caller(&self, caller: Operand) -> Vec<&CallSiteInfo> {
    self
      .caller_to_info
      .get(&caller)
      .into_iter()
      .flatten()
      .map(|info_id| &self[*info_id])
      .collect()
  }

  pub fn get_call_sites_by_callee(&self, callee: Operand) -> Vec<&CallSiteInfo> {
    self
      .callee_to_info
      .get(&callee)
      .into_iter()
      .flatten()
      .map(|info_id| &self[*info_id])
      .collect()
  }
}

#[derive(Debug)]
pub struct CallSiteInfo {
  pub caller: Operand,
  pub callee: Operand,
  /// InstId in caller's namesapce.
  pub call_inst_id: Operand,
  pub args: Vec<Operand>,
}

#[derive(Debug, Clone, Copy)]
pub struct CallSiteInfoId(usize);

impl Deref for CallSiteInfoId {
  type Target = usize;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for CallSiteInfoId {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl From<usize> for CallSiteInfoId {
  fn from(value: usize) -> Self {
    CallSiteInfoId(value)
  }
}

impl From<CallSiteInfoId> for usize {
  fn from(value: CallSiteInfoId) -> Self {
    value.0
  }
}

impl Index<CallSiteInfoId> for CallGraph {
  type Output = CallSiteInfo;

  fn index(&self, index: CallSiteInfoId) -> &Self::Output {
    &self.call_site_infos[index.0]
  }
}

impl IndexMut<CallSiteInfoId> for CallGraph {
  fn index_mut(&mut self, index: CallSiteInfoId) -> &mut Self::Output {
    &mut self.call_site_infos[index.0]
  }
}

impl Index<CallSiteInfoId> for IndexedArena<CallSiteInfo> {
  type Output = CallSiteInfo;

  fn index(&self, index: CallSiteInfoId) -> &Self::Output {
    &self[index.0]
  }
}

impl IndexMut<CallSiteInfoId> for IndexedArena<CallSiteInfo> {
  fn index_mut(&mut self, index: CallSiteInfoId) -> &mut Self::Output {
    &mut self[index.0]
  }
}
