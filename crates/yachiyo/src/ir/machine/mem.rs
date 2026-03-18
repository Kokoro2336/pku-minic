//! Memory management for Machine IR.

use crate::base::Type;
use crate::ir::lower::LOperand;
use crate::ir::machine::MOperand;
use crate::utils::arena::*;
use std::ops::{Index, IndexMut};

pub type RoDataInfo = IndexedArena<RoData>;

#[derive(Debug, Clone)]
pub struct RoData {
    inner: Vec<MOperand>,
    size: u32,
    align: u32,
}

impl RoData {
    pub fn new(typ: Type, inner: Vec<MOperand>) -> Self {
        RoData {
            inner,
            size: typ.size(),
            align: typ.align(),
        }
    }
}

pub type DataInfo = IndexedArena<Data>;

#[derive(Debug, Clone)]
pub struct Data {
    inner: Vec<MOperand>,
    size: u32,
    align: u32,
}

impl Data {
    pub fn new(typ: Type, inner: Vec<MOperand>) -> Self {
        Data {
            inner,
            size: typ.size(),
            align: typ.align(),
        }
    }
}

pub type FrameInfo = IndexedArena<Slot>;

#[derive(Debug, Clone)]
pub enum Slot {
    Param { size: u32, align: u32 },
    Local { size: u32, align: u32 },
    CalleeSaved { size: u32, align: u32 },
}

/// TODO: implement stack frame layout and management.
impl FrameInfo {
    /// Return the size of the entire stack frame.
    /// CAUTION: The size should be 16-bytes aligned.
    pub fn size(&mut self) -> u32 {
        todo!()
    }
}

impl Index<LOperand> for DataInfo {
    type Output = Data;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Data(id) => self.get(id).unwrap(),
            _ => panic!("DataInfo index: expected LOperand::Data, got {:?}", index),
        }
    }
}

impl IndexMut<LOperand> for DataInfo {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Data(id) => self.get_mut(id).unwrap(),
            _ => panic!(
                "DataInfo index_mut: expected LOperand::Data, got {:?}",
                index
            ),
        }
    }
}

impl Index<LOperand> for FrameInfo {
    type Output = Slot;

    fn index(&self, index: LOperand) -> &Self::Output {
        match index {
            LOperand::Slot(id) => self.get(id).unwrap(),
            _ => panic!("FrameInfo index: expected LOperand::Slot, got {:?}", index),
        }
    }
}

impl IndexMut<LOperand> for FrameInfo {
    fn index_mut(&mut self, index: LOperand) -> &mut Self::Output {
        match index {
            LOperand::Slot(id) => self.get_mut(id).unwrap(),
            _ => panic!(
                "FrameInfo index_mut: expected LOperand::Slot, got {:?}",
                index
            ),
        }
    }
}
