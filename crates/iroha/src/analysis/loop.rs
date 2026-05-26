//! Loop Analysis, constructing loop forest and finding natural loops.
//! Referencing Cranelift's implementation: https://github.com/bytecodealliance/wasmtime/blob/9c0346ac6df9d4487565a5a7d24045cdc3754f5d/cranelift/codegen/src/loop_analysis.rs

use crate::analysis::DomAnalysis;

use yachiyo::analysis::{analyze, Analysis, DomTree, LoopData, LoopId, Loops};
use yachiyo::ir::mid::Operand;

pub struct LoopAnalysis<'a> {
  graph: &'a [(Vec<usize>, Vec<usize>)],
  /// LoopId -> LoopData
  loops: Loops,
  /// BBId -> LoopId
  block_to_loop: Vec<Option<LoopId>>,
}

impl LoopAnalysis<'_> {
  fn init(&mut self) {
    let cfg_len = self.graph.len();
    self.loops.clear();
    self.block_to_loop.clear();
    self.block_to_loop.resize(cfg_len, None);
  }

  fn dpo_rec(&self, order: &mut Vec<usize>, visited: &mut [bool], bb_id: usize) {
    if visited[bb_id] {
      return;
    }
    visited[bb_id] = true;

    for succ_id in &self.graph[bb_id].1 {
      self.dpo_rec(order, visited, *succ_id);
    }

    order.push(bb_id);
  }

  fn dpo(&self) -> Vec<usize> {
    let mut order = vec![];
    if self.graph.is_empty() {
      return order;
    }

    let mut visited = vec![false; self.graph.len()];
    self.dpo_rec(&mut order, &mut visited, 0);
    order
  }

  /// Traverse the CFG in reverse post-order and find natural loops.
  fn find_loop_header(&mut self, dom_tree: &DomTree) {
    let bbs_dpo = self.dpo();

    // RPO traversal
    for bb_id in bbs_dpo.into_iter().rev() {
      for pred_id in &self.graph[bb_id].0 {
        if dom_tree.is_dom(bb_id, *pred_id) {
          self.loops.push(LoopData::new(Operand::BB(bb_id)));
          self.block_to_loop[bb_id] = Some((self.loops.len() - 1).into());
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
        let header_id = lp.header.get_bb_id();
        stack.extend(
          self.graph[header_id]
            .0
            .iter()
            .filter(|pred_id| dom_tree.is_dom(header_id, **pred_id))
            .copied(),
        );
      }
      while let Some(node) = stack.pop() {
        // Add the block to the loop
        self.loops[lp_id.into()].blocks.insert(node);

        let continue_dfs: Option<usize>;
        match self.block_to_loop[node] {
          None => {
            // If the block is not assigned to any loop, it indicates that the block is part of lp.
            // Regular blocks of lp's inner loop should all have been assigned a loop.
            // Header blocks should have been assigned a loop in find_loop_header.
            self.block_to_loop[node] = Some(lp_id.into());
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
                  continue_dfs = Some(self.loops[node_loop].header.get_bb_id());
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
            self.graph[node]
              .0
              .iter()
              // Push all the preds, not the dominated one.
              .copied(),
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
            // level == 0 means we are not in a loop(INVALID), so we start from 1
            self.loops[*node_id].level = 1usize.into();
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
    for lp_id in (0..self.loops.len()).rev() {
      let loop_data = &mut self.loops[lp_id.into()];
      loop_data.owned_blocks = loop_data.blocks.clone();
    }
    for lp_id in (0..self.loops.len()).rev() {
      let lp_blocks = self.loops[lp_id.into()].blocks.clone();
      let lp_parent = self.loops[lp_id.into()].parent;
      if let Some(parent_id) = lp_parent {
        self.loops[parent_id].owned_blocks -= lp_blocks;
      }
    }
  }

  fn find_exit_blocks(&mut self) {
    for lp_id in 0..self.loops.len() {
      let lp = &mut self.loops[lp_id.into()];
      for block_id in lp.blocks.iter() {
        for succ_id in &self.graph[block_id].1 {
          if !lp.blocks.contains(*succ_id) {
            lp.exit_blocks.insert(*succ_id);
          }
        }
      }
    }
  }
}

impl<'a> Analysis for LoopAnalysis<'a> {
  type Input = &'a [(Vec<usize>, Vec<usize>)];
  type Output = (Loops, Vec<Option<LoopId>>);

  fn name() -> &'static str {
    "Loop Analysis"
  }
  fn new(input: Self::Input) -> Self {
    Self {
      graph: input,
      loops: Loops::default(),
      block_to_loop: Vec::new(),
    }
  }
  fn run(&mut self) -> Self::Output {
    let (dom_tree, _) = analyze::<DomAnalysis>(self.graph);
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
