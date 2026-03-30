//! Liveness Analysis based on iterative dataflow analysis, referencing Cranelift's implementation.
//! Reference: https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/frontend/src/frontend/safepoints.rs

use yachiyo::analysis::Analysis;
use yachiyo::ir::back::{BOperand, BackIR};
use yachiyo::utils::set::ArraySet;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

pub type LiveSet = ArraySet<BOperand>;
pub type LiveIns = Vec<LiveSet>;
pub type LiveOuts = Vec<LiveSet>;

#[derive(Default)]
pub struct LiveAnalysis<'a> {
    ir: Option<&'a BackIR>,

    current_function: Option<BOperand>,

    /// The live set for the current block being processed.
    current_live: LiveSet,

    // Ancillary structures
    dfs_post_order: Worklist<BOperand, BitSet>,
    visited: BitSet,

    /// LiveIn result
    live_ins: LiveIns,
    /// LiveOut result
    live_outs: LiveOuts,
}

impl LiveAnalysis<'_> {
    pub fn new() -> Self {
        Self::default()
    }

    fn dfs(&mut self, bb_id: BOperand) {
        if self.visited.contains(bb_id.get_bb_id()) {
            return;
        }

        self.visited.insert(bb_id.get_bb_id());

        let func_id = self.current_function.unwrap();
        let ir = self.ir.unwrap();
        let bb = &ir.funcs[func_id].cfg[bb_id];
        for (succ, _) in &bb.succs {
            self.dfs(*succ);
        }

        // Post-order traversal.
        self.dfs_post_order.push_back(bb_id);
    }

    fn init(&mut self, func_id: BOperand) {
        self.current_function = Some(func_id);
        let cfg_len = self.ir.unwrap().funcs[func_id].cfg.len();

        // Clear and resize live_ins and live_outs.
        self.live_ins.clear();
        self.live_outs.clear();
        self.live_ins.resize(cfg_len, LiveSet::new());
        self.live_outs.resize(cfg_len, LiveSet::new());

        self.dfs_post_order.clear();
        self.visited.clear();

        self.current_live.clear();
    }

    #[inline(always)]
    fn get_rd(&self, op_id: BOperand) -> Option<BOperand> {
        let func_id = self.current_function;
        self.ir.unwrap().get_rd(func_id, op_id)
    }

    #[inline(always)]
    fn get_src(&self, op_id: BOperand) -> Vec<BOperand> {
        let func_id = self.current_function;
        self.ir.unwrap().get_src(func_id, op_id)
    }

    #[inline(always)]
    fn process_def(&mut self, op_id: BOperand) {
        let def = self.get_rd(op_id);
        if let Some(def) = def {
            self.current_live.remove(&def);
        }
    }

    #[inline(always)]
    fn process_use(&mut self, op_id: BOperand) {
        let uses = self.get_src(op_id);
        for use_id in uses {
            // Live analysis only cares about registers.
            if !matches!(use_id, BOperand::Reg(_)) {
                continue;
            }
            self.current_live.insert(use_id);
        }
    }

    fn process_block(&mut self, bb_id: BOperand) {
        // Initialize current_live with live_outs of the block.
        self.current_live.clear();
        self.current_live
            .extend(self.live_outs[bb_id.get_bb_id()].iter().cloned());

        // Process instructions in reverse order.
        let func_id = self.current_function.unwrap();
        let ir = self.ir.unwrap();
        let bb = &ir.funcs[func_id].cfg[bb_id];
        for op_id in bb.cur.iter().rev() {
            // Process defs first, then uses.
            self.process_def(*op_id);
            self.process_use(*op_id);
        }
    }

    fn analyze(&mut self) -> (LiveIns, LiveOuts) {
        let func_id = self
            .current_function
            .expect("LiveAnalysis analyze: no current function");
        while let Some(bb_id) = self.dfs_post_order.pop_front() {
            let old_live_in_len = self.live_ins[bb_id.get_bb_id()].len();

            // Update live_outs of the block based on live_ins of its successors.
            let ir = self.ir.unwrap();
            let bb = &ir.funcs[func_id].cfg[bb_id];
            for (succ, _) in &bb.succs {
                let succ_live_in = &self.live_ins[succ.get_bb_id()];
                // Get the union of live_outs of the block and live_ins of its successor.
                self.live_outs[bb_id.get_bb_id()] =
                    self.live_outs[bb_id.get_bb_id()].union(succ_live_in);
            }

            // Process the block to update live_ins.
            self.process_block(bb_id);

            // Update live_ins of the block with current_live.
            self.live_ins[bb_id.get_bb_id()] = std::mem::take(&mut self.current_live);

            // If the live-in set changes, we need to reprocess the predecessors.
            if old_live_in_len != self.live_ins[bb_id.get_bb_id()].len() {
                let ir = self.ir.unwrap();
                let bb = &ir.funcs[func_id].cfg[bb_id];
                for (pred, _) in &bb.preds {
                    self.dfs_post_order.push_back(*pred);
                }
            }
        }
        (
            std::mem::take(&mut self.live_ins),
            std::mem::take(&mut self.live_outs),
        )
    }
}

impl<'a> Analysis<'a> for LiveAnalysis<'a> {
    type Input = BackIR;
    type Output = (Vec<LiveIns>, Vec<LiveOuts>);

    fn name(&self) -> &'static str {
        "Live Analysis"
    }

    fn mount(&mut self, ir: &'a Self::Input) {
        self.ir = Some(ir);
    }

    fn run(&mut self) -> Self::Output {
        // resize live_ins and live_outs
        let mut live_ins = Vec::new();
        let mut live_outs = Vec::new();

        for func_id in self.ir.unwrap().funcs.ids() {
            self.init(BOperand::Func(func_id));
            let entry = self.ir.unwrap().funcs[func_id]
                .cfg
                .entry
                .expect("No entry for current function.");
            self.dfs(BOperand::BB(entry)); // Assuming entry block is always BB(0)

            // Run live analysis
            let (func_live_ins, func_live_outs) = self.analyze();

            live_ins.push(func_live_ins);
            live_outs.push(func_live_outs);
        }

        (live_ins, live_outs)
    }
}
