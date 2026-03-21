//! Pass management for BackIR.

use crate::debug::info;
use crate::ir::back::BackIR;

use std::collections::VecDeque;

pub trait BPass<'a> {
    /// Get the name of the pass, which will be used for logging and debugging purposes. It should be unique for each pass to avoid confusion in logs.
    fn name(&self) -> &str;
    /// mount the IR to the pass, which will be called before `run()`.
    fn mount(&mut self, program: &'a mut BackIR);
    /// run the pass on the mounted IR. The IR is guaranteed to be mounted before this method is called.
    fn run(&mut self);
}

#[derive(Default)]
pub struct BPassManager<'a> {
    // The lifetime 'a is tied to the IR that the passes will operate on.
    // The `+ 'a` bound is necessary because the passes themselves (like DCE<'a>)
    // contain a mutable reference to the IR with lifetime 'a.
    passes: VecDeque<Box<dyn BPass<'a> + 'a>>,
}

impl<'a> BPassManager<'a> {
    pub fn register(mut self, pass: Box<dyn BPass<'a> + 'a>) -> Self {
        self.passes.push_back(pass);
        self
    }

    pub fn run(mut self, ir: &'a mut BackIR) {
        let ir_ptr: *mut BackIR = ir;
        while let Some(mut pass) = self.passes.pop_front() {
            info!("Running backend pass: {}", pass.name());
            // SAFETY: Passes run sequentially and each pass only borrows `ir` during this iteration.
            unsafe { pass.mount(&mut *ir_ptr) };
            pass.run();
            info!("Finished backend pass: {}", pass.name());
        }
    }
}
