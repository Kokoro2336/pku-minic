//! Function Inlining.

use yachiyo::analysis::{CallSiteInfo, SCCS};
use yachiyo::base::Type;
use yachiyo::ir::mid::{Op, OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::r#match::match_src;

use crate::analysis::{CallGraphAnalysis, LoopAnalysis, LoopId, Loops, SCCAnalysis};

// More aggressive inlining!!!
const MAX_INLINE_INSTS: usize = 200;
const MIN_INLINE_DEPTH: usize = 1;

#[derive(Default)]
pub struct Inlining<'a> {
  cx: PassContext<'a>,

  /// ParamIdx -> Arg OpId
  param_map: Vec<Operand>,
  /// Callee's ValueId -> Caller's new ValueId
  value_map: Vec<Operand>,
  /// Callee's BBId -> Caller's new BBId
  bb_map: Vec<Operand>,

  /// (PhiId, incomings) in callee.
  old_phis: Vec<(Operand, Vec<PhiIncoming>)>,
  /// RetId in callee.
  old_rets: Vec<Operand>,
}

impl Inlining<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn get(&self, operand: Operand) -> Operand {
    match operand {
      Operand::Param(id) => self.param_map[id],
      Operand::Value(id) => self.value_map[id],
      Operand::BB(id) => self.bb_map[id],
      _ => operand,
    }
  }

  fn reset(&mut self, callee: Operand) {
    self.param_map.clear();
    self.value_map.clear();
    self.bb_map.clear();
    self.old_phis.clear();
    self.old_rets.clear();

    self
      .param_map
      .resize(self.cx.get_func(callee).params.len(), Operand::Undefined);
    self
      .value_map
      .resize(self.cx.get_func(callee).dfg.len(), Operand::Undefined);
    self
      .bb_map
      .resize(self.cx.get_func(callee).cfg.len(), Operand::Undefined);
  }

  fn inline(
    &mut self,
    CallSiteInfo {
      callee,
      args,
      call_inst_id,
      caller,
      ..
    }: &CallSiteInfo,
  ) {
    self.reset(*callee);

    // Split the block
    let call_bb_id = self.cx.op_bb(*call_inst_id);
    self.cx.set_current_block(call_bb_id);
    let after_bb_id = self.cx.split_block_after(Some(*call_inst_id));

    // Map the param to the args
    self
      .param_map
      .iter_mut()
      .zip(args.iter())
      .for_each(|(param, arg)| *param = *arg);

    // Map the blocks
    for callee_bb_id in self.cx.get_func(*callee).cfg.collect() {
      let caller_bb_id = self.cx.create_new_block();
      self.bb_map[callee_bb_id] = caller_bb_id;
    }

    // Create instructions in RPO order.
    for &callee_bb_id in self.cx.get_func(*callee).cfg.dpo().iter().rev() {
      let caller_bb_id = self.get(callee_bb_id);
      self.cx.set_current_block(caller_bb_id);
      let cur = {
        let mut guard = self.cx.guard();
        guard.set_current_func(Some(*callee));
        guard.set_current_block(callee_bb_id);
        guard.get_bb(callee_bb_id).cur.clone()
      };
      cur
        .into_iter()
        .for_each(|inst_id| self.clone_inst(inst_id, *callee, callee_bb_id));
    }

    // Redirect original block to the mapped entry
    let old_entry = Operand::BB(self.cx.get_func(*callee).cfg.entry.unwrap());
    let new_entry = self.get(old_entry);
    let original_bb_term_id = self.cx.get_term(call_bb_id);
    self
      .cx
      .redirect_bb(original_bb_term_id, after_bb_id, new_entry);

    // Process phis
    for (old_phi_id, incomings) in std::mem::take(&mut self.old_phis) {
      let new_phi_id = self.get(old_phi_id);
      for incoming in incomings {
        let PhiIncoming::Data { bb, value } = incoming else {
          unreachable!()
        };
        let (new_value, new_bb) = (self.get(value), self.get(bb));
        self.cx.append_phi_incoming(new_phi_id, new_bb, new_value);
      }
    }

    // Process Rets, redirecting them to a new phi in after_bb
    let Type::Function { return_type, .. } = self.cx.get_func(*callee).typ.clone() else {
      unreachable!()
    };
    // If the function has a non-void return type, we need to create a new phi at the head of after_bb.
    let phi_id = if *return_type != Type::Void {
      let mut guard = self.cx.guard();
      guard.set_current_block(after_bb_id);
      Some(guard.create_at_head(Op::new(
        *return_type.clone(),
        vec![],
        OpData::Phi { incomings: vec![] },
      )))
    } else {
      None
    };
    for old_ret_id in std::mem::take(&mut self.old_rets) {
      let new_ret_id = self.get(old_ret_id);
      // If no return value, simply remove Ret
      let ret_bb = self.cx.op_bb(new_ret_id);

      let OpData::Ret { value } = self.cx.get_op(new_ret_id).data.clone() else {
        unreachable!()
      };
      if let Some(value) = value {
        let phi_id = phi_id.unwrap();
        self.cx.append_phi_incoming(phi_id, ret_bb, value);
      }

      self.cx.replace_op(
        new_ret_id,
        ret_bb,
        Op::new(
          Type::Void,
          vec![],
          OpData::Jump {
            target_bb: after_bb_id,
          },
        ),
      );
    }

    if *return_type != Type::Void {
      let phi_id = phi_id.unwrap();
      self.cx.replace_all_uses(*call_inst_id, phi_id);
      // Update arg info in call graph analysis.
      let call_graph = &mut *self
        .cx
        .get_analysis_result_mut::<CallGraphAnalysis>()
        .unwrap();
      call_graph
        .caller_to_info
        .get_mut(caller)
        .unwrap()
        .iter_mut()
        .for_each(|&mut id| {
          let info = &mut call_graph.call_site_infos[id];
          for arg in info.args.iter_mut() {
            if *arg == *call_inst_id {
              *arg = phi_id;
            }
          }
        });
    }
    // Finally, remove the call instruction.
    self.cx.remove_op(*call_inst_id, Some(call_bb_id));
  }

  fn clone_inst(&mut self, inst_id: Operand, callee: Operand, callee_bb_id: Operand) {
    let (typ, attrs, mut op_data) = {
      let mut guard = self.cx.guard();
      guard.set_current_func(Some(callee));
      guard.set_current_block(callee_bb_id);
      let op = guard.get_op(inst_id);
      (op.typ.clone(), op.attrs.clone(), op.data.clone())
    };

    let remap = |operand: &mut Operand| *operand = self.get(*operand);

    if let OpData::Phi { incomings } = op_data.clone() {
      // Fill Phi incomings later.
      op_data = OpData::Phi { incomings: vec![] };
      self.old_phis.push((inst_id, incomings));
    } else {
      // Replace the operand
      match_src! {
        target: &mut op_data,
        bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
        bin_arm: OpData { lhs, rhs } => {
          remap(lhs);
          remap(rhs);
        },
        un_ops: [Sitofp, Fptosi, Zext, Uitofp],
        un_arm: OpData { value } => {
          remap(value);
        },
        fallback: {
          // In DCE, Load is pure.
          OpData::Load { addr } => {
            remap(addr);
          }
          OpData::GEP { base, indices } => {
            remap(base);
            for index in indices.iter_mut() {
              remap(index);
            }
          }

          OpData::Call { func, args } => {
            remap(func);
            for arg in args.iter_mut() {
              remap(arg);
            }
          }

          OpData::Store { addr, value } => {
            remap(addr);
            remap(value);
          }

          OpData::Br { cond, then_bb, else_bb } => {
            remap(cond);
            remap(then_bb);
            remap(else_bb);
          }

          OpData::Jump { target_bb } => {
            remap(target_bb);
          }

          OpData::Ret { value } => {
            if let Some(value) = value.as_mut() {
              remap(value);
            }
            self.old_rets.push(inst_id);
          }

          OpData::Phi {..} => unreachable!(),

          OpData::Alloca(_) => {/*do nothing*/}

          | OpData::GlobalAlloca(_)
          | OpData::Declare { .. } => {
              unreachable!();
          }
        }
      }
    }

    #[cfg(feature = "debug")]
    yachiyo::debug::info!("Cloning inst {:?}", op_data);

    let new_inst_id = self.cx.create(Op::new(typ, attrs, op_data));
    self.value_map[inst_id.get_op_id()] = new_inst_id;
  }

  fn inlinable(
    &self,
    CallSiteInfo {
      caller,
      callee,
      call_inst_id,
      ..
    }: &CallSiteInfo,
    scc: &SCCS,
    callers: &[Vec<Operand>],
    loops: &Loops,
    block_to_loop: &[Option<LoopId>],
  ) -> bool {
    let callee_func = self.cx.get_func(*callee);
    // Non-external function
    !callee_func.is_external
      // Non-recursive function
      && !callers[usize::from(*callee)].contains(callee)
      // Must be small or called in a deep loop (heuristic).
      && (callee_func.dfg.len() <= MAX_INLINE_INSTS
        || block_to_loop[self.cx.op_bb(*call_inst_id).get_bb_id()].is_some_and(|loop_id| {
          usize::from(loops[loop_id].level) >= MIN_INLINE_DEPTH
        }))
      // If recursive, only inline if the call site is not in the same SCC as the callee.
      && !scc[*caller].iter().any(|&func_id| func_id == *callee)
  }
}

impl<'a> Pass<'a> for Inlining<'a> {
  fn name(&self) -> &str {
    "Inlining"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.cx.mount(program);
  }
  fn run(&mut self) {
    let call_graph = &mut *self.cx.analyze_mut::<CallGraphAnalysis>(self.cx.ir());

    let scc = &*self.cx.analyze::<SCCAnalysis>(call_graph);
    let scc_topo = scc.topo(&call_graph.callers, &call_graph.callees);

    for &func_id in scc_topo.iter().rev() {
      if self.cx.get_func(func_id).is_external {
        continue;
      }

      self.init(func_id);
      let call_site_info_ids = call_graph.caller_to_info.entry(func_id).or_default();

      for &mut call_site_info_id in call_site_info_ids {
        let call_site_info = &call_graph.call_site_infos[call_site_info_id];
        // Recompute loops and block_to_loop for each call site, as inlining may change the loop structure.
        let (loops, block_to_loop) = &*self.cx.analyze::<LoopAnalysis>(self.cx.get_func(func_id));
        if self.inlinable(
          call_site_info,
          scc,
          &call_graph.callers,
          loops,
          block_to_loop,
        ) {
          self.inline(call_site_info);
        }
      }
    }
  }
}
