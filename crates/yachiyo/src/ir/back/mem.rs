//! Memory management for BackIR.

use crate::base::Type;
use crate::config::{PARAM_REG_MAX_NUM, STK_FRM_ALIGN};
use crate::ir::back::{BOperand, BType};
use crate::utils::IndexedArena;

use rustc_hash::FxHashMap;
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
  pub typ: BType,
}

impl RoData {
  pub fn new(typ: Type, inner: Vec<BOperand>) -> Self {
    RoData {
      inner,
      typ: typ.into(),
    }
  }

  pub fn inner(&self) -> &[BOperand] {
    &self.inner
  }

  pub fn size(&self) -> u32 {
    self.typ.size()
  }

  pub fn align(&self) -> u32 {
    self.typ.align()
  }
}

impl MemInfo for RoDataInfo {
  fn size(&self) -> u32 {
    let mut total = 0_u32;
    for id in self.collect() {
      let data = &self[id];
      total = align_up(total, data.typ.align());
      total += data.typ.size();
    }
    total
  }
}

pub type DataInfo = IndexedArena<Data>;

#[derive(Debug, Clone)]
pub struct Data {
  inner: Vec<BOperand>,
  pub typ: BType,
}

impl Data {
  pub fn new(typ: Type, inner: Vec<BOperand>) -> Self {
    Data {
      inner,
      typ: typ.into(),
    }
  }

  pub fn inner(&self) -> &[BOperand] {
    &self.inner
  }

  pub fn size(&self) -> u32 {
    self.typ.size()
  }

  pub fn align(&self) -> u32 {
    self.typ.align()
  }
}

impl MemInfo for DataInfo {
  fn size(&self) -> u32 {
    let mut total = 0_u32;
    for id in self.collect() {
      let data = &self[id];
      total = align_up(total, data.typ.align());
      total += data.typ.size();
    }
    total
  }
}

#[derive(Debug, Clone, Default)]
pub struct FrameInfo {
  /// Offset of Param/Local/CalleeSaved is fixed, so we store it in the arena.
  storage: IndexedArena<Slot>,
  /// Arguments layout of called functions to reuse the layout info.
  arg_outgoing: FxHashMap<BOperand, Vec<BOperand>>,
  /// Args' offsets of each call are calculated dynamically, so we don't need to store their offsets.
  arg_outgoing_size: u32,
  /// Total size of stack frame.
  size: u32,
}

#[derive(Debug, Clone)]
pub enum Slot {
  CalleeSaved {
    typ: BType,
    offset: i32,
  },
  Local {
    typ: BType,
    offset: i32,
  },
  Param {
    index: u32,
    typ: BType,
    offset: i32,
  },
  /// Different from other kind of slot, the offset of Arg is not fixed, it is determined by the caller and callee together.
  /// We still store an Arg for each argument of each call, but the call sites' args offset can overlap with each other.
  Arg {
    typ: BType,
    offset: i32,
  },
}

impl FrameInfo {
  /// If the new size is larger than the current `arg_outgoing_size`, update it.
  pub fn update_arg_outgoing_size(&mut self, size: u32) {
    self.arg_outgoing_size = self.arg_outgoing_size.max(size);
  }

  pub fn alloc(&mut self, slot: Slot) -> usize {
    self.storage.alloc(slot)
  }

  pub fn len(&self) -> usize {
    self.storage.len()
  }

  pub fn is_empty(&self) -> bool {
    self.storage.is_empty()
  }

  pub fn get_spilled_arg_offsets(
    &mut self,
    callee_func_id: BOperand,
    callee_func_typ: &Type,
  ) -> Vec<BOperand> {
    if self.arg_outgoing.contains_key(&callee_func_id) {
      return self.arg_outgoing[&callee_func_id].clone();
    }
    if let Type::Function { param_types, .. } = callee_func_typ {
      let arg_ids = param_types
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
          if index as u32 >= PARAM_REG_MAX_NUM {
            let slot_id = self.storage.alloc(Slot::Arg {
              typ: param.clone().into(),
              offset: 0, // offset will be assigned later in build()
            });
            Some(BOperand::Slot(slot_id))
          } else {
            None
          }
        })
        .collect::<Vec<_>>();

      // Compute the offset of outgoing args for this call and update the total size if necessary.
      let mut offset = 0_u32;
      for id in arg_ids.iter() {
        if let BOperand::Slot(slot_id) = id {
          if let Slot::Arg {
            typ,
            offset: slot_offset,
            ..
          } = &mut self.storage[*slot_id]
          {
            offset = align_up(offset, typ.align());
            *slot_offset = offset as i32;
            offset += typ.size();
          } else {
            unreachable!()
          }
        } else {
          unreachable!()
        }
      }

      // Update the max outgoing size in site.
      self.arg_outgoing_size = self.arg_outgoing_size.max(offset);
      // Update layout map.
      self.arg_outgoing.insert(callee_func_id, arg_ids.clone());

      arg_ids
    } else {
      panic!(
        "get_arg_offsets: expected function type, got {:?}",
        callee_func_typ
      );
    }
  }

  /// Build the stack frame.
  pub fn build(&mut self) {
    // Group slots by their kind.
    let mut callee_saved_slots = Vec::new();
    let mut local_slots = Vec::new();
    let mut param_slots = Vec::new();

    for id in self.storage.collect() {
      let slot = &self.storage[id];
      match slot {
        Slot::CalleeSaved { .. } => callee_saved_slots.push(id),
        Slot::Local { .. } => local_slots.push(id),
        Slot::Param { .. } => param_slots.push(id),
        Slot::Arg { .. } => {} // We don't need to assign offsets for Arg slots here, they will be assigned in get_arg_offsets() when we encounter a call site.
      }
    }

    // Sort params by their index.
    param_slots.sort_by_key(|&id| {
      if let Slot::Param { index, .. } = &self.storage[id] {
        *index
      } else {
        unreachable!()
      }
    });

    // We start assigning offsets from the top of arg outgoing area.
    let mut offset = self.arg_outgoing_size as i32;

    // Assign offsets for local slots.
    for id in local_slots {
      if let Slot::Local {
        typ,
        offset: slot_offset,
      } = &mut self.storage[id]
      {
        offset = align_up(offset as u32, typ.align()) as i32;
        *slot_offset = offset;
        offset += typ.size() as i32;
      }
    }

    // Assign offsets for callee-saved slots.
    for id in callee_saved_slots {
      if let Slot::CalleeSaved {
        typ,
        offset: slot_offset,
      } = &mut self.storage[id]
      {
        offset = align_up(offset as u32, typ.align()) as i32;
        *slot_offset = offset;
        offset += typ.size() as i32;
      }
    }

    // Align the stack frame size with STK_FRM_ALIGN.
    offset = align_up(offset as u32, STK_FRM_ALIGN) as i32;
    // And this is the total size of stack frame.
    self.size = offset as u32;

    // Assign offsets for param slots.
    for id in param_slots {
      if let Slot::Param {
        typ,
        offset: slot_offset,
        ..
      } = &mut self.storage[id]
      {
        offset = align_up(offset as u32, typ.align()) as i32;
        *slot_offset = offset;
        offset += typ.size() as i32;
      }
    }
  }
}

impl MemInfo for FrameInfo {
  /// Return the size of the entire stack frame.
  fn size(&self) -> u32 {
    self.size
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
      BOperand::Slot(id) => &self.storage[id],
      _ => panic!("FrameInfo index: expected BOperand::Slot, got {:?}", index),
    }
  }
}

impl IndexMut<BOperand> for FrameInfo {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::Slot(id) => &mut self.storage[id],
      _ => panic!(
        "FrameInfo index_mut: expected BOperand::Slot, got {:?}",
        index
      ),
    }
  }
}

impl Index<usize> for FrameInfo {
  type Output = Slot;

  fn index(&self, index: usize) -> &Self::Output {
    &self.storage[index]
  }
}

impl IndexMut<usize> for FrameInfo {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.storage[index]
  }
}

pub type BssInfo = IndexedArena<Bss>;

#[derive(Debug, Clone)]
pub struct Bss {
  pub typ: BType,
}

impl Bss {
  pub fn new(typ: Type) -> Self {
    Bss { typ: typ.into() }
  }

  pub fn size(&self) -> u32 {
    self.typ.size()
  }

  pub fn align(&self) -> u32 {
    self.typ.align()
  }
}

impl MemInfo for BssInfo {
  fn size(&self) -> u32 {
    let mut total = 0_u32;
    for id in self.collect() {
      let bss = &self[id];
      total = align_up(total, bss.align());
      total += bss.size();
    }
    total
  }
}

impl Index<BOperand> for BssInfo {
  type Output = Bss;

  fn index(&self, index: BOperand) -> &Self::Output {
    match index {
      BOperand::Bss(id) => self.get(id).unwrap(),
      _ => panic!("BssInfo index: expected BOperand::Bss, got {:?}", index),
    }
  }
}

impl IndexMut<BOperand> for BssInfo {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::Bss(id) => self.get_mut(id).unwrap(),
      _ => panic!("BssInfo index_mut: expected BOperand::Bss, got {:?}", index),
    }
  }
}
