//! Basic Block definition for BackIR.

#[cfg(feature = "debug")]
use crate::debug::info;
use crate::ir::back::BOperand;
use crate::utils::{Arena, ArenaItem, BitSet, IndexedArena};

use std::ops::{Deref, DerefMut, Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Default)]
pub struct BCFG(IndexedArena<BBasicBlock>);

impl BCFG {
  pub fn new() -> Self {
    Self(IndexedArena::new())
  }
}

impl Deref for BCFG {
  type Target = IndexedArena<BBasicBlock>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for BCFG {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone, Default)]
pub struct BBasicBlock {
  /// (Predecessor block, Terminator InstId)
  pub preds: Vec<(BOperand, BOperand)>,
  pub cur: Vec<BOperand>,
  pub succs: Vec<(BOperand, BOperand)>,
}

impl BCFG {
  fn dpo_rec(&self, order: &mut Vec<usize>, visited: &mut BitSet, bb_id: BOperand) {
    if visited.contains(bb_id.get_bb_id()) {
      return;
    }
    visited.insert(bb_id.get_bb_id());

    let bb = &self[bb_id];
    for (succ, _) in &bb.succs {
      self.dpo_rec(order, visited, *succ);
    }

    // Post-order traversal.
    order.push(bb_id.get_bb_id());
  }

  pub fn dpo(&self) -> Vec<usize> {
    let mut order = vec![];
    let mut visited = BitSet::new();
    let entry = BOperand::BB(self.entry.unwrap());
    self.dpo_rec(&mut order, &mut visited, entry);
    order
  }

  pub fn add_succ(&mut self, bb_idx: BOperand, succ_idx: (BOperand, BOperand)) {
    self[bb_idx.get_bb_id()].succs.push(succ_idx);
  }

  pub fn add_pred(&mut self, bb_idx: BOperand, pred_idx: (BOperand, BOperand)) {
    self[bb_idx.get_bb_id()].preds.push(pred_idx);
  }

  pub fn remove_succ(&mut self, bb_idx: BOperand, succ_idx: (BOperand, BOperand)) {
    if let Some(pos) = self[bb_idx.get_bb_id()]
      .succs
      .iter()
      .position(|x| *x == succ_idx)
    {
      self[bb_idx.get_bb_id()].succs.swap_remove(pos);
    } else {
      panic!(
        "Remove succ {:?}: not found in succs of block {:?}: {:?}",
        succ_idx,
        bb_idx,
        self[bb_idx.get_bb_id()]
      );
    }
  }

  pub fn remove_pred(&mut self, bb_idx: BOperand, pred_idx: (BOperand, BOperand)) {
    if let Some(pos) = self[bb_idx.get_bb_id()]
      .preds
      .iter()
      .position(|x| *x == pred_idx)
    {
      self[bb_idx.get_bb_id()].preds.swap_remove(pos);
    } else {
      panic!(
        "Remove pred {:?}: not found in preds of block {:?}: {:?}",
        pred_idx,
        bb_idx,
        self[bb_idx.get_bb_id()]
      );
    }
  }
}

impl Index<BOperand> for BCFG {
  type Output = BBasicBlock;

  fn index(&self, index: BOperand) -> &Self::Output {
    match index {
      BOperand::BB(id) => self.get(id).unwrap(),
      _ => panic!("BCFG index: expected BOperand::BB, got {:?}", index),
    }
  }
}

impl IndexMut<BOperand> for BCFG {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::BB(id) => self.get_mut(id).unwrap(),
      _ => panic!("BCFG index_mut: expected BOperand::BB, got {:?}", index),
    }
  }
}

impl Index<usize> for BCFG {
  type Output = BBasicBlock;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for BCFG {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

impl Arena<BBasicBlock> for BCFG {
  fn remove(&mut self, idx: usize) -> BBasicBlock {
    if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
      data
    } else {
      panic!("BCFG remove: index {} points to None or NewIndex", idx);
    }
  }
  fn gc(&mut self) -> Vec<ArenaItem<BBasicBlock>> {
    let new_arena: Vec<ArenaItem<BBasicBlock>> = vec![];
    let mut old_arena = std::mem::replace(&mut self.storage, new_arena);

    // Transport
    old_arena.iter_mut().for_each(|item| {
      if matches!(item, ArenaItem::Data(_)) {
        let new_idx = self.storage.len();
        let data = item.replace(new_idx);
        self.storage.push(data);
      }
    });

    #[cfg(feature = "debug")]

    info!(
      "BCFG GC: {} basic blocks collected, recycle rate: {:.2}%",
      old_arena.len() - self.storage.len(),
      (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
    );

    let remap_idx = |idx: &mut usize, old_arena: &Vec<ArenaItem<BBasicBlock>>| {
      *idx = match old_arena.get(*idx) {
        Some(ArenaItem::NewIndex(new_idx)) => *new_idx,
        _ => panic!("BCFG gc: index {} in BBasicBlock not found", *idx),
      };
    };

    if let Some(entry) = self.entry.as_mut() {
      remap_idx(entry, &old_arena);
    }

    for idx in self.map.values_mut() {
      remap_idx(idx, &old_arena);
    }

    let remap_bb = |bb_idx: &mut BOperand| {
      let old_idx = bb_idx.get_bb_id();
      *bb_idx = match old_arena.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => BOperand::BB(*new_idx),
        _ => panic!("BCFG gc: BB index {} in BBasicBlock not found", old_idx),
      };
    };

    // rewrite idx in preds and succs
    for item in self.storage.iter_mut() {
      if let ArenaItem::Data(bb) = item {
        for (pred, _) in bb.preds.iter_mut() {
          remap_bb(pred);
        }
        for (succ, _) in bb.succs.iter_mut() {
          remap_bb(succ);
        }
      }
    }

    old_arena
  }
}
