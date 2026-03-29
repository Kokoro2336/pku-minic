use crate::debug::info;
use crate::ir::back::BOperand;
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type BCFG = IndexedArena<BBasicBlock>;

#[derive(Debug, Clone, Default)]
pub struct BBasicBlock {
    pub preds: Vec<BOperand>,
    pub cur: Vec<BOperand>,
    pub succs: Vec<BOperand>,
}

impl BCFG {
    pub fn add_succ(&mut self, bb_idx: BOperand, succ_idx: BOperand) {
        if !self[bb_idx.get_bb_id()].succs.contains(&succ_idx) {
            self[bb_idx.get_bb_id()].succs.push(succ_idx);
        }
    }

    pub fn add_pred(&mut self, bb_idx: BOperand, pred_idx: BOperand) {
        if !self[bb_idx.get_bb_id()].preds.contains(&pred_idx) {
            self[bb_idx.get_bb_id()].preds.push(pred_idx);
        }
    }

    pub fn remove_succ(&mut self, bb_idx: BOperand, succ_idx: BOperand) {
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

    pub fn remove_pred(&mut self, bb_idx: BOperand, pred_idx: BOperand) {
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
                for pred in bb.preds.iter_mut() {
                    remap_bb(pred);
                }
                for succ in bb.succs.iter_mut() {
                    remap_bb(succ);
                }
            }
        }

        old_arena
    }
}
