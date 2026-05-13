//! Function definition of IR.

#[cfg(feature = "debug")]
use crate::debug::info;

use crate::base::Type;
use crate::ir::mid::{BasicBlock, Op, OpData, Operand, PhiIncoming, CFG, DFG};
use crate::utils::arena::*;
use crate::utils::map::IndexedMap;
use std::ops::{Deref, DerefMut, Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Default)]
pub struct CG(IndexedArena<Function>);

impl CG {
  pub fn new() -> Self {
    Self(IndexedArena::new())
  }
}

impl Deref for CG {
  type Target = IndexedArena<Function>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for CG {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone)]
pub struct Param {
  pub name: String,
  pub typ: Type,
  users: Vec<(Operand, usize)>,
}

impl Param {
  pub fn new(name: String, typ: Type) -> Self {
    Self {
      name,
      typ,
      users: vec![],
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct Params(IndexedArena<Param>);

impl Deref for Params {
  type Target = IndexedArena<Param>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for Params {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone)]
pub struct Function {
  pub name: String,
  pub is_external: bool,
  pub typ: Type,
  pub cfg: CFG,
  pub dfg: DFG,
  pub params: Params,
  pub op_to_bb: IndexedMap<Operand, Operand>,
}

impl Function {
  pub fn new(name: String, is_external: bool, typ: Type) -> Self {
    Self {
      name,
      is_external,
      typ,
      cfg: CFG::default(),
      dfg: DFG::default(),
      params: Params::default(),
      op_to_bb: IndexedMap::default(),
    }
  }

  pub fn rebuild_op_to_bb(&mut self) {
    let mut op_to_bb = IndexedMap::with_capacity(self.dfg.len());
    for (bb_id, item) in self.cfg.storage.iter().enumerate() {
      if let ArenaItem::Data(bb) = item {
        for &op_id in &bb.cur {
          op_to_bb[op_id] = Operand::BB(bb_id);
        }
      }
    }
    self.op_to_bb = op_to_bb;
  }

  pub fn get_src_tuple(&self, op_id: Operand) -> Vec<(&Operand, usize)> {
    self.dfg.get_src_tuple(op_id)
  }

  pub fn get_src(&self, op_id: Operand) -> Vec<&Operand> {
    self
      .get_src_tuple(op_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }

  pub fn get_src_tuple_mut(&mut self, op_id: Operand) -> Vec<(&mut Operand, usize)> {
    self.dfg.get_src_tuple_mut(op_id)
  }

  pub fn get_src_mut(&mut self, op_id: Operand) -> Vec<&mut Operand> {
    self
      .get_src_tuple_mut(op_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }
}

impl Arena<Function> for CG {
  fn remove(&mut self, idx: usize) -> Function {
    if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
      data
    } else {
      panic!("ArenaItem is not Function Data");
    }
  }

  fn gc(&mut self) -> Vec<ArenaItem<Function>> {
    let new_arena: Vec<ArenaItem<Function>> = vec![];
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
      "CG GC: {} functions collected, recycle rate: {:.2}%",
      old_arena.len() - self.storage.len(),
      (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
    );

    let remap_idx = |idx: &mut usize, old_arena: &Vec<ArenaItem<Function>>| {
      *idx = match old_arena.get(*idx) {
        Some(ArenaItem::NewIndex(new_idx)) => *new_idx,
        _ => panic!("CG gc: index {} not found", *idx),
      };
    };

    if let Some(entry) = self.entry.as_mut() {
      remap_idx(entry, &old_arena);
    }

    for idx in self.map.values_mut() {
      remap_idx(idx, &old_arena);
    }

    let remap_with_dfg = |op_idx: &mut Operand, old_arena_dfg: &Vec<ArenaItem<Op>>| {
      let old_idx = op_idx.get_op_id();
      *op_idx = match old_arena_dfg.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => Operand::Value(*new_idx),
        _ => {
          panic!("Compaction gc: op index {} in BB not found", old_idx);
        }
      };
    };

    let remap_with_cfg = |bb_idx: &mut Operand, old_arena_cfg: &Vec<ArenaItem<BasicBlock>>| {
      let old_idx = bb_idx.get_bb_id();
      *bb_idx = match old_arena_cfg.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => Operand::BB(*new_idx),
        _ => {
          panic!("Compaction gc: {:?} points to None or NewIndex", old_idx);
        }
      };
    };

    for func in self.storage.iter_mut() {
      let ArenaItem::Data(func) = func else {
        continue;
      };
      if func.is_external {
        continue;
      }

      let old_arena_dfg = func.dfg.gc();
      let old_arena_cfg = func.cfg.gc();

      for item in func.params.storage.iter_mut() {
        if let ArenaItem::Data(param) = item {
          for (op_id, _) in param.users.iter_mut() {
            remap_with_dfg(op_id, &old_arena_dfg);
          }
        }
      }

      // rewrite op refs in BasicBlocks
      for item in func.cfg.storage.iter_mut() {
        if let ArenaItem::Data(bb) = item {
          for op_idx in bb.cur.iter_mut() {
            remap_with_dfg(op_idx, &old_arena_dfg);
          }
        }
      }

      // rewrite BBId in dfg ops
      for item in func.dfg.storage.iter_mut() {
        if let ArenaItem::Data(op) = item {
          match &mut op.data {
            OpData::Jump { target_bb } => remap_with_cfg(target_bb, &old_arena_cfg),
            OpData::Br {
              then_bb, else_bb, ..
            } => {
              remap_with_cfg(then_bb, &old_arena_cfg);
              remap_with_cfg(else_bb, &old_arena_cfg);
            }
            OpData::Phi { incomings } => {
              for phi_incoming in incomings.iter_mut() {
                if let PhiIncoming::Data { bb, .. } = phi_incoming {
                  remap_with_cfg(bb, &old_arena_cfg);
                }
              }
            }
            OpData::Call {
              func: Operand::Func(func_id),
              ..
            } => {
              remap_idx(func_id, &old_arena);
            }
            _ => {}
          }
        }
      }

      func.rebuild_op_to_bb();
    }

    // replace old storage
    old_arena
  }
}

impl CG {
  pub fn add(&mut self, func: Function) -> usize {
    let name = func.name.clone();
    let func_id = self.alloc(func);
    self.add_name(name, func_id);
    func_id
  }
  pub fn collect_internal(&self) -> Vec<usize> {
    self
      .storage
      .iter()
      .enumerate()
      .filter_map(|(idx, item)| {
        if let ArenaItem::Data(func) = item {
          if !func.is_external {
            Some(idx)
          } else {
            None
          }
        } else {
          None
        }
      })
      .collect()
  }
}

impl Index<Operand> for CG {
  type Output = Function;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Func(id) => self.get(id).unwrap(),
      _ => panic!("CG index: expected Operand::Func, got {:?}", index),
    }
  }
}

impl IndexMut<Operand> for CG {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Func(id) => self.get_mut(id).unwrap(),
      _ => panic!("CG index_mut: expected Operand::Func, got {:?}", index),
    }
  }
}

impl Index<usize> for CG {
  type Output = Function;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for CG {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

impl Index<Operand> for Params {
  type Output = Param;

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Param(id) => self.get(id).unwrap(),
      _ => panic!("Params index: expected Operand::Param, got {:?}", index),
    }
  }
}

impl IndexMut<Operand> for Params {
  fn index_mut(&mut self, index: Operand) -> &mut Self::Output {
    match index {
      Operand::Param(id) => self.get_mut(id).unwrap(),
      _ => panic!("Params index_mut: expected Operand::Param, got {:?}", index),
    }
  }
}

impl Index<usize> for Params {
  type Output = Param;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl IndexMut<usize> for Params {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

impl Params {
  pub fn add_use(&mut self, param_id: Operand, user_tuple: (Operand, usize)) {
    let Operand::Param(param_id) = param_id else {
      return;
    };

    self[param_id].users.push(user_tuple);
  }

  pub fn remove_use(&mut self, param_id: Operand, user_tuple: (Operand, usize)) {
    let Operand::Param(param_id) = param_id else {
      return;
    };

    let param = &mut self[param_id];
    if let Some(pos) = param.users.iter().position(|x| *x == user_tuple) {
      param.users.swap_remove(pos);
    } else {
      panic!(
        "Use {:?}: not found in users of param {:?}: {:?}",
        user_tuple, param_id, param
      );
    }
  }

  pub fn users(&self, param_id: Operand) -> &[(Operand, usize)] {
    let Operand::Param(param_id) = param_id else {
      return &[];
    };

    &self[param_id].users
  }

  pub fn users_mut(&mut self, param_id: Operand) -> &mut Vec<(Operand, usize)> {
    let Operand::Param(param_id) = param_id else {
      panic!(
        "Params users_mut: expected Param operand, got {:?}",
        param_id
      );
    };

    &mut self[param_id].users
  }

  pub fn clear_uses(&mut self) {
    for item in self.storage.iter_mut() {
      if let ArenaItem::Data(param) = item {
        param.users.clear();
      }
    }
  }
}
