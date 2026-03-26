//! Live Analysis based on dataflow iteration.

use yachiyo::analysis::Analysis;
use yachiyo::ir::back::{BOperand, BackIR};
use yachiyo::utils::bitset::BitSet;
use yachiyo::utils::set::Set;

pub type LiveSet = Set<BOperand>;
pub type LiveIns = Vec<LiveSet>;
pub type LiveOuts = Vec<LiveSet>;

#[derive(Default)]
pub struct LiveAnalysis<'a> {
    ir: Option<&'a BackIR>,

    current_function: Option<BOperand>,
    current_live: LiveSet,

    // Ancillary structures
    dfs_post_order: Vec<BOperand>,
    dfs_visited: BitSet,

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
        if !self.dfs_visited.insert(bb_id.get_bb_id()) {
            return;
        }

        let func_id = self.current_function.unwrap();
        let ir = self.ir.unwrap();
        let bb = &ir.funcs[func_id].cfg[bb_id];
        for succ in &bb.succs {
            self.dfs(succ.to_owned());
        }

        // Post-order traversal.
        self.dfs_post_order.push(bb_id);
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
        self.dfs_visited.clear();

        self.current_live.clear();
    }

    fn analyze(&mut self) -> (LiveIns, LiveOuts) {
        while let Some(bb_id) = self.dfs_post_order.pop() {}
        (
            std::mem::take(&mut self.live_ins),
            std::mem::take(&mut self.live_outs),
        )
    }
}

impl<'a> Analysis<'a> for LiveAnalysis<'a> {
    type Input = BackIR;
    type Output = (LiveIns, LiveOuts);

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

        (live_ins, live_outs)
    }
}
