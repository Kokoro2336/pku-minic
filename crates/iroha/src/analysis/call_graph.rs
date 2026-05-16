//! Call Graph Analysis.

use yachiyo::analysis::{Analysis, CallGraph, CallSiteInfo, CallSiteInfoId};
use yachiyo::ir::mid::{OpData, OpType, Operand, IR};
use yachiyo::utils::IndexedArena;

use rustc_hash::FxHashMap;

pub struct CallGraphAnalysis<'a> {
  ir: &'a IR,
}

impl<'a> Analysis for CallGraphAnalysis<'a> {
  type Input = &'a IR;
  type Output = CallGraph;

  fn name() -> &'static str {
    "Call Graph Analysis"
  }

  fn new(input: Self::Input) -> Self {
    Self { ir: input }
  }

  fn run(&mut self) -> Self::Output {
    let funcs_len = self.ir.funcs.len();
    let mut callers = vec![vec![]; funcs_len];
    let mut callees = vec![vec![]; funcs_len];
    let mut call_site_infos = IndexedArena::new();
    let mut caller_to_infos: FxHashMap<Operand, Vec<CallSiteInfoId>> = FxHashMap::default();
    let mut callee_to_infos: FxHashMap<Operand, Vec<CallSiteInfoId>> = FxHashMap::default();

    for func_id in self.ir.funcs.collect() {
      let func_id = Operand::Func(func_id);
      let call_ops = self.ir.get_all_ops(Some(func_id), OpType::Call);

      for call_op in call_ops {
        let OpData::Call { func, args } = &self.ir.funcs[func_id].dfg[call_op].data else {
          unreachable!()
        };

        let Operand::Func(callee_id) = func else {
          unreachable!("Unexpected call target: {:?}", func);
        };
        callers[*callee_id].push(func_id);
        callees[func_id.get_func_id()].push(Operand::Func(*callee_id));
        let info_id: CallSiteInfoId = call_site_infos
          .alloc(CallSiteInfo {
            caller: func_id,
            callee: Operand::Func(*callee_id),
            call_inst_id: call_op,
            args: args.clone(),
          })
          .into();
        caller_to_infos.entry(func_id).or_default().push(info_id);
        callee_to_infos
          .entry(Operand::Func(*callee_id))
          .or_default()
          .push(info_id);
      }
    }

    CallGraph {
      callers,
      callees,
      call_site_infos,
      caller_to_info: caller_to_infos,
      callee_to_info: callee_to_infos,
    }
  }
}
