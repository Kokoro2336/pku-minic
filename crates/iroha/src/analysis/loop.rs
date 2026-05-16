//! Loop Analysis, constructing loop forest and finding natural loops.
//! Referencing Cranelift's implementation: https://github.com/bytecodealliance/wasmtime/blob/9c0346ac6df9d4487565a5a7d24045cdc3754f5d/cranelift/codegen/src/loop_analysis.rs

use crate::analysis::DomAnalysis;

use yachiyo::analysis::{analyze, Analysis, DomTree, LoopData, LoopId, Loops};
use yachiyo::ir::mid::{Function, Operand};

pub struct LoopAnalysis<'a> {
  func: &'a Function,
  /// LoopId -> LoopData
  loops: Loops,
  /// BBId -> LoopId
  block_to_loop: Vec<Option<LoopId>>,
}

impl LoopAnalysis<'_> {
  fn init(&mut self) {
    let cfg_len = self.func.cfg.len();
    self.loops.clear();
    self.block_to_loop.clear();
    self.block_to_loop.resize(cfg_len, None);
  }

  /// Traverse the CFG in reverse post-order and find natural loops.
  fn find_loop_header(&mut self, dom_tree: &DomTree) {
    let bbs_dpo = self.func.cfg.dpo();

    // RPO traversal
    for bb_id in bbs_dpo.into_iter().rev() {
      let bb = &self.func.cfg[bb_id];
      for (pred_id, _) in &bb.preds {
        if dom_tree.is_dom(bb_id.get_bb_id(), pred_id.get_bb_id()) {
          self.loops.push(LoopData::new(bb_id));
          self.block_to_loop[bb_id.get_bb_id()] = Some((self.loops.len() - 1).into());
          break;
        }
      }
    }
  }

  fn discover_loop_blocks(&mut self, dom_tree: &DomTree) {
    let mut stack = vec![];
    // Traverse in DPO
    for lp_id in (0..self.loops.len()).rev() {
      // Start from the dominated pred of the header.
      {
        let lp = &self.loops[lp_id.into()];
        stack.extend(
          self.func.cfg[lp.header]
            .preds
            .iter()
            .filter(|(pred_id, _)| dom_tree.is_dom(lp.header.get_bb_id(), pred_id.get_bb_id()))
            .map(|(pred_id, _)| *pred_id),
        );
      }
      while let Some(node) = stack.pop() {
        // Add the block to the loop
        self.loops[lp_id.into()].blocks.insert(node.get_bb_id());

        let continue_dfs: Option<Operand>;
        match self.block_to_loop[node.get_bb_id()] {
          None => {
            // If the block is not assigned to any loop, it indicates that the block is part of lp.
            // Regular blocks of lp's inner loop should all have been assigned a loop.
            // Header blocks should have been assigned a loop in find_loop_header.
            self.block_to_loop[node.get_bb_id()] = Some(lp_id.into());
            // As the block is a regular block, the tracing should continue.
            continue_dfs = Some(node);
          }
          Some(mut node_loop) => {
            let mut node_loop_parent_option = self.loops[node_loop].parent;
            while let Some(node_loop_parent) = node_loop_parent_option {
              if node_loop_parent == lp_id.into() {
                break;
              } else {
                node_loop = node_loop_parent;
                node_loop_parent_option = self.loops[node_loop].parent;
              }
            }
            match node_loop_parent_option {
              None => {
                if node_loop == lp_id.into() {
                  // lp has been visited, stop tracing.
                  continue_dfs = None;
                } else {
                  // Unknown inner loop of lp
                  self.loops[node_loop].parent = Some(lp_id.into());
                  // Jump to the inner loop header to continue tracing, as the inner loop header should have been assigned a loop in find_loop_header.
                  continue_dfs = Some(self.loops[node_loop].header);
                }
              }
              Some(_) => {
                // Already known inner loop of lp, do nothing and stop tracing.
                continue_dfs = None;
              }
            }
          }
        }
        match continue_dfs {
          Some(node) => stack.extend(
            // Continue processing the preds of the block.
            self.func.cfg[node]
              .preds
              .iter()
              // Push all the preds, not the dominated one.
              .map(|(pred_id, _)| *pred_id),
          ),
          None => { /*No upstream block to process, do nothing*/ }
        }
      }
    }
  }

  fn assign_loop_level(&mut self) {
    let mut stack = vec![];
    for lp_id in 0..self.loops.len() {
      let lp = &self.loops[lp_id.into()];
      if lp.has_invalid_level() {
        stack.push(lp_id.into());
        while let Some(node_id) = stack.last() {
          if let Some(parent_id) = self.loops[*node_id].parent {
            if self.loops[parent_id].has_invalid_level() {
              stack.push(parent_id);
            } else {
              self.loops[*node_id].level =
                (usize::from(self.loops[parent_id].level) + 1usize).into();
              stack.pop();
            }
          } else {
            self.loops[*node_id].level = 0usize.into();
            stack.pop();
          }
        }
      }
    }
  }

  fn fill_loop_blocks(&mut self) {
    for lp_id in (0..self.loops.len()).rev() {
      let lp_parent = self.loops[lp_id.into()].parent;
      if let Some(parent_id) = lp_parent {
        let lp_blocks = self.loops[lp_id.into()].blocks.clone();
        let lp_parent = &mut self.loops[parent_id];
        lp_parent.blocks |= lp_blocks;
      }
    }
  }

  fn find_exit_blocks(&mut self) {
    for lp_id in 0..self.loops.len() {
      let lp = &mut self.loops[lp_id.into()];
      for block_id in lp.blocks.iter() {
        let block = &self.func.cfg[block_id];
        for (succ_id, _) in &block.succs {
          if !lp.blocks.contains(succ_id.get_bb_id()) {
            lp.exit_blocks.insert(succ_id.get_bb_id());
          }
        }
      }
    }
  }
}

impl<'a> Analysis for LoopAnalysis<'a> {
  type Input = &'a Function;
  type Output = (Loops, Vec<Option<LoopId>>);

  fn name() -> &'static str {
    "Loop Analysis"
  }
  fn new(input: Self::Input) -> Self {
    Self {
      func: input,
      loops: Loops::default(),
      block_to_loop: Vec::new(),
    }
  }
  fn run(&mut self) -> Self::Output {
    let (dom_tree, _) = analyze::<DomAnalysis>(self.func);
    self.init();
    self.find_loop_header(&dom_tree);
    self.discover_loop_blocks(&dom_tree);
    self.assign_loop_level();
    self.fill_loop_blocks();
    self.find_exit_blocks();
    (
      // The loops are naturally in a RPO order.
      std::mem::take(&mut self.loops),
      std::mem::take(&mut self.block_to_loop),
    )
  }
}
