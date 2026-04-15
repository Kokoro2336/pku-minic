//! Function definition of BackIR.

#[cfg(feature = "debug")]
use crate::debug::info;
use crate::ir::back::{
  BBasicBlock, BOp, BOpData, BOperand, FrameInfo, LOpData, MOpData, Reg, VirtReg, BCFG, BDFG,
};
use crate::utils::arena::*;
use crate::utils::r#match::{match_full_ops, match_some};

use std::ops::{Index, IndexMut};

#[allow(clippy::upper_case_acronyms)]
pub type BCG = IndexedArena<BFunction>;
pub type VRegs = IndexedArena<VirtReg>;

#[derive(Debug, Clone)]
pub struct BFunction {
  pub name: String,
  pub is_external: bool,
  pub cfg: BCFG,
  pub dfg: BDFG,
  /// Virtual registers used in this function.
  /// Distinct from MidIR, the virtual register is represented as a separate entity
  /// rather than the instruction id like MidIR.
  pub vregs: VRegs,
  /// Stack frame information.
  pub frame_info: FrameInfo,
}

impl BFunction {
  pub fn new(name: String, is_external: bool) -> Self {
    Self {
      name,
      cfg: BCFG::new(),
      dfg: BDFG::new(),
      vregs: VRegs::new(),
      frame_info: FrameInfo::default(),
      is_external,
    }
  }
  pub fn get_rd_tuple(&self, lop_id: BOperand) -> Option<(&BOperand, usize)> {
    self.dfg.get_rd_tuple(lop_id)
  }

  pub fn get_rd(&self, lop_id: BOperand) -> Option<&BOperand> {
    self.dfg.get_rd(lop_id)
  }

  pub fn get_rd_tuple_mut(&mut self, lop_id: BOperand) -> Option<(&mut BOperand, usize)> {
    self.dfg.get_rd_tuple_mut(lop_id)
  }

  pub fn get_rd_mut(&mut self, lop_id: BOperand) -> Option<&mut BOperand> {
    self.dfg.get_rd_mut(lop_id)
  }

  pub fn get_src_tuple(&self, lop_id: BOperand) -> Vec<(&BOperand, usize)> {
    self.dfg.get_src_tuple(lop_id)
  }

  pub fn get_src(&self, lop_id: BOperand) -> Vec<&BOperand> {
    self.dfg.get_src(lop_id)
  }

  pub fn get_src_tuple_mut(&mut self, lop_id: BOperand) -> Vec<(&mut BOperand, usize)> {
    self.dfg.get_src_tuple_mut(lop_id)
  }

  pub fn get_src_mut(&mut self, lop_id: BOperand) -> Vec<&mut BOperand> {
    self.dfg.get_src_mut(lop_id)
  }
}

impl Index<BOperand> for BCG {
  type Output = BFunction;

  fn index(&self, index: BOperand) -> &Self::Output {
    match index {
      BOperand::Func(id) => self.get(id).unwrap(),
      _ => panic!("BCG index: expected BOperand::Func, got {:?}", index),
    }
  }
}

impl IndexMut<BOperand> for BCG {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::Func(id) => self.get_mut(id).unwrap(),
      _ => panic!("BCG index_mut: expected BOperand::Func, got {:?}", index),
    }
  }
}

impl VRegs {
  pub fn add_use(&mut self, vreg_id: BOperand, use_op_id: (BOperand, usize)) {
    #[cfg(feature = "debug")]
    crate::debug::info!("Add use {:?} to vreg {:?}", use_op_id, vreg_id);
    let op_id = match_some! {
        target: vreg_id,
        enu: BOperand,
        minor_arms: {
            BOperand::Reg(Reg::Virt(id)) => id,
            BOperand::Reg(Reg::X(_))
            | BOperand::Reg(Reg::F(_)) => return,
        },
        uni_ops: [IntImm, FloatImm, Func, Inst, Slot, Data, RoData, Bss, BB, Undef],
        uni_arm: return
    };
    let vreg = &mut self[op_id];
    vreg.uses.push(use_op_id);
  }

  /// op_idx: VReg, use_idx: Inst that uses the VReg.
  pub fn remove_use(&mut self, vreg_id: BOperand, use_tuple: (BOperand, usize)) {
    #[cfg(feature = "debug")]
    crate::debug::info!("Remove use {:?} from vreg {:?}", use_tuple, vreg_id);
    let vreg_id = match_some! {
        target: vreg_id,
        enu: BOperand,
        minor_arms: {
            BOperand::Reg(Reg::Virt(id)) => id,
            BOperand::Reg(Reg::X(_))
            | BOperand::Reg(Reg::F(_)) => return,
        },
        uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, Bss, BB, Undef],
        uni_arm: return
    };
    let vreg = &mut self[vreg_id];
    if let Some(pos) = vreg.uses.iter().position(|x| *x == use_tuple) {
      vreg.uses.swap_remove(pos);
    } else {
      panic!(
        "Use {:?}: not found in users of virtual register {:?}",
        use_tuple, vreg_id
      );
    }
  }

  pub fn remove_def(&mut self, vreg_id: BOperand, def_op_id: BOperand) {
    let vreg_id = match_some! {
        target: vreg_id,
        enu: BOperand,
        minor_arms: {
            BOperand::Reg(Reg::Virt(id)) => id,
            BOperand::Reg(Reg::X(_))
            | BOperand::Reg(Reg::F(_)) => return,
        },
        uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, Bss, BB, Undef],
        uni_arm: return
    };
    let vreg = &mut self[vreg_id];
    if let Some(pos) = vreg.defs.iter().position(|x| *x == def_op_id) {
      vreg.defs.swap_remove(pos);
    } else {
      panic!("Def {:?}: not found in defs of op {:?}", def_op_id, vreg_id);
    }
    #[cfg(feature = "debug")]
    crate::debug::info!("Remove def {:?} from vreg {:?}", def_op_id, vreg_id);
  }

  pub fn add_def(&mut self, vreg_id: BOperand, def_op_id: BOperand) {
    let vreg_id = match_some! {
        target: vreg_id,
        enu: BOperand,
        minor_arms: {
            BOperand::Reg(Reg::Virt(id)) => id,
            BOperand::Reg(Reg::X(_))
            | BOperand::Reg(Reg::F(_)) => return,
        },
        uni_ops: [IntImm, FloatImm, Inst, Func, Slot, Data, RoData, Bss, BB, Undef],
        uni_arm: return
    };
    let vreg = &mut self[vreg_id];
    if vreg.defs.contains(&def_op_id) {
      return;
    }
    vreg.defs.push(def_op_id);
  }

  pub fn clear_dead(&mut self) {
    for vreg_id in self.ids() {
      let item = &self.storage[vreg_id];
      if let ArenaItem::Data(vreg) = item {
        if vreg.defs.is_empty() {
          if vreg.uses.is_empty() {
            self.remove(vreg_id);
          } else {
            panic!(
              "Cannot clear vreg {:?} because it has uses {:?}",
              vreg_id, vreg.uses
            );
          }
        }
      }
    }
  }
}

impl Index<BOperand> for VRegs {
  type Output = VirtReg;

  fn index(&self, index: BOperand) -> &Self::Output {
    match index {
      BOperand::Reg(Reg::Virt(id)) => self.get(id).unwrap(),
      _ => panic!(
        "VRegs index: expected BOperand::Reg(Reg::Virt), got {:?}",
        index
      ),
    }
  }
}

impl IndexMut<BOperand> for VRegs {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::Reg(Reg::Virt(id)) => self.get_mut(id).unwrap(),
      _ => panic!(
        "VRegs index_mut: expected BOperand::Reg(Reg::Virt), got {:?}",
        index
      ),
    }
  }
}

impl Arena<VirtReg> for VRegs {
  fn remove(&mut self, idx: usize) -> VirtReg {
    if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
      data
    } else {
      panic!("VRegs remove: index {} points to None or NewIndex", idx);
    }
  }
  fn gc(&mut self) -> Vec<ArenaItem<VirtReg>> {
    let new_arena: Vec<ArenaItem<VirtReg>> = vec![];
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
      "VRegs GC: {} virtual registers collected, recycle rate: {:.2}%",
      old_arena.len() - self.storage.len(),
      (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
    );

    // No entry. No need to remap.
    // defs and uses should be remapped outside of this function.
    old_arena
  }
}

impl Arena<BFunction> for BCG {
  fn remove(&mut self, idx: usize) -> BFunction {
    if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
      data
    } else {
      panic!("BCG remove: index {} points to None or NewIndex", idx);
    }
  }
  fn gc(&mut self) -> Vec<ArenaItem<BFunction>> {
    let new_arena: Vec<ArenaItem<BFunction>> = vec![];
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
      "BCG GC: {} functions collected, recycle rate: {:.2}%",
      old_arena.len() - self.storage.len(),
      (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
    );

    // No entry. No need to remap.

    let remap_idx = |idx: &mut usize, old_arena: &Vec<ArenaItem<BFunction>>| {
      *idx = match old_arena.get(*idx) {
        Some(ArenaItem::NewIndex(new_idx)) => *new_idx,
        _ => panic!("BCG gc: index {} in BFunction not found", *idx),
      };
    };

    if let Some(entry) = self.entry.as_mut() {
      remap_idx(entry, &old_arena);
    }

    for idx in self.map.values_mut() {
      remap_idx(idx, &old_arena);
    }

    let remap_with_cfg = |bb_idx: &mut BOperand, old_arena_cfg: &Vec<ArenaItem<BBasicBlock>>| {
      let old_idx = bb_idx.get_bb_id();
      *bb_idx = match old_arena_cfg.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => BOperand::BB(*new_idx),
        _ => panic!("BCG gc: BB index {} in BFunction not found", old_idx),
      };
    };

    let remap_with_dfg = |op_idx: &mut BOperand, old_arena_dfg: &Vec<ArenaItem<BOp>>| {
      let old_idx = op_idx.get_inst_id();
      *op_idx = match old_arena_dfg.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => BOperand::Inst(*new_idx),
        _ => {
          panic!("BCG gc: op index {} in BFunction not found", old_idx);
        }
      };
    };

    let remap_with_vregs = |vreg_idx: &mut BOperand, old_arena_vregs: &Vec<ArenaItem<VirtReg>>| {
      let old_idx = vreg_idx.get_virt_id();
      *vreg_idx = match old_arena_vregs.get(old_idx) {
        Some(ArenaItem::NewIndex(new_idx)) => BOperand::Reg(Reg::Virt(*new_idx)),
        _ => {
          panic!("BCG gc: vreg index {} in BFunction not found", old_idx);
        }
      };
    };

    self.storage.iter_mut().for_each(|func| {
        if let ArenaItem::Data(func) = func {
            let old_arena_cfg = func.cfg.gc();
            let old_arena_dfg = func.dfg.gc();
            let old_arena_vregs = func.vregs.gc();

            // Rewrite BOp refs in BBasicBlocks
            func.cfg.storage.iter_mut().for_each(|item| {
                if let ArenaItem::Data(bb) = item {
                    for op_idx in bb.cur.iter_mut() {
                        remap_with_dfg(op_idx, &old_arena_dfg);
                    }
                }
            });

            // Rewrite BB refs and VReg refs in BOps
            func.dfg.storage.iter_mut().for_each(|item| {
                if let ArenaItem::Data(op) = item {
                    match &mut op.data {
                        BOpData::L(lop_data) => {
                            match_full_ops! {
                                target: lop_data,
                                bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, Xor, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Shl, Shr, Sar],
                                bin_arm: LOpData { rd, lhs, rhs } => {
                                    if rd.is_virt() {
                                        remap_with_vregs(rd, &old_arena_vregs);
                                    }
                                    if lhs.is_virt() {
                                        remap_with_vregs(lhs, &old_arena_vregs);
                                    }
                                    if rhs.is_virt() {
                                        remap_with_vregs(rhs, &old_arena_vregs);
                                    }
                                },
                                un_ops: [Sitofp, Fptosi],
                                un_arm: LOpData { rd, value } => {
                                    if rd.is_virt() {
                                        remap_with_vregs(rd, &old_arena_vregs);
                                    }
                                    if value.is_virt() {
                                        remap_with_vregs(value, &old_arena_vregs);
                                    }
                                },
                                fallback: {
                                    LOpData::Store { addr, value, .. } => {
                                        if addr.is_virt() {
                                            remap_with_vregs(addr, &old_arena_vregs);
                                        }
                                        if value.is_virt() {
                                            remap_with_vregs(value, &old_arena_vregs);
                                        }
                                    },
                                    LOpData::Load { rd, addr } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                        if addr.is_virt() {
                                            remap_with_vregs(addr, &old_arena_vregs);
                                        }
                                    }
                                    LOpData::Br { cond, then_bb, else_bb } => {
                                        if cond.is_virt() {
                                            remap_with_vregs(cond, &old_arena_vregs);
                                        }
                                        remap_with_cfg(then_bb, &old_arena_cfg);
                                        remap_with_cfg(else_bb, &old_arena_cfg);
                                    }
                                    LOpData::Move { rd, src } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                        if src.is_virt() {
                                            remap_with_vregs(src, &old_arena_vregs);
                                        }
                                    },
                                    LOpData::Jump { target_bb } => {
                                        remap_with_cfg(target_bb, &old_arena_cfg);
                                    }
                                    LOpData::Call { func } => {
                                        if let BOperand::Func(func_id) = func {
                                            remap_idx(func_id, &old_arena);
                                        }
                                    }
                                    LOpData::Ret
                                    | LOpData::LoadIntImm {..}
                                    | LOpData::LoadFloatImm {..}
                                    | LOpData::LoadAddress {..} => {}
                                }
                            }
                        }
                        BOpData::M(mop_data) => {
                            match_full_ops! {
                                target: mop_data,
                            bin_ops: [Add, Sub, Addw, Subw, Mulw, Divw, Remw, Sllw, Srlw, Sraw, Slt, Sltu, Xor, FaddS, FsubS, FmulS, FdivS, FeqS, FneS, FltS, FgeS, FleS, FgtS],
                                bin_arm: MOpData { rd, rs1, rs2 } => {
                                    if rd.is_virt() {
                                        remap_with_vregs(rd, &old_arena_vregs);
                                    }
                                    if rs1.is_virt() {
                                        remap_with_vregs(rs1, &old_arena_vregs);
                                    }
                                    if rs2.is_virt() {
                                        remap_with_vregs(rs2, &old_arena_vregs);
                                    }
                                },
                                un_ops: [FcvtWS, FcvtSW, FmvWX, FmvXW, Mv, FmvS],
                                un_arm: MOpData { rd, rs } => {
                                    if rd.is_virt() {
                                        remap_with_vregs(rd, &old_arena_vregs);
                                    }
                                    if rs.is_virt() {
                                        remap_with_vregs(rs, &old_arena_vregs);
                                    }
                                },
                                fallback: {
                                    MOpData::Li { rd, .. } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                    }
                                    MOpData::La { rd, .. } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                        // TODO: What about the label?
                                    }
                                    MOpData::Addi { rd, rs1, .. }
                                    | MOpData::Addiw { rd, rs1, .. }
                                    | MOpData::Slliw { rd, rs1, .. }
                                    | MOpData::Srliw { rd, rs1, .. }
                                    | MOpData::Sraiw { rd, rs1, .. }
                                    | MOpData::Slti { rd, rs1, .. }
                                    | MOpData::Sltiu { rd, rs1, .. }
                                    | MOpData::Xori { rd, rs1, .. } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                        if rs1.is_virt() {
                                            remap_with_vregs(rs1, &old_arena_vregs);
                                        }
                                    },
                                    MOpData::Lw { rd, base, .. }
                                    | MOpData::Ld { rd, base, .. }
                                    | MOpData::Flw { rd, base, .. }
                                    | MOpData::Fld { rd, base, .. } => {
                                        if rd.is_virt() {
                                            remap_with_vregs(rd, &old_arena_vregs);
                                        }
                                        if base.is_virt() {
                                            remap_with_vregs(base, &old_arena_vregs);
                                        }
                                    },
                                    MOpData::Sw { rs, base, .. }
                                    | MOpData::Sd { rs, base, .. }
                                    | MOpData::Fsw { rs, base, .. }
                                    | MOpData::Fsd { rs, base, .. } => {
                                        if rs.is_virt() {
                                            remap_with_vregs(rs, &old_arena_vregs);
                                        }
                                        if base.is_virt() {
                                            remap_with_vregs(base, &old_arena_vregs);
                                        }
                                    },
                                    MOpData::J { target } => {
                                        remap_with_cfg(target, &old_arena_cfg);
                                    }
                                    MOpData::Call { target } => {
                                        if let BOperand::Func(func_id) = target {
                                            remap_idx(func_id, &old_arena);
                                        }
                                    }
                                    MOpData::Bnez { rs, target } => {
                                        if rs.is_virt() {
                                            remap_with_vregs(rs, &old_arena_vregs);
                                        }
                                        remap_with_cfg(target, &old_arena_cfg);
                                    }
                                    MOpData::Beq {rs1, rs2, offset}
                                    | MOpData::Bne { rs1, rs2, offset }
                                    | MOpData::Bge { rs1, rs2, offset }
                                    | MOpData::Blt { rs1, rs2, offset }
                                    | MOpData::Bgeu { rs1, rs2, offset }
                                    | MOpData::Bltu { rs1, rs2, offset } => {
                                        if rs1.is_virt() {
                                            remap_with_vregs(rs1, &old_arena_vregs);
                                        }
                                        if rs2.is_virt() {
                                            remap_with_vregs(rs2, &old_arena_vregs);
                                        }
                                        if offset.is_virt() {
                                          remap_with_vregs(offset, &old_arena_vregs);
                                        }
                                    }
                                    MOpData::Ret => {}
                                }
                            }
                        }
                    }
                }
            });

            // Rewrite BOps refs in vregs
            func.vregs.storage.iter_mut().for_each(|item| {
                if let ArenaItem::Data(vreg) = item {
                    for use_tuple in vreg.uses.iter_mut() {
                        remap_with_dfg(&mut use_tuple.0, &old_arena_dfg);
                    }
                    for def_op in vreg.defs.iter_mut() {
                        remap_with_dfg(def_op, &old_arena_dfg);
                    }
                }
            });
        }
    });

    old_arena
  }
}

impl BCG {
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
