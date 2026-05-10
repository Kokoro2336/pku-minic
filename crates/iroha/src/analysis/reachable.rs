//! Reachability Analysis.

use yachiyo::analysis::Analysis;
use yachiyo::ir::mid::{Function, Operand};
use yachiyo::utils::set::BitSet;

pub struct Reachability<'a> {
  func: &'a Function,
  visited: BitSet,
}

impl Reachability<'_> {
  fn dfs(&mut self, bb_id: Operand) {
    if self.visited.contains(bb_id.get_bb_id()) {
      return;
    }
    self.visited.insert(bb_id.get_bb_id());
    let cfg = &self.func.cfg;
    for (succ, _) in &cfg[bb_id].succs {
      self.dfs(*succ);
    }
  }
}

impl<'a> Analysis for Reachability<'a> {
  type Input = &'a Function;
  type Output = BitSet;

  fn name(&self) -> &str {
    "Reachability Analysis"
  }

  fn new(input: Self::Input) -> Self {
    Self {
      func: input,
      visited: BitSet::new(),
    }
  }

  fn run(&mut self) -> Self::Output {
    let entry = self.func.cfg.entry.unwrap();
    self.dfs(Operand::BB(entry));
    std::mem::take(&mut self.visited)
  }
}
