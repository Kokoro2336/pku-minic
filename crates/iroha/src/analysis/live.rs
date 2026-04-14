//! Liveness Analysis based on iterative dataflow analysis, referencing Cranelift's implementation.
//! Reference: https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/frontend/src/frontend/safepoints.rs

use yachiyo::analysis::Analysis;
use yachiyo::ir::back::{get_clobbered, BAttr, BFunction, BOperand, Reg};
use yachiyo::utils::set::ArraySet;
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

pub type LiveSet = ArraySet<BOperand>;
pub type LiveIns = Vec<LiveSet>;
pub type LiveOuts = Vec<LiveSet>;

#[derive(Default)]
pub struct LiveAnalysis<'a> {
  func: Option<&'a BFunction>,

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

    let bb = &self.func.expect("No current func").cfg[bb_id];
    for (succ, _) in &bb.succs {
      self.dfs(*succ);
    }

    // Post-order traversal.
    self.dfs_post_order.push_back(bb_id);
  }

  fn init(&mut self) {
    let cfg_len = self.func.expect("No current func").cfg.len();

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
    self.func.unwrap().get_rd(op_id).cloned()
  }

  #[inline(always)]
  fn get_src(&self, op_id: BOperand) -> Vec<BOperand> {
    self
      .func
      .unwrap()
      .get_src(op_id)
      .into_iter()
      .cloned()
      .collect()
  }

  #[inline(always)]
  fn process_def(&mut self, op_id: BOperand) {
    let def = self.get_rd(op_id);
    let op = &self.func.unwrap().dfg[op_id];
    // If the instruction has an implicit def, remove it from the live set.
    op.attrs
      .iter()
      .find(|attr| matches!(attr, BAttr::ImplicitDef(_)))
      .and_then(|implicit_def| {
        if let BAttr::ImplicitDef(implicit_def_op) = implicit_def {
          // Remove implicit def operand from live set.
          self.current_live.remove(implicit_def_op);
          Some(())
        } else {
          None
        }
      });
    op.attrs
      .iter()
      .find(|attr| matches!(attr, BAttr::Clobber))
      .and_then(|clobber| {
        if let BAttr::Clobber = clobber {
          // If the instruction has a clobber attribute, remove all clobbered registers from the live set.
          let clobbered_regs = get_clobbered::<ArraySet<Reg>>()
            .into_iter()
            .map(BOperand::Reg);
          for reg in clobbered_regs {
            self.current_live.remove(&reg);
          }
          Some(())
        } else {
          None
        }
      });
    if let Some(def) = def {
      self.current_live.remove(&def);
    }
  }

  #[inline(always)]
  fn process_use(&mut self, op_id: BOperand) {
    let mut uses = self.get_src(op_id);
    let op = &self.func.unwrap().dfg[op_id];
    op.attrs
      .iter()
      .find(|attr| matches!(attr, BAttr::ImplicitUse(_)))
      .and_then(|implicit_use| {
        if let BAttr::ImplicitUse(implicit_src) = implicit_use {
          // Add implicit use operands to uses.
          uses.extend(implicit_src);
          Some(())
        } else {
          None
        }
      });
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
    self
      .current_live
      .extend(self.live_outs[bb_id.get_bb_id()].iter().cloned());

    // Process instructions in reverse order.
    let bb = &self.func.expect("No current function").cfg[bb_id];
    for op_id in bb.cur.iter().rev() {
      // Process defs first, then uses.
      self.process_def(*op_id);
      self.process_use(*op_id);
    }
  }
}

impl<'a> Analysis<'a> for LiveAnalysis<'a> {
  type Input = BFunction;
  type Output = (LiveIns, LiveOuts);

  fn name(&self) -> &'static str {
    "Live Analysis"
  }

  fn mount(&mut self, func: &'a Self::Input) {
    self.func = Some(func);
  }

  fn run(&mut self) -> Self::Output {
    self.init();
    let entry = self
      .func
      .unwrap()
      .cfg
      .entry
      .expect("No entry for current function.");
    self.dfs(BOperand::BB(entry));

    // Run main loop
    while let Some(bb_id) = self.dfs_post_order.pop_front() {
      let old_live_in_len = self.live_ins[bb_id.get_bb_id()].len();

      // Update live_outs of the block based on live_ins of its successors.
      let bb = &self.func.expect("No current function").cfg[bb_id];
      for (succ, _) in &bb.succs {
        let succ_live_in = &self.live_ins[succ.get_bb_id()];
        // Get the union of live_outs of the block and live_ins of its successor.
        self.live_outs[bb_id.get_bb_id()] = self.live_outs[bb_id.get_bb_id()].union(succ_live_in);
      }

      // Process the block to update live_ins.
      self.process_block(bb_id);

      // Update live_ins of the block with current_live.
      self.live_ins[bb_id.get_bb_id()] = std::mem::take(&mut self.current_live);

      // If the live-in set changes, we need to reprocess the predecessors.
      if old_live_in_len != self.live_ins[bb_id.get_bb_id()].len() {
        let bb = &self.func.expect("No current function").cfg[bb_id];
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
