//! Memory management for Machine IR.

use crate::config::STK_FRM_ALIGN;
use crate::ir::back::BOperand;
use crate::ir::back::BType;
use crate::utils::arena::*;

use std::ops::{Index, IndexMut};

pub trait MemInfo {
    fn size(&self) -> u32;
}

#[inline]
fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

pub type RoDataInfo = IndexedArena<RoData>;
#[derive(Debug, Clone)]
pub struct RoData {
    inner: Vec<BOperand>,
    size: u32,
    align: u32,
}

impl RoData {
    pub fn new(typ: BType, inner: Vec<BOperand>) -> Self {
        RoData {
            inner,
            size: typ.size(),
            align: typ.align(),
        }
    }

    pub fn inner(&self) -> &[BOperand] {
        &self.inner
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn align(&self) -> u32 {
        self.align
    }
}

impl MemInfo for RoDataInfo {
    fn size(&self) -> u32 {
        let mut total = 0_u32;
        for id in self.collect() {
            let data = &self[id];
            total = align_up(total, data.align);
            total += data.size;
        }
        total
    }
}

pub type DataInfo = IndexedArena<Data>;

#[derive(Debug, Clone)]
pub struct Data {
    inner: Vec<BOperand>,
    size: u32,
    align: u32,
}

impl Data {
    pub fn new(typ: BType, inner: Vec<BOperand>) -> Self {
        Data {
            inner,
            size: typ.size(),
            align: typ.align(),
        }
    }

    pub fn inner(&self) -> &[BOperand] {
        &self.inner
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn align(&self) -> u32 {
        self.align
    }
}

impl MemInfo for DataInfo {
    fn size(&self) -> u32 {
        let mut total = 0_u32;
        for id in self.collect() {
            let data = &self[id];
            total = align_up(total, data.align);
            total += data.size;
        }
        total
    }
}

pub type FrameInfo = IndexedArena<Slot>;

#[derive(Debug, Clone)]
pub enum Slot {
    Param { size: u32, align: u32, offset: i32 },
    Arg { size: u32, align: u32, offset: i32 },
    Local { size: u32, align: u32, offset: i32 },
    CalleeSaved { size: u32, align: u32, offset: i32 },
}

/// TODO: implement stack frame layout and management.
impl FrameInfo {
    pub fn calc_offset(&mut self) {
        let mut fp_offset = 0_u32;
        let mut sp_offset = 0_u32;

        for id in self.collect() {
            let slot = self.get_mut(id).unwrap();
            match slot {
                Slot::Param {
                    size,
                    align,
                    offset,
                } => {
                    fp_offset = align_up(fp_offset, *align);
                    *offset = fp_offset as i32;
                    fp_offset += *size;
                }
                Slot::Arg {
                    size,
                    align,
                    offset,
                }
                | Slot::Local {
                    size,
                    align,
                    offset,
                }
                | Slot::CalleeSaved {
                    size,
                    align,
                    offset,
                } => {
                    sp_offset = align_up(sp_offset, *align);
                    *offset = sp_offset as i32;
                    sp_offset += *size;
                }
            }
        }
    }
}

impl MemInfo for FrameInfo {
    /// Return the size of the entire stack frame.
    /// CAUTION: The size should be 16-bytes aligned.
    fn size(&self) -> u32 {
        let mut total = 0_u32;

        for id in self.collect() {
            let slot = &self[id];
            match slot {
                Slot::Param { .. } => {}
                Slot::Arg { size, align, .. }
                | Slot::Local { size, align, .. }
                | Slot::CalleeSaved { size, align, .. } => {
                    total = align_up(total, *align);
                    total += *size;
                }
            }
        }

        align_up(total, STK_FRM_ALIGN)
    }
}

impl Index<BOperand> for DataInfo {
    type Output = Data;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::Data(id) => self.get(id).unwrap(),
            _ => panic!("DataInfo index: expected BOperand::Data, got {:?}", index),
        }
    }
}

impl IndexMut<BOperand> for DataInfo {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::Data(id) => self.get_mut(id).unwrap(),
            _ => panic!(
                "DataInfo index_mut: expected BOperand::Data, got {:?}",
                index
            ),
        }
    }
}

impl Index<BOperand> for RoDataInfo {
    type Output = RoData;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::RoData(id) => self.get(id).unwrap(),
            _ => panic!(
                "RoDataInfo index: expected BOperand::RoData, got {:?}",
                index
            ),
        }
    }
}

impl IndexMut<BOperand> for RoDataInfo {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::RoData(id) => self.get_mut(id).unwrap(),
            _ => panic!(
                "RoDataInfo index_mut: expected BOperand::RoData, got {:?}",
                index
            ),
        }
    }
}

impl Index<BOperand> for FrameInfo {
    type Output = Slot;

    fn index(&self, index: BOperand) -> &Self::Output {
        match index {
            BOperand::Slot(id) => self.get(id).unwrap(),
            _ => panic!("FrameInfo index: expected BOperand::Slot, got {:?}", index),
        }
    }
}

impl IndexMut<BOperand> for FrameInfo {
    fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
        match index {
            BOperand::Slot(id) => self.get_mut(id).unwrap(),
            _ => panic!(
                "FrameInfo index_mut: expected BOperand::Slot, got {:?}",
                index
            ),
        }
    }
}
