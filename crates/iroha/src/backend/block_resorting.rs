//! Block Resorting via Pittis-Hansen Algorithm.
//! Reference: https://dl.acm.org/doi/10.1145/93548.93550

use yachiyo::analysis::{analyze, LoopId, Loops};
use yachiyo::ir::back::{BOpData, BOperand, BackIR, LOpData, MOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::{ArenaItem, BitSet, Worklist};

use crate::analysis::LoopAnalysis;

use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[derive(Default)]
pub struct BlockResorting<'a> {
  cx: BPassContext<'a>,
}

impl BlockResorting<'_> {
  fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  fn resort(&self, loops: &Loops, block_to_loops: &[Option<LoopId>]) -> Vec<BOperand> {
    let func_id = self.cx.get_current_func_id();
    let mut edges: FxHashMap<(BOperand, BOperand), f64> = FxHashMap::default();
    let mut worklist: Worklist<BOperand, BitSet> = Worklist::default();
    worklist.push_back(self.cx.get_entry(func_id));

    // Collect edges and calculat the weights.
    while let Some(bb_id) = worklist.pop_front() {
      let succs = self.cx.get_bb(bb_id).succs.clone();
      let bb_lp_id = block_to_loops[bb_id.get_bb_id()];

      let succ_len = succs.len();
      let weight = if succ_len == 0 {
        continue;
      } else {
        1_f64 / succ_len as f64
      };

      for (succ, _) in succs {
        if edges.insert((bb_id, succ), weight).is_some() {
          continue;
        }
        if let Some(bb_lp_id) = bb_lp_id {
          let lp_data = &loops[bb_lp_id];
          if lp_data.blocks.contains(succ.get_bb_id()) {
            // If successor is also in the same loop, add weight * loop_level ^ 10 to the edge.
            let lp_level = usize::from(lp_data.level) as f64;
            if lp_level != 0.0 {
              *edges.get_mut(&(bb_id, succ)).unwrap() *= lp_level.powi(10);
            }
          }
        }
        worklist.push_back(succ);
      }
    }

    // Sort edges by weight in descending order.
    let mut sorted_edges: Vec<((BOperand, BOperand), f64)> = edges.into_iter().collect();
    sorted_edges.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let sorted_edges: Vec<(BOperand, BOperand)> =
      sorted_edges.into_iter().map(|(edge, _)| edge).collect();

    // Resort blocks.
    let mut chains = vec![vec![]; self.cx.get_cfg().len()];
    for bb_id in self.cx.get_bbs() {
      chains[bb_id.get_bb_id()].push(bb_id);
    }
    let mut seq = vec![];
    // BBId -> Current chain head.
    let mut heads: Vec<BOperand> = (0..chains.len()).map(BOperand::BB).collect();
    let mut chain_key = vec![usize::MAX; chains.len()];
    let mut merge_order = 0usize;
    fn find(heads: &mut [BOperand], bb_id: BOperand) -> BOperand {
      if bb_id == heads[bb_id.get_bb_id()] {
        bb_id
      } else {
        let head = find(heads, heads[bb_id.get_bb_id()]);
        heads[bb_id.get_bb_id()] = head;
        head
      }
    }

    // Merge chains.
    for (src, dst) in sorted_edges {
      let src_head = find(&mut heads, src);
      let dst_head = find(&mut heads, dst);
      if src_head == dst_head {
        continue;
      }
      let src_head_idx = src_head.get_bb_id();
      let dst_head_idx = dst_head.get_bb_id();

      let (src_chain, dst_chain) = if src_head_idx < dst_head_idx {
        let (left, right) = chains.split_at_mut(dst_head_idx);
        (&mut left[src_head_idx], &mut right[0])
      } else {
        let (left, right) = chains.split_at_mut(src_head_idx);
        (&mut right[0], &mut left[dst_head_idx])
      };

      if src_chain.last() == Some(&src) && dst_chain.first() == Some(&dst) {
        // Merge src chain and dst chain.
        merge_order += 1;
        chain_key[src_head_idx] = chain_key[src_head_idx]
          .min(chain_key[dst_head_idx])
          .min(merge_order);
        src_chain.append(dst_chain);
        heads[dst_head_idx] = src_head;
      }
    }

    let mut inserted: BitSet = BitSet::new();
    let mut queued: BitSet = BitSet::new();
    let mut heap = BinaryHeap::new();

    let entry_head = find(&mut heads, self.cx.get_entry(func_id));
    let entry_head_idx = entry_head.get_bb_id();
    queued.insert(entry_head_idx);
    heap.push(Reverse((chain_key[entry_head_idx], entry_head_idx)));

    while let Some(Reverse((_, head_idx))) = heap.pop() {
      let chain = chains[head_idx].clone();

      for bb_id in &chain {
        if inserted.insert(bb_id.get_bb_id()) {
          seq.push(*bb_id);
        }
      }

      for bb_id in chain {
        let succs = self.cx.get_bb(bb_id).succs.clone();
        for (succ, _) in succs {
          if inserted.contains(succ.get_bb_id()) {
            continue;
          }

          let succ_head = find(&mut heads, succ);
          let succ_head_idx = succ_head.get_bb_id();
          if queued.insert(succ_head_idx) {
            heap.push(Reverse((chain_key[succ_head_idx], succ_head_idx)));
          }
        }
      }
    }

    seq
  }

  fn rewrite(&mut self, seq: Vec<BOperand>) {
    let func_id = self.cx.get_current_func_id();
    let func = self.cx.get_func_mut(func_id);
    let cfg_len = func.cfg.len();
    let mut seen = vec![false; cfg_len];
    let mut order = vec![];

    for bb_id in seq {
      let old_idx = bb_id.get_bb_id();
      if !seen[old_idx] {
        seen[old_idx] = true;
        order.push(old_idx);
      }
    }
    for bb_id in func.cfg.collect() {
      if !seen[bb_id] {
        seen[bb_id] = true;
        order.push(bb_id);
      }
    }

    let mut old_storage = std::mem::take(&mut func.cfg.storage);
    let mut remap = vec![usize::MAX; cfg_len];
    for old_idx in order {
      let new_idx = func.cfg.storage.len();
      remap[old_idx] = new_idx;
      let bb = match old_storage[old_idx].replace(new_idx) {
        ArenaItem::Data(bb) => bb,
        _ => panic!("BlockResorting rewrite: block {} is not data", old_idx),
      };
      func.cfg.storage.push(ArenaItem::Data(bb));
    }

    let remap_idx = |idx: &mut usize| {
      *idx = remap[*idx];
    };
    if let Some(entry) = func.cfg.entry.as_mut() {
      remap_idx(entry);
    }
    for idx in func.cfg.map.values_mut() {
      remap_idx(idx);
    }

    let remap_bb = |bb_idx: &mut BOperand| {
      let old_idx = bb_idx.get_bb_id();
      *bb_idx = BOperand::BB(remap[old_idx]);
    };

    for item in func.cfg.storage.iter_mut() {
      if let ArenaItem::Data(bb) = item {
        for (pred_id, _) in bb.preds.iter_mut() {
          remap_bb(pred_id);
        }
        for (succ_id, _) in bb.succs.iter_mut() {
          remap_bb(succ_id);
        }
      }
    }

    for item in func.dfg.storage.iter_mut() {
      if let ArenaItem::Data(op) = item {
        match &mut op.data {
          BOpData::L(LOpData::Br {
            then_bb, else_bb, ..
          }) => {
            remap_bb(then_bb);
            remap_bb(else_bb);
          }
          BOpData::L(LOpData::Jump { target_bb }) => {
            remap_bb(target_bb);
          }
          BOpData::M(MOpData::J { target }) => {
            remap_bb(target);
          }
          BOpData::M(MOpData::Bnez { target, .. } | MOpData::Beqz { target, .. }) => {
            remap_bb(target);
          }
          BOpData::M(
            MOpData::Beq { offset, .. }
            | MOpData::Bne { offset, .. }
            | MOpData::Blt { offset, .. }
            | MOpData::Bge { offset, .. }
            | MOpData::Bltu { offset, .. }
            | MOpData::Bgeu { offset, .. },
          ) => {
            remap_bb(offset);
          }
          _ => {}
        }
      }
    }

    func.rebuild_op_to_bb();
  }
}

impl<'a> BPass<'a> for BlockResorting<'a> {
  fn name(&self) -> &str {
    "BlockResorting"
  }

  fn mount(&mut self, program: &'a mut BackIR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      let graph = self.cx.extract_cfg();
      let (loops, block_to_loops) = analyze::<LoopAnalysis>(&graph);
      let seq = self.resort(&loops, &block_to_loops);
      self.rewrite(seq);
    }
  }
}
