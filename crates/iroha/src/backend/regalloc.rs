//! Register allocation (RegAlloc) via Graph Coloring.
//! Based on Appel and George's paper Iterated Register Coalescing.
//! Reference: https://dl.acm.org/doi/10.1145/229542.229546

use crate::analysis::LiveAnalysis;
use yachiyo::analysis::analyze;
use yachiyo::ir::back::BackIR;
use yachiyo::pass::BPass;

pub struct RegAlloc<'a> {
    ir: Option<&'a mut BackIR>,
}

impl<'a> BPass<'a> for RegAlloc<'a> {
    fn name(&self) -> &str {
        "Register Allocation"
    }

    fn mount(&mut self, ir: &'a mut BackIR) {
        self.ir = Some(ir);
    }

    fn run(&mut self) {
        let ir = self.ir.as_mut().unwrap();
        let (live_ins, live_outs) = analyze::<LiveAnalysis>(ir);
    }
}
