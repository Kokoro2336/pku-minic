//! Call Graph Analysis.

use yachiyo::analysis::{Analysis, CallGraph};
use yachiyo::ir::mid::{OpData, OpType, Operand, IR};

#[derive(Default)]
pub struct CallGraphAnalysis<'a> {
  ir: Option<&'a IR>,
}

impl<'a> Analysis<'a> for CallGraphAnalysis<'a> {
  type Input = IR;
  type Output = CallGraph;

  fn name(&self) -> &str {
    "Call Graph Analysis"
  }

  fn mount(&mut self, input: &'a Self::Input) {
    self.ir = Some(input);
  }

  fn run(&mut self) -> Self::Output {
    let funcs_len = self.ir.as_ref().unwrap().funcs.len();
    let mut callers = vec![vec![]; funcs_len];
    let mut callees = vec![vec![]; funcs_len];

    for func_id in self.ir.as_mut().unwrap().funcs.collect() {
      let func_id = Operand::Func(func_id);
      let call_ops = self
        .ir
        .as_ref()
        .unwrap()
        .get_all_ops(Some(func_id), OpType::Call);
      for call_op in call_ops {
        let OpData::Call { func, .. } = &self.ir.as_ref().unwrap().funcs[func_id].dfg[call_op].data
        else {
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
