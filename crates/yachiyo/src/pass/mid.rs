//! Pass management for IR optimization and transformation.

use crate::cli::Cli;
#[cfg(feature = "debug")]
use crate::debug::info;
use crate::debug::DumpLLVM;
use crate::ir::mid::IR;

use std::collections::VecDeque;

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
          .file_stem()
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
    if self.cli.emit_llvm && self.cli.dump_llvm_after.is_empty() {
      #[cfg(feature = "debug")]
      info!("Start Dumping LLVM IR.");
      let filename = self
        .cli
        .output
        .file_stem()
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
      std::process::exit(0)
    }
  }
}
