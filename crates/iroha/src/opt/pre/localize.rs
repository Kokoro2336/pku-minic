//! Localize Global Scalar.

use crate::analysis::CallGraphAnalysis;

use yachiyo::analysis::{analyze, CallGraph};
use yachiyo::base::Type;
use yachiyo::ir::mid::{Attr, Op, OpData, Operand, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::Worklist;

use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct Localize<'a> {
  cx: PassContext<'a>,
  /// FuncId -> GlobalId used by the function locally -> Load/Store Ids
  mem_insts: Vec<FxHashMap<Operand, Vec<Operand>>>,
  /// FuncId -> Call/Ret Ids
  barriers: Vec<Vec<Operand>>,
  /// FuncId -> GlobalId used by the function indirectly through calls
  might_used_globals: Vec<BitSet>,
}

impl Localize<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn is_mutable_global(&self, global: Operand) -> bool {
    self.cx.globals()[global]
      .attrs
      .iter()
      .any(|attr| matches!(attr, Attr::GlobalArray { mutable: true, .. }))
  }

  fn run(&mut self) {
    let func_id = self.cx.current_func();
    let globals = self.mem_insts[func_id.get_func_id()]
      .keys()
      .cloned()
      .collect::<Vec<_>>();
    let entry = self.cx.get_func(func_id).cfg.entry.unwrap();
    self.cx.set_current_block(Operand::BB(entry));

    for global in globals {
      // Create alloca at the head of entry
      let global_typ = self.cx.get_op_type(global);
      let alloca_id = self.cx.create_at_head(Op::new(
        global_typ.clone(),
        vec![],
        OpData::Alloca(global_typ.unwrap_ptr()),
      ));

      // Replace all load/store with the alloca
      for &mem_inst in &self.mem_insts[func_id.get_func_id()][&global] {
        let src_tuple = self
          .cx
          .get_src_tuple(mem_inst)
          .iter()
          .map(|(op_id, idx)| (**op_id, *idx))
          .collect::<Vec<_>>();

        for (src_id, src_idx) in src_tuple {
          if src_id == global {
            self.cx.replace_use((mem_inst, src_idx), global, alloca_id);
          }
        }
      }

      // Store global's value to the alloca at the entry
      self.cx.set_after_inst(Some(alloca_id));
      let load_id = self.cx.create(Op::new(
        global_typ.unwrap_ptr(),
        vec![],
        OpData::Load { addr: global },
      ));
      self.cx.create(Op::new(
        Type::Void,
        vec![],
        OpData::Store {
          addr: alloca_id,
          value: load_id,
        },
      ));

      // Insert flush and reload around barriers
      for &barrier in &self.barriers[func_id.get_func_id()] {
        let mut guard = self.cx.guard();
        let barrier_bb = guard.op_bb(barrier);
        guard.set_current_block(barrier_bb);

        let op_data = guard.get_op_data(barrier).clone();
        match op_data {
          OpData::Call { func, .. } => {
            if !self.might_used_globals[func.get_func_id()].contains(global.get_global_id()) {
              continue;
            }

            guard.set_before_inst(Some(barrier));
            let load_alloca_id = guard.create(Op::new(
              global_typ.unwrap_ptr(),
              vec![],
              OpData::Load { addr: alloca_id },
            ));
            guard.create(Op::new(
              Type::Void,
              vec![],
              OpData::Store {
                addr: global,
                value: load_alloca_id,
              },
            ));
            guard.set_after_inst(Some(barrier));
            let load_global_id = guard.create(Op::new(
              global_typ.unwrap_ptr(),
              vec![],
              OpData::Load { addr: global },
            ));
            guard.create(Op::new(
              Type::Void,
              vec![],
              OpData::Store {
                addr: alloca_id,
                value: load_global_id,
              },
            ));
          }
          OpData::Ret { .. } => {
            guard.set_before_inst(Some(barrier));
            let load_alloca_id = guard.create(Op::new(
              global_typ.unwrap_ptr(),
              vec![],
              OpData::Load { addr: alloca_id },
            ));
            guard.create(Op::new(
              Type::Void,
              vec![],
              OpData::Store {
                addr: global,
                value: load_alloca_id,
              },
            ));
          }
          _ => unreachable!(),
        }
      }
    }
  }
}

impl<'a> Pass<'a> for Localize<'a> {
  fn name(&self) -> &str {
    "Localize"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    // Initalize the data structures
    let funcs_len = self.cx.ir().funcs.len();
    self.mem_insts.resize(funcs_len, FxHashMap::default());
    self.barriers.resize(funcs_len, Vec::new());
    self.might_used_globals.resize(funcs_len, BitSet::new());

    // Iterate over each function,
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      self.cx.set_current_func(Some(func_id));
      let bb_ids = self.cx.get_func(func_id).cfg.collect();
      // Collect memory instructions and barriers
      for bb_id in bb_ids {
        let cur = self.cx.get_bb(Operand::BB(bb_id)).cur.clone();
        for inst in cur {
          let op_data = self.cx.get_op_data(inst);
          match op_data {
            OpData::Load { addr } | OpData::Store { addr, .. } => {
              if matches!(addr, Operand::Global(_)) && self.is_mutable_global(*addr) {
                self.mem_insts[func_id.get_func_id()]
                  .entry(*addr)
                  .or_default()
                  .push(inst);
                // Update might-used globals
                if let Operand::Global(global_id) = addr {
                  self.might_used_globals[func_id.get_func_id()].insert(*global_id);
                }
              }
            }
            OpData::Call { .. } | OpData::Ret { .. } => {
              self.barriers[func_id.get_func_id()].push(inst);
            }
            _ => {}
          }
        }
      }
    }

    let func_ids = self.cx.ir().funcs.collect_internal();

    // Reversely propagate might-used globals through call graph
    let CallGraph {
      callers, callees, ..
    } = analyze::<CallGraphAnalysis>(self.cx.ir());
    let mut worklist: Worklist<Operand, BitSet> = Worklist::new();
    for &func_id in &func_ids {
      worklist.push_back(Operand::Func(func_id));
    }

    while let Some(func_id) = worklist.pop_front() {
      let used_globals = self.might_used_globals[func_id.get_func_id()].clone();
      for &callee in &callees[func_id.get_func_id()] {
        let callee_used_globals = self.might_used_globals[callee.get_func_id()].clone();
        self.might_used_globals[func_id.get_func_id()] |= callee_used_globals;
      }
      if used_globals != self.might_used_globals[func_id.get_func_id()] {
        for &caller in &callers[func_id.get_func_id()] {
          worklist.push_back(caller);
        }
      }
    }

    // Start localizing global variables in each function
    for func_id in func_ids {
      self.init(Operand::Func(func_id));
      self.run();
    }
  }
}
