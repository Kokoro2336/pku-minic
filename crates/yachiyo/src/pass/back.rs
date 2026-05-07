//! Pass management for BackIR.

use crate::cli::Cli;
#[cfg(feature = "debug")]
use crate::debug::info;
use crate::debug::DumpASM;
use crate::ir::back::{BBuilder, BFunction, BOp, BOperand, BType, BackIR, Reg, Slot};

use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct BPassContext<'a> {
  pub ir: Option<&'a mut BackIR>,
  pub builder: BBuilder,
}

pub struct BPassContextGuard<'cx, 'a> {
  cx: &'cx mut BPassContext<'a>,
  current_function: Option<BOperand>,
  current_block: Option<BOperand>,
  current_inst: Option<BOperand>,
}

impl<'cx, 'a> BPassContextGuard<'cx, 'a> {
  pub fn new(cx: &'cx mut BPassContext<'a>) -> Self {
    Self {
      current_function: cx.builder.current_function,
      current_block: cx.builder.current_block,
      current_inst: cx.builder.current_inst,
      cx,
    }
  }
}

impl<'a> Deref for BPassContextGuard<'_, 'a> {
  type Target = BPassContext<'a>;

  fn deref(&self) -> &Self::Target {
    self.cx
  }
}

impl DerefMut for BPassContextGuard<'_, '_> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.cx
  }
}

impl Drop for BPassContextGuard<'_, '_> {
  fn drop(&mut self) {
    self.cx.builder.current_function = self.current_function;
    self.cx.builder.current_block = self.current_block;
    self.cx.builder.current_inst = self.current_inst;
  }
}

impl<'a> BPassContext<'a> {
  pub fn guard(&mut self) -> BPassContextGuard<'_, 'a> {
    BPassContextGuard::new(self)
  }

  pub fn mount(&mut self, ir: &'a mut BackIR) {
    self.ir = Some(ir);
  }

  pub fn ir(&self) -> &BackIR {
    self.ir.as_ref().unwrap()
  }

  pub fn ir_mut(&mut self) -> &mut BackIR {
    self.ir.as_deref_mut().unwrap()
  }

  pub fn current_function_option(&self) -> Option<BOperand> {
    self.builder.current_function
  }

  pub fn current_func(&self) -> BOperand {
    self
      .builder
      .current_function
      .expect("BPassContext: current function is None")
  }

  pub fn current_block(&self) -> Option<BOperand> {
    self.builder.current_block
  }

  pub fn set_current_func(&mut self, func_id: BOperand) {
    self.builder.set_current_func(func_id);
  }

  pub fn set_current_block(&mut self, bb_id: BOperand) {
    self.builder.set_current_block(bb_id);
  }

  pub fn set_before_inst(&mut self, inst_id: Option<BOperand>) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self
      .builder
      .set_before_inst(ir, current_function_option, inst_id);
  }

  pub fn set_after_inst(&mut self, inst_id: Option<BOperand>) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self
      .builder
      .set_after_inst(ir, current_function_option, inst_id);
  }

  pub fn set_at_head(&mut self) {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.set_at_head(ir, current_function_option);
  }

  pub fn get_func(&self, func_id: BOperand) -> &BFunction {
    &self.ir().funcs[func_id]
  }

  pub fn get_func_mut(&mut self, func_id: BOperand) -> &mut BFunction {
    &mut self.ir_mut().funcs[func_id]
  }

  pub fn op_bb(&self, op_id: BOperand) -> BOperand {
    self.get_func(self.current_func()).op_to_bb[op_id]
  }

  pub fn create(&mut self, op: BOp) -> BOperand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.create(ir, current_function_option, op)
  }

  pub fn create_at_head(&mut self, op: BOp) -> BOperand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    self.builder.create_at_head(ir, current_function_option, op)
  }

  pub fn remove_op(&mut self, op_id: BOperand, bb_id: Option<BOperand>) -> BOp {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .remove_op(current_function_option, op_id, bb_id)
  }

  pub fn replace_op_no_rauw(&mut self, op_id: BOperand, bb_id: BOperand, new_op: BOp) -> BOperand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    ir.replace_op_no_rauw(
      &mut self.builder,
      current_function_option,
      op_id,
      bb_id,
      new_op,
    )
  }

  pub fn replace_op(&mut self, op_id: BOperand, bb_id: BOperand, new_op: BOp) -> BOperand {
    self.replace_op_no_rauw(op_id, bb_id, new_op)
  }

  pub fn replace_op_rauw(&mut self, op_id: BOperand, bb_id: BOperand, new_op: BOp) -> BOperand {
    let current_function_option = self.builder.current_function;
    let ir = self.ir.as_deref_mut().unwrap();
    ir.replace_op_rauw(
      &mut self.builder,
      current_function_option,
      op_id,
      bb_id,
      new_op,
    )
  }

  pub fn move_op_to_bb_at(
    &mut self,
    op_id: BOperand,
    old_bb: BOperand,
    new_bb: BOperand,
    pos: Option<BOperand>,
  ) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .move_op_to_bb_at(current_function_option, op_id, old_bb, new_bb, pos);
  }

  pub fn get_rd(&self, op_id: BOperand) -> Option<&BOperand> {
    self.ir().get_rd(self.builder.current_function, op_id)
  }

  pub fn get_vreg_id(&self, op_id: BOperand) -> BOperand {
    self.get_rd(op_id).cloned().unwrap()
  }

  pub fn get_src(&self, op_id: BOperand) -> Vec<&BOperand> {
    self.ir().get_src(self.builder.current_function, op_id)
  }

  pub fn get_src_owned(&self, op_id: BOperand) -> Vec<BOperand> {
    self.get_src(op_id).iter().map(|src| **src).collect()
  }

  pub fn get_src_tuple(&self, op_id: BOperand) -> Vec<(&BOperand, usize)> {
    self
      .ir()
      .get_src_tuple(self.builder.current_function, op_id)
  }

  pub fn get_src_tuple_owned(&self, op_id: BOperand) -> Vec<(BOperand, usize)> {
    self
      .get_src_tuple(op_id)
      .iter()
      .map(|(src, idx)| (**src, *idx))
      .collect()
  }

  pub fn replace_rd(&mut self, inst_id: BOperand, new_operand: BOperand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_rd(current_function_option, inst_id, new_operand);
  }

  pub fn replace_src(
    &mut self,
    use_tuple: (BOperand, usize),
    old_operand: BOperand,
    new_operand: BOperand,
  ) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_src(current_function_option, use_tuple, old_operand, new_operand);
  }

  pub fn replace_all_uses(&mut self, old: BOperand, new: BOperand) {
    let current_function_option = self.builder.current_function;
    self
      .ir_mut()
      .replace_all_uses(current_function_option, old, new);
  }

  pub fn alloc_slot(&mut self, slot: Slot) -> BOperand {
    let func = self.get_func_mut(self.current_func());
    BOperand::Slot(func.frame_info.alloc(slot))
  }

  pub fn get_operand_type(&self, operand: BOperand) -> BType {
    let func_id = self.current_func();

    match operand {
      BOperand::Inst(id) => self.get_func(func_id).dfg[id].typ.clone(),
      BOperand::Reg(reg) => match reg {
        Reg::X(_) => BType::I32,
        Reg::F(_) => BType::F32,
        Reg::Virt(_) => self.get_func(func_id).vregs[operand].typ.clone(),
      },
      BOperand::IntImm(_) => BType::I32,
      BOperand::FloatImm(_) => BType::F32,
      BOperand::Undef => BType::Void,

      BOperand::Slot(_) => match &self.get_func(func_id).frame_info[operand] {
        Slot::CalleeSaved { typ, .. }
        | Slot::Local { typ, .. }
        | Slot::Param { typ, .. }
        | Slot::Arg { typ, .. } => typ.clone(),
      },
      BOperand::Data(_) => self.ir().data_info[operand].typ.clone(),
      BOperand::RoData(_) => self.ir().rodata_info[operand].typ.clone(),
      BOperand::Bss(_) => self.ir().bss_info[operand].typ.clone(),

      BOperand::Func(_) | BOperand::BB(_) => unreachable!(),
    }
  }
}

pub trait BPass<'a> {
  /// Get the name of the pass, which will be used for logging and debugging purposes. It should be unique for each pass to avoid confusion in logs.
  fn name(&self) -> &str;
  /// mount the IR to the pass, which will be called before `run()`.
  fn mount(&mut self, program: &'a mut BackIR);
  /// run the pass on the mounted IR. The IR is guaranteed to be mounted before this method is called.
  fn run(&mut self);
}

pub struct BPassManager<'a> {
  // The lifetime 'a is tied to the IR that the passes will operate on.
  // The `+ 'a` bound is necessary because the passes themselves (like DCE<'a>)
  // contain a mutable reference to the IR with lifetime 'a.
  passes: VecDeque<Box<dyn BPass<'a> + 'a>>,
  cli: &'a Cli,
}

impl<'a> BPassManager<'a> {
  pub fn new(cli: &'a Cli) -> Self {
    BPassManager {
      passes: VecDeque::new(),
      cli,
    }
  }

  pub fn register(mut self, pass: Box<dyn BPass<'a> + 'a>) -> Self {
    self.passes.push_back(pass);
    self
  }

  pub fn run(mut self, ir: &'a mut BackIR) {
    let ir_ptr: *mut BackIR = ir;
    while let Some(mut pass) = self.passes.pop_front() {
      #[cfg(feature = "debug")]
      info!("Running backend pass: {}", pass.name());

      // SAFETY: Passes run sequentially and each pass only borrows `ir` during this iteration.
      unsafe { pass.mount(&mut *ir_ptr) };
      pass.run();

      #[cfg(feature = "debug")]
      info!("Finished backend pass: {}", pass.name());

      if self.cli.dump_asm_after == pass.name() {
        #[cfg(feature = "debug")]
        info!("Dumping assembly after backend pass: {}", pass.name());

        let filename = self
          .cli
          .output
          .as_ref()
          .and_then(|path| path.file_stem())
          .and_then(|s| s.to_str())
          .unwrap_or("output")
          .to_string();
        unsafe {
          DumpASM::new(&*ir_ptr, filename).run();
        }

        #[cfg(feature = "debug")]
        info!(
          "Finished dumping assembly after backend pass: {}",
          pass.name()
        );
        #[cfg(feature = "debug")]
        info!("Quit after dumping.");

        std::process::exit(0);
      }
    }

    #[cfg(feature = "debug")]
    info!("Start Dumping Assembly.");

    if let Some(output) = &self.cli.output {
      if let Err(e) = ir.dump_riscv_asm_to_file(output) {
        panic!("Error writing assembly output: {}", e);
      }
    } else {
      let asm_filename = "output".to_string();
      DumpASM::new(ir, asm_filename).run();
    }

    #[cfg(feature = "debug")]
    info!("Finish Dumping Assembly.");
  }
}
