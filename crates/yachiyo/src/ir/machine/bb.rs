use crate::ir::machine::MOperand;
use crate::utils::arena::IndexedArena;

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type MCFG = IndexedArena<MBasicBlock>;

#[derive(Debug, Clone, Default)]
pub struct MBasicBlock {
    pub prev: Vec<MOperand>,
    pub cur: Vec<MOperand>,
    pub succs: Vec<MOperand>,
}

impl MCFG {
    pub fn add_succ(&mut self, bb_idx: MOperand, succ_idx: MOperand) {
        if !self[bb_idx.get_bb_id()].succs.contains(&succ_idx) {
            self[bb_idx.get_bb_id()].succs.push(succ_idx);
        }
    }

    pub fn add_pred(&mut self, bb_idx: MOperand, pred_idx: MOperand) {
        if !self[bb_idx.get_bb_id()].prev.contains(&pred_idx) {
            self[bb_idx.get_bb_id()].prev.push(pred_idx);
        }
    }

    pub fn remove_succ(&mut self, bb_idx: MOperand, succ_idx: MOperand) {
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

    pub fn remove_pred(&mut self, bb_idx: MOperand, pred_idx: MOperand) {
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

impl Index<MOperand> for MCFG {
    type Output = MBasicBlock;

    fn index(&self, index: MOperand) -> &Self::Output {
        match index {
            MOperand::BB(id) => self.get(id).unwrap(),
            _ => panic!("MCFG index: expected MOperand::BB, got {:?}", index),
        }
    }
}

impl IndexMut<MOperand> for MCFG {
    fn index_mut(&mut self, index: MOperand) -> &mut Self::Output {
        match index {
            MOperand::BB(id) => self.get_mut(id).unwrap(),
            _ => panic!("MCFG index_mut: expected MOperand::BB, got {:?}", index),
        }
    }
}
