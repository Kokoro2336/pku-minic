//! Instruction defintion of BackIR.

use super::{BType, Reg};
#[cfg(feature = "debug")]
use crate::debug::info;
use crate::ir::back::LOpData;
use crate::ir::back::MOpData;
use crate::utils::arena::*;
use crate::utils::r#match::{match_rd, match_src};

use std::ops::{Index, IndexMut};

#[derive(Debug, Clone)]
pub struct VirtReg {
  pub typ: BType,
  pub defs: Vec<BOperand>,
  /// (OpId of uses, operand idx in the use instruction)
  pub uses: Vec<(BOperand, usize)>,
}

impl VirtReg {
  pub fn new(typ: BType) -> Self {
    Self {
      typ,
      defs: Vec::new(),
      uses: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BOperand {
  Func(usize),
  BB(usize),
  Inst(usize),
  Reg(Reg),

  // Immediate
  IntImm(i32),
  /// Float immediate, stored as its bit representation for the convenience of hashing.
  FloatImm(u32),

  /// Id of frame slot
  Slot(usize),
  /// Id of .data arena.
  Data(usize),
  /// Id of .rodata arena.
  RoData(usize),
  /// Id of .bss arena.
  Bss(usize),

  #[default]
  /// If an instruction instance is created with its rd undef, the builder will automatically assign it a new virtual register.
  /// Else if the undef lies in other kinds of operands, then the operand is really undefined.
  Undef,
}

impl std::fmt::Display for BOperand {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      BOperand::Func(id) => write!(f, "fn.{id}"),
      BOperand::BB(id) => write!(f, "bb.{id}"),
      BOperand::Inst(id) => write!(f, "inst.{id}"),
      BOperand::Reg(reg) => write!(f, "{reg}"),
      BOperand::IntImm(imm) => write!(f, "{imm}"),
      BOperand::FloatImm(imm) => write!(f, "{imm}"),
      BOperand::Slot(id) => write!(f, "slot.{id}"),
      BOperand::Data(id) => write!(f, "data.{id}"),
      BOperand::RoData(id) => write!(f, "rodata.{id}"),
      BOperand::Bss(id) => write!(f, "bss.{id}"),
      BOperand::Undef => write!(f, "undef"),
    }
  }
}

#[allow(unused)]
impl BOperand {
  #[inline(always)]
  pub fn get_bb_id(&self) -> usize {
    match self {
      BOperand::BB(id) => *id,
      _ => panic!("Not a basic block operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_inst_id(&self) -> usize {
    match self {
      BOperand::Inst(id) => *id,
      _ => panic!("Not an instruction operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_virt_id(&self) -> usize {
    match self {
      BOperand::Reg(Reg::Virt(id)) => *id,
      _ => panic!("Not a virtual register operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_phys_reg(&self) -> Reg {
    match self {
      BOperand::Reg(reg @ Reg::X(_)) | BOperand::Reg(reg @ Reg::F(_)) => *reg,
      _ => panic!("Not a physical register operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_func_id(&self) -> usize {
    match self {
      BOperand::Func(id) => *id,
      _ => panic!("Not a function operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn hi(imm: i32) -> Self {
    BOperand::IntImm(imm >> 16)
  }
  #[inline(always)]
  pub fn lo(imm: i32) -> Self {
    BOperand::IntImm(imm & 0xFFFF)
  }
  #[inline(always)]
  pub fn is_literal(&self) -> bool {
    matches!(self, BOperand::IntImm(_) | BOperand::FloatImm(_))
  }
  #[inline(always)]
  pub fn negate_literal(&self) -> Self {
    match self {
      BOperand::IntImm(imm) => BOperand::IntImm(-imm),
      BOperand::FloatImm(imm) => BOperand::FloatImm((-f32::from_bits(*imm)).to_bits()),
      _ => panic!("Not a literal operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_int_imm(&self) -> i32 {
    match self {
      BOperand::IntImm(imm) => *imm,
      _ => panic!("Not an int immediate operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn get_float_imm(&self) -> f32 {
    match self {
      BOperand::FloatImm(imm) => f32::from_bits(*imm),
      _ => panic!("Not a float immediate operand: {:?}", self),
    }
  }
  #[inline(always)]
  pub fn is_phys(&self) -> bool {
    matches!(self, BOperand::Reg(Reg::X(_)) | BOperand::Reg(Reg::F(_)))
  }
  #[inline(always)]
  pub fn is_virt(&self) -> bool {
    matches!(self, BOperand::Reg(Reg::Virt(_)))
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BAttr {
  Name(String),
  /// Indicates that this is a pointer arithmetic operation, which should not have a leading load/store.
  PtrArith,
  /// Indicates that this move is a phi move. If an instruction has this attribute, ISel won't create.
  PhiMove,
  /// For call instructions, indicates the operand is a return value.
  Clobber,
  /// For the result of call instruction.
  ImplicitDef(BOperand),
  /// For call/ret instructions, indicates the operand is a used value that is not explicitly passed in the operand list.
  ImplicitUse(Vec<BOperand>),
}

#[derive(Debug, Clone)]
pub struct BOp {
  pub typ: BType,
  pub attrs: Vec<BAttr>,
  pub data: BOpData,
}

impl BOp {
  pub fn new(typ: BType, attrs: Vec<BAttr>, data: BOpData) -> Self {
    Self { typ, attrs, data }
  }
}

#[derive(Debug, Clone)]
pub enum BOpData {
  M(MOpData),
  L(LOpData),
}

impl BOpData {
  pub fn is_move(&self) -> bool {
    match self {
      BOpData::M(mop_data) => matches!(mop_data, MOpData::Mv { .. } | MOpData::FmvS { .. }),
      BOpData::L(lop_data) => matches!(lop_data, LOpData::Move { .. }),
    }
  }
  pub fn is_call(&self) -> bool {
    match self {
      BOpData::M(mop_data) => matches!(mop_data, MOpData::Call { .. }),
      BOpData::L(lop_data) => matches!(lop_data, LOpData::Call { .. }),
    }
  }
  #[inline(always)]
  pub fn is_impure(&self) -> bool {
    match self {
      BOpData::M(mop_data) => mop_data.is_impure(),
      BOpData::L(lop_data) => lop_data.is_impure(),
    }
  }
}

pub type BDFG = IndexedArena<BOp>;

impl Index<BOperand> for BDFG {
  type Output = BOp;

  fn index(&self, index: BOperand) -> &Self::Output {
    match index {
      BOperand::Inst(id) => &self[id],
      _ => panic!("Invalid operand index: {:?}", index),
    }
  }
}

impl IndexMut<BOperand> for BDFG {
  fn index_mut(&mut self, index: BOperand) -> &mut Self::Output {
    match index {
      BOperand::Inst(id) => &mut self[id],
      _ => panic!("Invalid operand index: {:?}", index),
    }
  }
}

impl BDFG {
  pub fn get_rd_tuple(&self, lop_id: BOperand) -> Option<(&BOperand, usize)> {
    let bop = &self[lop_id];

    match &bop.data {
      BOpData::L(lop_data) => match_rd! {
          target: lop_data,
          op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, LoadFloatImm, LoadIntImm, LoadAddress, Move],
          rd_arm: LOpData(rd) => {
              Some((rd, 0))
          },
          fallback: {
              // For other LOpData which doesn't have rd field (e.g. Call and Store), we return Undef.
              LOpData::Store {..}
              | LOpData::Call {..}
              | LOpData::Br {..}
              | LOpData::Jump {..}
              | LOpData::Ret => None,
          }
      },
      BOpData::M(mop_data) => match_rd! {
          target: mop_data,
          op_with_rds: [
              Li, La, Mv, FmvS,
              Add, Sub, Addi, Addw, Subw, Mulw, Divw, Remw,
              Slliw, Srliw, Sraiw,
              Sllw, Srlw, Sraw,
              Slt, Slti, Sltu, Sltiu,
              Addiw,
              Xor, Xori,
              FaddS, FsubS, FmulS, FdivS,
              FeqS, FltS, FleS, FneS, FgtS, FgeS,
              FcvtWS, FcvtSW, FmvWX, FmvXW,
              Lw, Flw, Ld
          ],
          rd_arm: MOpData(rd) => {
              Some((rd, 0))
          },
          fallback: {
              // For other MOpData which doesn't have rd field (e.g. J and Call), we return Undef.
              MOpData::Sw {..}
              | MOpData::Fsw {..}
              | MOpData::Sd {..}
              | MOpData::J {..}
              | MOpData::Bnez {..}
              | MOpData::Call {..}
              | MOpData::Ret
              | MOpData::Beq {..}
              | MOpData::Bne {..}
              | MOpData::Blt {..}
              | MOpData::Bge {..}
              | MOpData::Bltu {..}
              | MOpData::Bgeu {..} => None,
          }
      },
    }
  }

  pub fn get_rd(&self, lop_id: BOperand) -> Option<&BOperand> {
    self.get_rd_tuple(lop_id).map(|(rd, _)| rd)
  }

  pub fn get_rd_tuple_mut(&mut self, lop_id: BOperand) -> Option<(&mut BOperand, usize)> {
    let bop = &mut self[lop_id];

    match &mut bop.data {
      BOpData::L(lop_data) => match_rd! {
          target: lop_data,
          op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, LoadFloatImm, LoadIntImm, LoadAddress, Move],
          rd_arm: LOpData(rd) => {
              Some((rd, 0))
          },
          fallback: {
              // For other LOpData which doesn't have rd field (e.g. Call and Store), we return Undef.
              LOpData::Store {..}
              | LOpData::Call {..}
              | LOpData::Br {..}
              | LOpData::Jump {..}
              | LOpData::Ret => None,
          }
      },
      BOpData::M(mop_data) => match_rd! {
          target: mop_data,
          op_with_rds: [
              Li, La, Mv, FmvS,
            Add, Sub, Addi, Addw, Subw, Mulw, Divw, Remw,
              Slliw, Srliw, Sraiw,
              Sllw, Srlw, Sraw,
              Slt, Slti, Sltu, Sltiu,
              Addiw,
              Xor, Xori,
              FaddS, FsubS, FmulS, FdivS,
              FeqS, FltS, FleS, FneS, FgtS, FgeS,
              FcvtWS, FcvtSW, FmvWX, FmvXW,
              Lw, Flw, Ld
          ],
          rd_arm: MOpData(rd) => {
              Some((rd, 0))
          },
          fallback: {
              // For other MOpData which doesn't have rd field (e.g. J and Call), we return Undef.
              MOpData::Sw {..}
              | MOpData::Fsw {..}
              | MOpData::Sd {..}
              | MOpData::J {..}
              | MOpData::Bnez {..}
              | MOpData::Call {..}
              | MOpData::Ret
              | MOpData::Beq {..}
              | MOpData::Bne {..}
              | MOpData::Blt {..}
              | MOpData::Bge {..}
              | MOpData::Bltu {..}
              | MOpData::Bgeu {..} => None,
          }
      },
    }
  }

  pub fn get_rd_mut(&mut self, lop_id: BOperand) -> Option<&mut BOperand> {
    self.get_rd_tuple_mut(lop_id).map(|(rd, _)| rd)
  }

  pub fn get_src_tuple(&self, lop_id: BOperand) -> Vec<(&BOperand, usize)> {
    let bop = &self[lop_id];

    match &bop.data {
      BOpData::L(lop_data) => match_src! {
          target: lop_data,
          bin_ops: [
              AddI, SubI, MulI, DivI, ModI,
              SNe, SEq, SGt, SLt, SGe, SLe,
              Xor, Shl, Shr, Sar,
              AddF, SubF, MulF, DivF,
              ONe, OEq, OGt, OLt, OGe, OLe
          ],
          bin_arm: LOpData { lhs, rhs } => {
              vec![(lhs, 1), (rhs, 2)]
          },
          un_ops: [Sitofp, Fptosi],
          un_arm: LOpData { value } => {
              vec![(value, 1)]
          },
          fallback: {
              LOpData::Store { addr, value } => vec![(addr, 0), (value, 1)],
              LOpData::Load { addr, .. } => vec![(addr, 1)],
              LOpData::Move { src, .. } => vec![(src, 1)],
              LOpData::Br { cond, .. } => vec![(cond, 0)],
              LOpData::Call { .. }
              | LOpData::Jump { .. }
              | LOpData::Ret
              | LOpData::LoadIntImm { .. }
              | LOpData::LoadFloatImm { .. }
              | LOpData::LoadAddress { .. } => vec![],
          }
      },
      BOpData::M(mop_data) => match_src! {
          target: mop_data,
          bin_ops: [
              Add, Sub, Addw, Subw, Mulw, Divw, Remw,
              Sllw, Srlw, Sraw,
              Slt, Sltu, Xor,
              FaddS, FsubS, FmulS, FdivS,
              FeqS, FltS, FleS, FneS, FgtS, FgeS
          ],
          bin_arm: MOpData { rs1, rs2 } => {
              vec![(rs1, 1), (rs2, 2)]
          },
          un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
          un_arm: MOpData { rs } => {
              vec![(rs, 1)]
          },
          fallback: {
              MOpData::Addi { rs1, imm, .. }
              | MOpData::Slti { rs1, imm, .. }
              | MOpData::Sltiu { rs1, imm, .. }
              | MOpData::Addiw { rs1, imm, .. }
              | MOpData::Slliw { rs1, imm, .. }
              | MOpData::Srliw { rs1, imm, .. }
              | MOpData::Sraiw { rs1, imm, .. }
              | MOpData::Xori { rs1, imm, .. } => vec![(rs1, 1), (imm, 2)],

              MOpData::Lw { base, offset, .. }
              | MOpData::Flw { base, offset, .. }
              | MOpData::Ld { base, offset, .. } => vec![(base, 1), (offset, 2)],

              MOpData::Sw { rs, base, offset }
              | MOpData::Fsw { rs, base, offset }
              | MOpData::Sd { rs, base, offset } => vec![(rs, 0), (base, 1), (offset, 2)],

              MOpData::Beq { rs1, rs2, offset }
              | MOpData::Bne { rs1, rs2, offset }
              | MOpData::Blt { rs1, rs2, offset }
              | MOpData::Bge { rs1, rs2, offset }
              | MOpData::Bltu { rs1, rs2, offset }
              | MOpData::Bgeu { rs1, rs2, offset } => vec![(rs1, 0), (rs2, 1), (offset, 2)],

              MOpData::Bnez { rs, .. } => vec![(rs, 0)],

              MOpData::Li { .. }
              | MOpData::La { .. }
              | MOpData::Call { .. }
              | MOpData::Ret
              | MOpData::J { .. } => vec![],
          }
      },
    }
  }

  pub fn get_src(&self, lop_id: BOperand) -> Vec<&BOperand> {
    self
      .get_src_tuple(lop_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }

  pub fn get_src_tuple_mut(&mut self, lop_id: BOperand) -> Vec<(&mut BOperand, usize)> {
    let bop = &mut self[lop_id];

    match &mut bop.data {
      BOpData::L(lop_data) => match_src! {
          target: lop_data,
          bin_ops: [
              AddI, SubI, MulI, DivI, ModI,
              SNe, SEq, SGt, SLt, SGe, SLe,
              Xor, Shl, Shr, Sar,
              AddF, SubF, MulF, DivF,
              ONe, OEq, OGt, OLt, OGe, OLe
          ],
          bin_arm: LOpData { lhs, rhs } => {
              vec![(lhs, 1), (rhs, 2)]
          },
          un_ops: [Sitofp, Fptosi],
          un_arm: LOpData { value } => {
              vec![(value, 1)]
          },
          fallback: {
              LOpData::Store { addr, value } => vec![(addr, 0), (value, 1)],
              LOpData::Load { addr, .. } => vec![(addr, 1)],
              LOpData::Move { src, .. } => vec![(src, 1)],
              LOpData::Br { cond, .. } => vec![(cond, 0)],

              LOpData::Call { .. }
              | LOpData::Jump { .. }
              | LOpData::Ret
              | LOpData::LoadIntImm { .. }
              | LOpData::LoadFloatImm { .. }
              | LOpData::LoadAddress { .. } => vec![],
          }
      },
      BOpData::M(mop_data) => match_src! {
          target: mop_data,
          bin_ops: [
              Add, Sub, Addw, Subw, Mulw, Divw, Remw,
              Sllw, Srlw, Sraw,
              Slt, Sltu, Xor,
              FaddS, FsubS, FmulS, FdivS,
              FeqS, FltS, FleS, FneS, FgtS, FgeS
          ],
          bin_arm: MOpData { rs1, rs2 } => {
              vec![(rs1, 1), (rs2, 2)]
          },
          un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
          un_arm: MOpData { rs } => {
              vec![(rs, 1)]
          },
          fallback: {
              MOpData::Addi { rs1, imm, .. }
              | MOpData::Slti { rs1, imm, .. }
              | MOpData::Sltiu { rs1, imm, .. }
              | MOpData::Addiw { rs1, imm, .. }
              | MOpData::Slliw { rs1, imm, .. }
              | MOpData::Srliw { rs1, imm, .. }
              | MOpData::Sraiw { rs1, imm, .. }
              | MOpData::Xori { rs1, imm, .. } => vec![(rs1, 1), (imm, 2)],

              MOpData::Lw { base, offset, .. }
              | MOpData::Flw { base, offset, .. }
              | MOpData::Ld { base, offset, .. } => vec![(base, 1), (offset, 2)],

              MOpData::Sw { rs, base, offset }
              | MOpData::Fsw { rs, base, offset }
              | MOpData::Sd { rs, base, offset } => vec![(rs, 0), (base, 1), (offset, 2)],

              MOpData::Li { .. } | MOpData::La { .. } => vec![],

              MOpData::Beq { rs1, rs2, offset }
              | MOpData::Bne { rs1, rs2, offset }
              | MOpData::Blt { rs1, rs2, offset }
              | MOpData::Bge { rs1, rs2, offset }
              | MOpData::Bltu { rs1, rs2, offset }
              | MOpData::Bgeu { rs1, rs2, offset } => vec![(rs1, 0), (rs2, 1), (offset, 2)],

              MOpData::Bnez { rs, .. } => vec![(rs, 0)],

              MOpData::Call { .. }
              | MOpData::Ret
              | MOpData::J { .. } => vec![],
          }
      },
    }
  }

  pub fn get_src_mut(&mut self, lop_id: BOperand) -> Vec<&mut BOperand> {
    self
      .get_src_tuple_mut(lop_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }
}

impl From<BOperand> for usize {
  fn from(operand: BOperand) -> Self {
    match operand {
      BOperand::Func(id) => id,
      BOperand::BB(id) => id,
      BOperand::Inst(id) => id,
      BOperand::Reg(Reg::Virt(id)) => id,
      _ => panic!("Cannot convert operand {:?} to usize", operand),
    }
  }
}

impl Arena<BOp> for BDFG {
  fn remove(&mut self, idx: usize) -> BOp {
    if let ArenaItem::Data(data) = std::mem::replace(&mut self.storage[idx], ArenaItem::None) {
      data
    } else {
      panic!("BDFG remove: index {} points to None or NewIndex", idx);
    }
  }
  fn gc(&mut self) -> Vec<ArenaItem<BOp>> {
    let new_arena: Vec<ArenaItem<BOp>> = vec![];
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
      "BDFG GC: {} instructions collected, recycle rate: {:.2}%",
      old_arena.len() - self.storage.len(),
      (old_arena.len() - self.storage.len()) as f64 / old_arena.len() as f64 * 100.0
    );

    // No entry. No need to remap.
    // Values of BOp are virtual registers, which should be remapped outside of this function.

    old_arena
  }
}
