//! Loop Analysis, constructing loop forest and finding natural loops.
//! Referencing Cranelift's implementation: https://github.com/bytecodealliance/wasmtime/blob/9c0346ac6df9d4487565a5a7d24045cdc3754f5d/cranelift/codegen/src/loop_analysis.rs

use crate::analysis::dom::{DomAnalysis, DomTree};

use yachiyo::analysis::{analyze, Analysis};
use yachiyo::ir::mid::{Function, Operand};
use yachiyo::utils::worklist::WorklistTrait;

/// Loop level, 0 for non-loop blocks, 1 for innermost loops, etc.
pub type LoopLevel = usize;
/// LoopId
type LoopId = usize;

pub struct LoopData {
  pub header: Operand,
  pub parent: Option<LoopId>,
  pub level: LoopLevel,
}

const INVALID_LOOP_LEVEL: LoopLevel = usize::MAX;
const ROOT_LEVEL: LoopLevel = 0;

impl LoopData {
  fn new(header: Operand) -> Self {
    Self {
      header,
      parent: None,
      level: INVALID_LOOP_LEVEL,
    }
  }
  #[inline(always)]
  fn has_invalid_level(&self) -> bool {
    self.level == INVALID_LOOP_LEVEL
  }
}

#[derive(Default)]
pub struct LoopAnalysis<'a> {
  func: Option<&'a Function>,
  /// LoopId -> LoopData
  loops: Vec<LoopData>,
  /// BBId -> LoopId
  block_to_loop: Vec<Option<LoopId>>,
}

impl LoopAnalysis<'_> {
  fn init(&mut self) {
    let cfg_len = self.func.unwrap().cfg.len();
    self.loops.clear();
    self.block_to_loop.clear();
    self.block_to_loop.resize(cfg_len, None);
  }

  fn is_idom(dom_tree: &DomTree, u: usize, v: usize) -> bool {
    dom_tree[u].contains(&v)
  }

  /// Traverse the CFG in reverse post-order and find natural loops.
  fn find_loop_header(&mut self, dom_tree: &DomTree) {
    let mut bbs_dpo = self.func.unwrap().dpo();

    // RPO traversal
    while let Some(bb_id) = bbs_dpo.pop_back() {
      let bb = &self.func.unwrap().cfg[bb_id];
      for (pred_id, _) in &bb.preds {
        if Self::is_idom(dom_tree, bb_id.get_bb_id(), pred_id.get_bb_id()) {
          self.loops.push(LoopData::new(bb_id));
          self.block_to_loop[bb_id.get_bb_id()] = Some(self.loops.len() - 1);
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
        let lp = &self.loops[lp_id];
        stack.extend(
          self.func.unwrap().cfg[lp.header]
            .preds
            .iter()
            .filter(|(pred_id, _)| {
              Self::is_idom(dom_tree, lp.header.get_bb_id(), pred_id.get_bb_id())
            })
            .map(|(pred_id, _)| *pred_id),
        );
      }
      while let Some(node) = stack.pop() {
        let continue_dfs: Option<Operand>;
        match self.block_to_loop[node.get_bb_id()] {
          None => {
            // If the block is not assigned to any loop, it indicates that the block is part of lp.
            // Regular blocks of lp's inner loop should all have been assigned a loop.
            // Header blocks should have been assigned a loop in find_loop_header.
            self.block_to_loop[node.get_bb_id()] = Some(lp_id);
            // As the block is a regular block, the tracing should continue.
            continue_dfs = Some(node);
          }
          Some(mut node_loop) => {
            let mut node_loop_parent_option = self.loops[node_loop].parent;
            while let Some(node_loop_parent) = node_loop_parent_option {
              if node_loop_parent == lp_id {
                break;
              } else {
                node_loop = node_loop_parent;
                node_loop_parent_option = self.loops[node_loop].parent;
              }
            }
            match node_loop_parent_option {
              None => {
                if node_loop == lp_id {
                  // lp has been visited, stop tracing.
                  continue_dfs = None;
                } else {
                  // Unknown inner loop of lp
                  self.loops[node_loop].parent = Some(lp_id);
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
            self.func.unwrap().cfg[node]
              .preds
              .iter()
              .filter(|(pred_id, _)| {
                let lp = &self.loops[lp_id];
                Self::is_idom(dom_tree, lp.header.get_bb_id(), pred_id.get_bb_id())
              })
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
      let lp = &self.loops[lp_id];
      if lp.has_invalid_level() {
        stack.push(lp_id);
        while let Some(node_id) = stack.last() {
          if let Some(paren_id) = self.loops[*node_id].parent {
            if self.loops[paren_id].has_invalid_level() {
              stack.push(paren_id);
            } else {
              self.loops[*node_id].level = self.loops[paren_id].level + 1;
              stack.pop();
            }
          } else {
            self.loops[*node_id].level = ROOT_LEVEL;
            stack.pop();
          }
        }
      }
    }
  }
}

impl<'a> Analysis<'a> for LoopAnalysis<'a> {
  type Input = Function;
  type Output = Vec<LoopData>;

  fn name(&self) -> &str {
    "Loop Analysis"
  }
  fn mount(&mut self, func: &'a Self::Input) {
    self.func = Some(func);
  }
  fn run(&mut self) -> Self::Output {
    let (dom_tree, _) = analyze::<DomAnalysis>(self.func.unwrap());
    self.init();
    self.find_loop_header(&dom_tree);
    self.discover_loop_blocks(&dom_tree);
    self.assign_loop_level();
    std::mem::take(&mut self.loops)
  }
}
