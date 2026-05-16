//! Strong Connected Components (SCC) Analysis via Tarjan's Algorithm.

use yachiyo::analysis::{Analysis, CallGraph, SCCS};
use yachiyo::ir::mid::Operand;
use yachiyo::utils::BitSet;

const INVALID: usize = usize::MAX;

pub struct SCCAnalysis<'a> {
  call_graph: &'a CallGraph,
  dfn: Vec<usize>,
  low: Vec<usize>,
  stack: Vec<Operand>,
  in_stack: BitSet,
  counter: usize,
  sccs: SCCS,
}

impl SCCAnalysis<'_> {
  fn init(&mut self) {
    self.dfn.clear();
    self.low.clear();
    self.stack.clear();
    self.in_stack.clear();
    self.counter = 0;

    self.dfn.resize(self.call_graph.callers.len(), INVALID);
    self.low.resize(self.call_graph.callers.len(), INVALID);
  }

  fn dfs(&mut self, n: usize) {
    self.dfn[n] = self.counter;
    self.low[n] = self.counter;
    self.counter += 1;

    self.stack.push(Operand::Func(n));
    self.in_stack.insert(n);

    for &m in &self.call_graph.callees[n] {
      let m = usize::from(m);
      if self.dfn[m] == INVALID {
        self.dfs(m);
        self.low[n] = self.low[n].min(self.low[m]);
      } else if self.in_stack.contains(m) {
        self.low[n] = self.low[n].min(self.dfn[m]);
      }
    }

    if self.low[n] == self.dfn[n] {
      let mut component = Vec::new();
      loop {
        let m = self.stack.pop().unwrap();
        self.in_stack.remove(usize::from(m));
        component.push(m);
        if m == Operand::Func(n) {
          break;
        }
      }
      self.sccs.push_component(component);
    }
  }
}

impl<'a> Analysis for SCCAnalysis<'a> {
  type Input = &'a CallGraph;
  type Output = SCCS;

  fn name() -> &'static str {
    "SCC Analysis"
  }

  fn new(input: Self::Input) -> Self {
    Self {
      call_graph: input,
      in_stack: BitSet::new(),
      counter: 0,
      stack: Vec::new(),
      dfn: Vec::new(),
      low: Vec::new(),
      sccs: SCCS::default(),
    }
  }

  fn run(&mut self) -> Self::Output {
    self.init();

    for func_id in 0..self.call_graph.callers.len() {
      if self.dfn[func_id] == INVALID {
        self.dfs(func_id);
      }
    }

    std::mem::take(&mut self.sccs)
  }
}
