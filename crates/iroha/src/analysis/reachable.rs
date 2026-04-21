//! Reachability Analysis.

use yachiyo::utils::set::BitSet;
use yachiyo::analysis::Analysis;
use yachiyo::ir::mid::{Function, Operand};

#[derive(Default)]
pub struct ReachabilityAnalysis<'a> {
  func: Option<&'a Function>,
  visited: BitSet,
}

impl ReachabilityAnalysis<'_> {
  fn dfs(&mut self, bb_id: Operand) {
    if self.visited.contains(bb_id.get_bb_id()) {
      return;
    }
    self.visited.insert(bb_id.get_bb_id());
    let cfg = &self.func.as_ref().unwrap().cfg;
    for (succ, _) in &cfg[bb_id].succs {
      self.dfs(*succ);
    }
  }
}

impl<'a> Analysis<'a> for ReachabilityAnalysis<'a> {
  type Input = Function;
  type Output = BitSet;

  fn name(&self) -> &str {
    "Reachability Analysis"
  }

  fn mount(&mut self, input: &'a Self::Input) {
    self.func = Some(input);
  }

  fn run(&mut self) -> Self::Output {
    let entry = self.func.as_ref().unwrap().cfg.entry.unwrap();
    self.dfs(Operand::BB(entry));
    std::mem::take(&mut self.visited)
  }
}
