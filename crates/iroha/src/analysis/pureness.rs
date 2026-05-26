//! Pureness Analysis.

use yachiyo::analysis::{Analysis, CallGraph, MemLoc, Pureness, PurenessResult, SCCS};
use yachiyo::ir::mid::{OpData, Operand};
use yachiyo::pass::PassContext;

pub struct PurenessAnalysis<'a, 'scc, 'cg> {
  cx: &'a mut PassContext<'a>,
  sccs: &'scc SCCS,
  call_graph: &'cg CallGraph,
  pureness: PurenessResult,
}

impl PurenessAnalysis<'_, '_, '_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn analyze(&mut self) {
    let func_id = self.cx.get_current_func_id();
    if self.pureness[func_id] == Pureness::Impure {
      return;
    }

    for bb_id in self.cx.get_cfg().collect() {
      let bb_id = Operand::BB(bb_id);
      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        let op_data = self.cx.get_op(inst_id).data.clone();

        let set = |this: &mut Self, func_id: Operand, pureness: Pureness| {
          let scc = &this.sccs[func_id];
          for &func in scc.iter() {
            this.pureness[func] = this.pureness[func].max(pureness);
          }
        };

        if let OpData::Load { addr } = op_data {
          let MemLoc { base, .. } = self.cx.compute_mem_loc(addr);
          match base {
            Operand::Param(_) | Operand::Global(_) | Operand::Undefined => {
              set(self, func_id, Pureness::ReadOnly);
            }
            Operand::Value(_) => {}
            _ => unreachable!(),
          }
        } else if let OpData::Store { addr, .. } = op_data {
          let MemLoc { base, .. } = self.cx.compute_mem_loc(addr);
          match base {
            Operand::Param(_) | Operand::Global(_) | Operand::Undefined => {
              set(self, func_id, Pureness::Impure);
              return;
            }
            Operand::Value(_) => {}
            _ => unreachable!(),
          }
        } else if let OpData::Call { func: callee, .. } = op_data {
          if callee == func_id {
            continue;
          }

          let func_pureness = self.pureness[callee];
          set(self, func_id, func_pureness);
          if self.pureness[callee] == Pureness::Impure {
            return;
          }
        }
      }
    }
  }
}

impl<'a, 'scc, 'cg> Analysis for PurenessAnalysis<'a, 'scc, 'cg> {
  type Input = (&'a mut PassContext<'a>, &'cg CallGraph, &'scc SCCS);
  type Output = PurenessResult;

  fn name() -> &'static str {
    "Pureness Analysis"
  }

  fn new(input: Self::Input) -> Self {
    let (cx, call_graph, sccs) = input;
    Self {
      cx,
      call_graph,
      sccs,
      pureness: PurenessResult::default(),
    }
  }

  fn run(&mut self) -> Self::Output {
    self
      .pureness
      .resize(self.cx.ir().funcs.len(), Pureness::Pure);

    for func_id in self.cx.ir().funcs.collect() {
      let func_id = Operand::Func(func_id);
      if !self.cx.get_func(func_id).is_external {
        continue;
      }
      self.pureness[func_id] = Pureness::Impure;
    }

    for &func_id in self
      .sccs
      .topo(&self.call_graph.callers, &self.call_graph.callees)
      .iter()
      .rev()
    {
      let _unsafe_guard = unsafe { PassContext::guard_unsafe(self.cx) };
      self.init(func_id);
      self.analyze();
    }

    std::mem::take(&mut self.pureness)
  }
}
