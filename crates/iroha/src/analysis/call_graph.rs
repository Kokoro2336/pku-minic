//! Call Graph Analysis.

use yachiyo::analysis::{Analysis, CallGraph};
use yachiyo::ir::mid::{OpData, OpType, Operand, IR};

pub struct CallGraphAnalysis<'a> {
  ir: &'a IR,
}

impl<'a> Analysis for CallGraphAnalysis<'a> {
  type Input = &'a IR;
  type Output = CallGraph;

  fn name(&self) -> &str {
    "Call Graph Analysis"
  }

  fn new(input: Self::Input) -> Self {
    Self { ir: input }
  }

  fn run(&mut self) -> Self::Output {
    let funcs_len = self.ir.funcs.len();
    let mut callers = vec![vec![]; funcs_len];
    let mut callees = vec![vec![]; funcs_len];

    for func_id in self.ir.funcs.collect() {
      let func_id = Operand::Func(func_id);
      let call_ops = self.ir.get_all_ops(Some(func_id), OpType::Call);
      for call_op in call_ops {
        let OpData::Call { func, .. } = &self.ir.funcs[func_id].dfg[call_op].data else {
          unreachable!()
        };

        let Operand::Func(callee_id) = func else {
          unreachable!("Unexpected call target: {:?}", func);
        };
        callers[*callee_id].push(func_id);
        callees[func_id.get_func_id()].push(Operand::Func(*callee_id));
      }
    }

    CallGraph { callers, callees }
  }
}
