//! Localize Global Variables.

use yachiyo::base::Type;
use yachiyo::ir::mid::{Op, OpData, Operand, IR};
use yachiyo::pass::{Pass, PassContext};

use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct Localize<'a> {
  cx: PassContext<'a>,
  /// FuncId -> GlobalId -> Load/Store Ids
  mem_insts: Vec<FxHashMap<Operand, Vec<Operand>>>,
  /// FuncId -> Call/Ret Ids
  barriers: Vec<Vec<Operand>>,
}

impl Localize<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
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
        let dfg = &mut self.cx.get_func_mut(func_id).dfg;

        for (src_id, src_idx) in src_tuple {
          if src_id == global {
            dfg.replace_use((mem_inst, src_idx), global, alloca_id);
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

        let op_data = guard.get_func(func_id).dfg[barrier].data.clone();
        match op_data {
          OpData::Call { .. } => {
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

    // Iterate over each function,
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let func = &self.cx.get_func(func_id);
      // Collect memory instructions and barriers
      for bb_id in func.cfg.collect() {
        let cur = self.cx.get_func(func_id).cfg[bb_id].cur.clone();
        for inst in cur {
          let op_data = &self.cx.get_func(func_id).dfg[inst].data;
          match op_data {
            OpData::Load { addr } | OpData::Store { addr, .. } => {
              if matches!(addr, Operand::Global(_)) {
                self.mem_insts[func_id.get_func_id()]
                  .entry(*addr)
                  .or_default()
                  .push(inst);
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

    // Start localizing global variables in each function
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.init(Operand::Func(func_id));
      self.run();
    }
  }
}
