//! Pass management for IR.

use crate::base::Type;
use crate::cli::Cli;
#[cfg(feature = "debug")]
use crate::debug::info;
use crate::debug::DumpLLVM;
use crate::ir::mid::{Builder, Function, Globals, LoopInfo, Op, OpType, Operand, IR};

use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct PassContext<'a> {
  pub ir: Option<&'a mut IR>,
  pub builder: Builder,
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

  pub fn mount(&mut self, ir: &'a mut IR) {
    self.ir = Some(ir);
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

  pub fn current_func(&self) -> Operand {
    self
      .builder
      .current_function
      .expect("PassContext: current function is None")
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

  pub fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir().funcs[func_id]
  }

  pub fn get_func_mut(&mut self, func_id: Operand) -> &mut Function {
    &mut self.ir_mut().funcs[func_id]
  }

  pub fn get_op_type(&self, operand: Operand) -> Type {
    let func_id = self.current_func();

    match operand {
      Operand::Value(_) => self.get_func(func_id).dfg[operand].typ.clone(),
      Operand::Param(_) => self.get_func(func_id).params[operand].1.clone(),
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
    self.get_func(self.current_func()).op_to_bb[op_id]
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

  pub fn create_new_block(&mut self) -> Operand {
    let current_function_option = self.builder.current_function;
    self.ir_mut().create_new_block(current_function_option)
  }

  pub fn replace_op(&mut self, op_id: Operand, bb_id: Operand, new_op: Op) -> Operand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    ir.replace_op(
      &mut self.builder,
      current_function_option,
      op_id,
      bb_id,
      new_op,
    )
  }

  pub fn remove_op(&mut self, op_id: Operand, bb_id: Option<Operand>) -> crate::ir::mid::Op {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .remove_op(current_function_option, op_id, bb_id)
  }

  pub fn move_op_to_bb_at(
    &mut self,
    op_id: Operand,
    from_bb: Operand,
    to_bb: Operand,
    before_op: Option<Operand>,
  ) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .move_op_to_bb_at(current_function_option, op_id, from_bb, to_bb, before_op);
  }

  pub fn replace_all_uses(&mut self, old: Operand, new: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_all_uses(current_function_option, old, new);
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

  pub fn remove_control_flow(&mut self, op_id: Operand, bb_id: Operand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .remove_control_flow(current_function_option, op_id, bb_id);
  }

  pub fn add_uses(&mut self, op_id: Operand) {
    let current_function_option = self.builder.current_function;
    self.ir_mut().add_uses(current_function_option, op_id);
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

  pub fn register(mut self, pass: Box<dyn Pass<'a> + 'a>) -> Self {
    self.passes.push_back(pass);
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
