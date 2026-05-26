//! Pass management for IR.

#[cfg(feature = "debug")]
use crate::debug::info;

use crate::analysis::{self, AffineExpr, Analysis, DomTree, MemLoc};
use crate::base::Type;
use crate::cli::Cli;
use crate::debug::DumpLLVM;
use crate::ir::mid::{
  Attr, BasicBlock, Builder, Function, Globals, LoopInfo, Op, OpData, OpType, Operand, PhiIncoming,
  CFG, DFG, IR,
};
use crate::pass::{AnalysisRef, AnalysisRefMut};

use rustc_hash::FxHashMap;
use std::any::{type_name, Any};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct PassContext<'a> {
  pub ir: Option<&'a mut IR>,
  pub builder: Builder,
  analysis_cache: RefCell<FxHashMap<&'static str, Box<dyn Any>>>,
}

pub struct PassContextGuard<'cx, 'a> {
  cx: &'cx mut PassContext<'a>,
  loop_stack: Vec<LoopInfo>,
  current_function: Option<Operand>,
  current_block: Option<Operand>,
  current_inst: Option<Operand>,
}

impl<'cx, 'a> PassContextGuard<'cx, 'a> {
  pub fn new(cx: &'cx mut PassContext<'a>) -> Self {
    Self {
      loop_stack: cx.builder.loop_stack.clone(),
      current_function: cx.builder.current_function,
      current_block: cx.builder.current_block,
      current_inst: cx.builder.current_inst,
      cx,
    }
  }
  pub fn cx(&mut self) -> &mut PassContext<'a> {
    self.cx
  }
}

impl<'a> Deref for PassContextGuard<'_, 'a> {
  type Target = PassContext<'a>;

  fn deref(&self) -> &Self::Target {
    self.cx
  }
}

impl DerefMut for PassContextGuard<'_, '_> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.cx
  }
}

impl Drop for PassContextGuard<'_, '_> {
  fn drop(&mut self) {
    self.cx.builder.loop_stack = self.loop_stack.clone();
    self.cx.builder.current_function = self.current_function;
    self.cx.builder.current_block = self.current_block;
    self.cx.builder.current_inst = self.current_inst;
  }
}

impl<'a> PassContext<'a> {
  pub fn guard(&mut self) -> PassContextGuard<'_, 'a> {
    PassContextGuard::new(self)
  }

  /// # Safety
  /// This function is unsafe because it allows creating multiple mutable references to the same `PassContext`.
  pub unsafe fn guard_unsafe(cx_ptr: *mut PassContext<'_>) -> PassContextGuard<'_, '_> {
    PassContextGuard::new(&mut *cx_ptr)
  }

  pub fn mount(&mut self, ir: &'a mut IR) {
    self.ir = Some(ir);
    self.analysis_cache.borrow_mut().clear();
  }

  pub fn ir(&self) -> &IR {
    self.ir.as_ref().unwrap()
  }

  pub fn ir_mut(&mut self) -> &mut IR {
    self.ir.as_deref_mut().unwrap()
  }

  pub fn globals(&self) -> &Globals {
    &self.ir().globals
  }

  pub fn globals_mut(&mut self) -> &mut Globals {
    &mut self.ir_mut().globals
  }

  pub fn current_function_option(&self) -> Option<Operand> {
    self.builder.current_function
  }

  pub fn get_current_func_id(&self) -> Operand {
    self
      .builder
      .current_function
      .expect("PassContext: current function is None")
  }

  pub fn current_func(&self) -> &Function {
    self.get_func(self.get_current_func_id())
  }

  pub fn current_func_mut(&mut self) -> &mut Function {
    self.get_func_mut(self.get_current_func_id())
  }

  pub fn get_cfg(&self) -> &CFG {
    &self.current_func().cfg
  }

  pub fn get_dfg(&self) -> &DFG {
    &self.current_func().dfg
  }

  pub fn current_block(&self) -> Option<Operand> {
    self.builder.current_block
  }

  pub fn set_current_func(&mut self, func_id: Option<Operand>) {
    self.builder.set_current_func(func_id);
  }

  pub fn set_current_block(&mut self, bb_id: Operand) {
    self.builder.set_current_block(bb_id);
  }

  pub fn set_before_inst(&mut self, inst_id: Option<Operand>) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self
      .builder
      .set_before_inst(ir, current_function_option, inst_id);
  }

  pub fn set_after_inst(&mut self, inst_id: Option<Operand>) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self
      .builder
      .set_after_inst(ir, current_function_option, inst_id);
  }

  pub fn set_before_term(&mut self) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.set_before_term(ir, current_function_option);
  }

  pub fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir().funcs[func_id]
  }

  pub fn get_func_mut(&mut self, func_id: Operand) -> &mut Function {
    &mut self.ir_mut().funcs[func_id]
  }

  pub fn extract_cfg(&self) -> Vec<(Vec<usize>, Vec<usize>)> {
    let cfg = self.get_cfg();
    let mut graph = vec![(vec![], vec![]); cfg.len()];
    for bb_id in cfg.collect() {
      let bb = &cfg[Operand::BB(bb_id)];
      graph[bb_id] = (
        bb.preds
          .iter()
          .map(|(pred_id, _)| pred_id.get_bb_id())
          .collect(),
        bb.succs
          .iter()
          .map(|(succ_id, _)| succ_id.get_bb_id())
          .collect(),
      );
    }
    graph
  }

  pub fn get_pre_header_id(&self, header_id: Operand, dom_tree: &DomTree) -> Option<Operand> {
    self
      .get_bb(header_id)
      .preds
      .iter()
      .find_map(|(pred_id, _)| {
        (!dom_tree.is_dom(header_id.get_bb_id(), pred_id.get_bb_id())).then_some(*pred_id)
      })
  }

  pub fn get_latch_id(&self, header_id: Operand, dom_tree: &DomTree) -> Option<Operand> {
    self
      .get_bb(header_id)
      .preds
      .iter()
      .find_map(|(pred_id, _)| {
        dom_tree
          .is_dom(header_id.get_bb_id(), pred_id.get_bb_id())
          .then_some(*pred_id)
      })
  }

  pub fn analyze<A>(&self, input: A::Input) -> AnalysisRef<A::Output>
  where
    A: Analysis,
    A::Output: 'static,
  {
    let result = analysis::analyze::<A>(input);
    {
      self
        .analysis_cache
        .borrow_mut()
        .insert(type_name::<A>(), Box::new(result));
    }
    self.get_analysis_result::<A>().unwrap()
  }

  pub fn analyze_mut<A>(&self, input: A::Input) -> AnalysisRefMut<A::Output>
  where
    A: Analysis,
    A::Output: 'static,
  {
    let result = analysis::analyze::<A>(input);
    {
      self
        .analysis_cache
        .borrow_mut()
        .insert(type_name::<A>(), Box::new(result));
    }
    self.get_analysis_result_mut::<A>().unwrap()
  }

  pub fn get_analysis_result<A>(&self) -> Option<AnalysisRef<A::Output>>
  where
    A: Analysis,
    A::Output: 'static,
  {
    let cache = self.analysis_cache.borrow();
    cache
      .get(type_name::<A>())
      .and_then(|result| result.downcast_ref::<A::Output>())
      .map(|result| AnalysisRef::new(result as *const A::Output))
  }

  pub fn get_analysis_result_mut<A>(&self) -> Option<AnalysisRefMut<A::Output>>
  where
    A: Analysis,
    A::Output: 'static,
  {
    let mut cache = self.analysis_cache.borrow_mut();
    cache
      .get_mut(type_name::<A>())
      .and_then(|result| result.downcast_mut::<A::Output>())
      .map(|result| AnalysisRefMut::new(result as *mut A::Output))
  }

  pub fn clean_analysis_cache<A>(&self)
  where
    A: Analysis,
  {
    self.analysis_cache.borrow_mut().remove(type_name::<A>());
  }

  pub fn clear_analysis_cache(&self) {
    self.analysis_cache.borrow_mut().clear();
  }

  pub fn get_op_type(&self, operand: Operand) -> Type {
    let func_id = self.get_current_func_id();

    match operand {
      Operand::Value(_) => self.get_op(operand).typ.clone(),
      Operand::Param(_) => self.get_func(func_id).params[operand].typ.clone(),
      Operand::Global(_) => self.ir().globals[operand].typ.clone(),
      Operand::Func(_) => self.ir().funcs[operand].typ.clone(),

      Operand::Bool(_) => Type::Bool,
      Operand::Int(_) => Type::Int,
      Operand::Float(_) => Type::Float,
      Operand::Undefined => Type::Void,

      Operand::BB(_) => unreachable!(),
    }
  }

  pub fn op_bb(&self, op_id: Operand) -> Operand {
    self.get_func(self.get_current_func_id()).op_to_bb[op_id]
  }

  pub fn create(&mut self, op: Op) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.create(ir, current_function_option, op)
  }

  pub fn create_at_head(&mut self, op: Op) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.create_at_head(ir, current_function_option, op)
  }

  pub fn create_before_term(&mut self, op: Op) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self
      .builder
      .create_before_term(ir, current_function_option, op)
  }

  pub fn create_new_block(&mut self) -> Operand {
    let current_function_option = self.builder.current_function;
    self.ir_mut().create_new_block(current_function_option)
  }

  pub fn replace_op(&mut self, op_id: Operand, new_op: Op) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    ir.replace_op(&mut self.builder, current_function_option, op_id, new_op)
  }

  pub fn remove_op(&mut self, op_id: Operand) -> crate::ir::mid::Op {
    let current_function_option = self.builder.current_function;
    self.ir_mut().remove_op(current_function_option, op_id)
  }

  pub fn move_op_to_bb_at(&mut self, op_id: Operand, to_bb: Operand, before_op: Option<Operand>) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .move_op_to_bb_at(current_function_option, op_id, to_bb, before_op);
  }

  pub fn redirect_bb(&mut self, operand: Operand, old_bb: Operand, new_bb: Operand) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    ir.redirect_bb(
      &mut self.builder,
      current_function_option,
      operand,
      old_bb,
      new_bb,
    )
  }

  pub fn split_block_before(&mut self, split_point: Option<Operand>) -> Operand {
    let current_function_option = self.builder.current_function;
    let current_block = self
      .builder
      .current_block
      .expect("PassContext split_block_before: current_block is None");
    let ir = self.ir.as_deref_mut().unwrap();
    ir.split_block_before(
      &mut self.builder,
      current_function_option,
      current_block,
      split_point,
    )
  }

  pub fn split_block_after(&mut self, split_point: Option<Operand>) -> Operand {
    let current_function_option = self.builder.current_function;
    let current_block = self
      .builder
      .current_block
      .expect("PassContext split_block_after: current_block is None");
    let ir = self.ir.as_deref_mut().unwrap();
    ir.split_block_after(
      &mut self.builder,
      current_function_option,
      current_block,
      split_point,
    )
  }

  #[inline(always)]
  pub fn funcs_internal(&self) -> Vec<Operand> {
    self
      .ir()
      .funcs
      .collect_internal()
      .into_iter()
      .map(Operand::Func)
      .collect()
  }

  pub fn replace_all_uses(&mut self, old: Operand, new: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_all_uses(current_function_option, old, new);
  }

  pub fn replace_use(&mut self, op_tuple: (Operand, usize), old: Operand, new: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_use(current_function_option, op_tuple, old, new);
  }

  pub fn users(&self, operand: Operand) -> &[(Operand, usize)] {
    self.ir().users(self.builder.current_function, operand)
  }

  pub fn users_mut(&mut self, operand: Operand) -> &mut Vec<(Operand, usize)> {
    let current_function_option = self.builder.current_function;
    self.ir_mut().users_mut(current_function_option, operand)
  }

  pub fn get_all_ops(&self, op_typ: OpType) -> Vec<Operand> {
    self.ir().get_all_ops(self.builder.current_function, op_typ)
  }

  pub fn get_all_ops_in_block(&self, bb_id: Operand, op_typ: OpType) -> Vec<Operand> {
    self
      .ir()
      .get_all_ops_in_block(self.builder.current_function, bb_id, op_typ)
  }

  pub fn get_all_non_phi_in_block(&mut self, bb_id: Operand) -> Vec<Operand> {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .get_all_non_phi_in_block(current_function_option, bb_id)
  }

  pub fn get_src_tuple(&self, op_id: Operand) -> Vec<(&Operand, usize)> {
    self
      .ir()
      .get_src_tuple(self.builder.current_function, op_id)
  }

  pub fn get_src_tuple_owned(&self, op_id: Operand) -> Vec<(Operand, usize)> {
    self
      .get_src_tuple(op_id)
      .iter()
      .map(|(src, idx)| (**src, *idx))
      .collect()
  }

  pub fn get_src(&self, op_id: Operand) -> Vec<&Operand> {
    self.ir().get_src(self.builder.current_function, op_id)
  }

  pub fn get_src_owned(&self, op_id: Operand) -> Vec<Operand> {
    self.get_src(op_id).iter().map(|src| **src).collect()
  }

  pub fn remove_control_flow(&mut self, op_id: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .remove_control_flow(current_function_option, op_id);
  }

  pub fn add_uses(&mut self, op_id: Operand) {
    let current_function_option = self.builder.current_function;
    self.ir_mut().add_uses(current_function_option, op_id);
  }

  pub fn add_use(&mut self, used: Operand, user_tuple: (Operand, usize)) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .add_use(current_function_option, used, user_tuple);
  }

  pub fn remove_use(&mut self, used: Operand, user_tuple: (Operand, usize)) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .remove_use(current_function_option, used, user_tuple);
  }

  pub fn clear_uses(&mut self) {
    let current_function_option = self.builder.current_function;
    self.ir_mut().clear_uses(current_function_option);
  }

  pub fn get_term(&self, bb_id: Operand) -> Operand {
    *self.get_bb(bb_id).cur.last().unwrap()
  }

  pub fn append_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand, value: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .append_phi_incoming(current_function_option, phi_id, value, bb_id);
  }

  pub fn add_phi_incoming(&mut self, phi_id: Operand, idx: usize, value: Operand, bb_id: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .add_phi_incoming(current_function_option, phi_id, idx, value, bb_id);
  }

  pub fn slay_phi_incoming(&mut self, phi_id: Operand, bb_id: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .slay_phi_incoming(current_function_option, phi_id, bb_id);
  }

  pub fn trace_ptr(&self, operand: Operand, mem_loc: &mut MemLoc) {
    match operand {
      Operand::Global(_) => mem_loc.base = operand,
      Operand::Param(_) => {
        let func_id = self.get_current_func_id();
        let param_typ = &self.get_func(func_id).params[operand].typ;
        if matches!(param_typ, Type::Pointer { .. }) {
          mem_loc.base = operand;
        } else {
          mem_loc.set_unknown(operand);
        }
      }
      Operand::Value(_) => {
        let op = self.get_op(operand);
        let op_data = &op.data;
        match op_data {
          OpData::GEP { base, indices } => {
            let base_typ = self.get_op_type(*base);
            mem_loc.offset += &AffineExpr::from_gep(base_typ, indices.clone());
            self.trace_ptr(*base, mem_loc);
          }
          OpData::Alloca(_) => mem_loc.base = operand,
          OpData::Load { .. } => mem_loc.set_unknown(operand),
          _ => mem_loc.set_unknown(operand),
        }
      }
      Operand::Func(_)
      | Operand::Bool(_)
      | Operand::Int(_)
      | Operand::Float(_)
      | Operand::Undefined
      | Operand::BB(_) => {
        mem_loc.set_unknown(operand);
      }
    }
  }

  pub fn get_phi_incoming_value(&self, phi_id: Operand, bb_id: Operand) -> Option<Operand> {
    let phi = self.get_op(phi_id);
    let OpData::Phi { incomings } = &phi.data else {
      unreachable!("Expected a phi node, got {:?}", phi.data);
    };
    for incoming in incomings {
      let PhiIncoming::Data { value, bb } = incoming else {
        continue;
      };
      if *bb == bb_id {
        return Some(*value);
      }
    }
    None
  }

  /// This API should receive the addr in Load/Store/GEP.
  pub fn compute_mem_loc(&self, operand: Operand) -> MemLoc {
    let typ = self.get_op_type(operand);
    let mut mem_loc = MemLoc::new(typ.unwrap_ptr());
    self.trace_ptr(operand, &mut mem_loc);
    mem_loc
  }

  #[inline(always)]
  pub fn get_op(&self, op_id: Operand) -> &Op {
    match op_id {
      Operand::Value(_) => &self.get_dfg()[op_id],
      Operand::Global(_) => &self.ir().globals[op_id],
      _ => unreachable!("Expected Value or Global operand, got {:?}", op_id),
    }
  }

  #[inline(always)]
  pub fn has_attr(&self, op_id: Operand, attr: &Attr) -> bool {
    self.get_op(op_id).attrs.contains(attr)
  }

  #[inline(always)]
  pub fn add_attr(&mut self, op_id: Operand, attr: Attr) {
    let op = self.get_op_mut(op_id);
    if !op.attrs.contains(&attr) {
      op.attrs.push(attr);
    }
  }

  #[inline(always)]
  pub fn get_attrs(&self, op_id: Operand) -> &[Attr] {
    &self.get_op(op_id).attrs
  }

  #[inline(always)]
  pub fn get_op_mut(&mut self, op_id: Operand) -> &mut Op {
    let func_id = self.get_current_func_id();
    match op_id {
      Operand::Value(_) => &mut self.get_func_mut(func_id).dfg[op_id],
      Operand::Global(_) => &mut self.ir_mut().globals[op_id],
      _ => unreachable!("Expected Value or Global operand, got {:?}", op_id),
    }
  }

  #[inline(always)]
  pub fn get_bb(&self, bb_id: Operand) -> &BasicBlock {
    &self.get_cfg()[bb_id]
  }

  #[inline(always)]
  pub fn get_bb_mut(&mut self, bb_id: Operand) -> &mut BasicBlock {
    let func_id = self.get_current_func_id();
    &mut self.get_func_mut(func_id).cfg[bb_id]
  }

  /// For cases where guard doesn't live long enough, e.g., in AliasAnalysis.
  pub fn with_current_func<R>(
    &mut self,
    func_id: Operand,
    f: impl FnOnce(&mut PassContext<'a>) -> R,
  ) -> R {
    let original_func = self.builder.current_function;
    self.set_current_func(Some(func_id));
    let result = f(self);
    self.set_current_func(original_func);
    result
  }

  #[inline(always)]
  pub fn get_op_data(&self, op_id: Operand) -> &OpData {
    &self.get_op(op_id).data
  }

  #[inline(always)]
  pub fn get_op_data_mut(&mut self, op_id: Operand) -> &mut OpData {
    &mut self.get_op_mut(op_id).data
  }
}

pub trait Pass<'a> {
  /// Get the name of the pass, which will be used for logging and debugging purposes. It should be unique for each pass to avoid confusion in logs.
  fn name(&self) -> &str;
  /// mount the IR to the pass, which will be called before `run()`.
  fn mount(&mut self, program: &'a mut IR);
  /// run the pass on the mounted IR. The IR is guaranteed to be mounted before this method is called.
  fn run(&mut self);
}

pub struct PassManager<'a> {
  // The lifetime 'a is tied to the IR that the passes will operate on.
  // The `+ 'a` bound is necessary because the passes themselves (like DCE<'a>)
  // contain a mutable reference to the IR with lifetime 'a.
  passes: VecDeque<Box<dyn Pass<'a> + 'a>>,
  cli: &'a Cli,
}

impl<'a> PassManager<'a> {
  pub fn new(cli: &'a Cli) -> Self {
    PassManager {
      passes: VecDeque::new(),
      cli,
    }
  }

  pub fn register<P: 'a + Default + Pass<'a>>(mut self) -> Self {
    self.passes.push_back(Box::new(P::default()));
    self
  }

  pub fn run(mut self, ir: &'a mut IR) {
    let ir_ptr: *mut IR = ir;
    while let Some(mut pass) = self.passes.pop_front() {
      #[cfg(feature = "debug")]
      info!("Running pass: {}", pass.name());

      // SAFETY: Passes run sequentially and each pass only borrows `ir` during this iteration.
      unsafe { pass.mount(&mut *ir_ptr) };
      pass.run();

      #[cfg(feature = "debug")]
      info!("Finished pass: {}", pass.name());

      if self.cli.emit_llvm && self.cli.dump_llvm_after == pass.name() {
        #[cfg(feature = "debug")]
        info!("Dumping IR after pass: {}", pass.name());

        let filename = self
          .cli
          .output
          .as_ref()
          .and_then(|path| path.file_stem())
          .and_then(|s| s.to_str())
          .unwrap_or("output")
          .to_string();
        unsafe {
          DumpLLVM::new(&mut *ir_ptr, filename).run();
        }

        #[cfg(feature = "debug")]
        info!("Finished dumping IR after pass: {}", pass.name());
        #[cfg(feature = "debug")]
        info!("Quit after dumping.");

        std::process::exit(0)
      }
    }

    // If no pass specified, dump the LLVM IR after all optimizations.
    if self.cli.dump_llvm_after.is_empty() {
      #[cfg(feature = "debug")]
      info!("Start Dumping LLVM IR.");

      let filename = self
        .cli
        .output
        .as_ref()
        .and_then(|path| path.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
      unsafe {
        DumpLLVM::new(&mut *ir_ptr, filename).run();
      }

      #[cfg(feature = "debug")]
      info!("Finish Dumping LLVM IR.");
      #[cfg(feature = "debug")]
      info!("Quit after dumping.");

      if self.cli.emit_llvm {
        std::process::exit(0)
      }
    }
  }
}
