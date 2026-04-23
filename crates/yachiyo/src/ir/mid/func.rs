//! Function definition of IR.

#[cfg(feature = "debug")]
use crate::debug::info;

use crate::base::Type;
use crate::ir::mid::{BasicBlock, Op, OpData, Operand, PhiIncoming, CFG, DFG};
use crate::utils::arena::*;
use crate::utils::r#match::match_some;
use crate::utils::set::BitSet;
use crate::utils::worklist::{Worklist, WorklistTrait};
use std::ops::{Index, IndexMut};

pub type CG = IndexedArena<Function>;
pub type Params = IndexedArena<(String, Type)>;

#[derive(Debug, Clone)]
pub struct Function {
  pub name: String,
  pub is_external: bool,
  pub typ: Type,
  pub cfg: CFG,
  pub dfg: DFG,
  pub params: Params,
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
    }
  }

  fn dpo_rec(&self, order: &mut Worklist<Operand, BitSet>, visited: &mut BitSet, bb_id: Operand) {
    if visited.contains(bb_id.get_bb_id()) {
      return;
    }
    visited.insert(bb_id.get_bb_id());

    let bb = &self.cfg[bb_id];
    for (succ, _) in &bb.succs {
      self.dpo_rec(order, visited, *succ);
    }

    // Post-order traversal.
    order.push_back(bb_id);
  }

  pub fn dpo(&self) -> Worklist<Operand, BitSet> {
    let mut order: Worklist<Operand, BitSet> = Worklist::new();
    let mut visited = BitSet::new();
    let entry = Operand::BB(self.cfg.entry.unwrap());
    self.dpo_rec(&mut order, &mut visited, entry);
    order
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

impl Arena<Function> for IndexedArena<Function> {
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
          panic!("Compaction gc: BB index {} in Op not found", old_idx);
        }
      };
    };

    // No need to rewrite anything inside Function for now
    self.storage.iter_mut().for_each(|func| {
            if let ArenaItem::Data(func) = func {
                if func.is_external {
                    return;
                }
                let old_arena_dfg = func.dfg.gc();
                let old_arena_cfg = func.cfg.gc();

                // rewrite op refs in BasicBlocks
                func.cfg.storage.iter_mut().for_each(|item| {
                    if let ArenaItem::Data(bb) = item {
                        for op_idx in bb.cur.iter_mut() {
                            remap_with_dfg(op_idx, &old_arena_dfg);
                        }
                    }
                });

                // rewrite BBId in dfg ops
                func.dfg.storage.iter_mut().for_each(|item| {
                    if let ArenaItem::Data(op) = item {
                        match_some! {
                            target: &mut op.data,
                            enu: OpData,
                            minor_arms: {
                                OpData::Jump { target_bb } => {
                                    remap_with_cfg(target_bb, &old_arena_cfg);
                                }
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
                                        // If incoming == None, do nothing
                                    }
                                }

                                // Special: Call needs to rewrite the function operand.
                                OpData::Call { func, .. } => {
                                    if let Operand::Func(func_id) = func {
                                        remap_idx(func_id, &old_arena);
                                    }
                                }
                            },
                            uni_ops: [AddF, SubF, MulF, DivF, AddI, SubI, MulI, DivI, ModI, Load, Store, Alloca, GlobalAlloca, Declare, GEP, Sitofp, Fptosi, Zext, Uitofp, Ret, Shl, Shr, Sar, SNe, SEq, Xor, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe],
                            uni_arm: { /* no BBId to rewrite */ }
                        }
                    }
                });
            }
        });

    // replace old storage
    old_arena
  }
}

impl IndexedArena<Function> {
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

impl Index<Operand> for Params {
  type Output = (String, Type);

  fn index(&self, index: Operand) -> &Self::Output {
    match index {
      Operand::Param(id) => self.get(id).unwrap(),
      _ => panic!("Params index: expected Operand::Param, got {:?}", index),
    }
  }
}
