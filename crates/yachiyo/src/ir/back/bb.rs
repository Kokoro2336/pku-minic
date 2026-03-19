use crate::ir::back::BOperand;
use crate::utils::arena::IndexedArena;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type BCFG = IndexedArena<BBasicBlock>;

#[derive(Debug, Clone, Default)]
pub struct BBasicBlock {
    pub prev: Vec<BOperand>,
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
        if !self[bb_idx.get_bb_id()].prev.contains(&pred_idx) {
            self[bb_idx.get_bb_id()].prev.push(pred_idx);
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
            .prev
            .iter()
            .position(|x| *x == pred_idx)
        {
            self[bb_idx.get_bb_id()].prev.swap_remove(pos);
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
