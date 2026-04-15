//! Instruction definition of Lower IR.

use crate::ir::back::{BOpData, BOperand, BType};
use strum_macros::EnumDiscriminants;

#[derive(Debug, Clone, EnumDiscriminants)]
// Specify the type enum's name
#[strum_discriminants(name(LOpType))]
#[strum_discriminants(derive(Hash, Ord, PartialOrd))]
#[allow(clippy::upper_case_acronyms)]
pub enum LOpData {
  /* regular instructions */
  /// Integer
  AddI {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SubI {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  MulI {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  DivI {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  ModI {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },

  // The comparisons are logical.
  Xor {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },

  // Comparison(S: Signed. And SysY only has signed comparison)
  SNe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SEq {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SGt {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SLt {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SGe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SLe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },

  // Bitwise shift
  Shl {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  Shr {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  Sar {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },

  /// Float
  AddF {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  SubF {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  MulF {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  DivF {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  // Mod is invalid for float in SysY

  // On the language level, SysY doesn't support And, Or, Xor for float

  // Comparison. SysY doesn't support NaN, so we only have one type of comparison here.
  ONe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  OEq {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  OGt {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  OLt {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  OGe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },
  OLe {
    rd: BOperand,
    lhs: BOperand,
    rhs: BOperand,
  },

  /// Cast operations
  Sitofp {
    rd: BOperand,
    value: BOperand,
  }, // int to float
  Fptosi {
    rd: BOperand,
    value: BOperand,
  }, // float to int

  // SysY doesn't support bitwise shift for float
  /// Memory operations
  Store {
    addr: BOperand,
    value: BOperand,
    /// For frame lowering, we need to know the value type to determine the store instruction.
    val_typ: BType,
  },
  Load {
    rd: BOperand,
    addr: BOperand,
  },
  Move {
    rd: BOperand,
    src: BOperand,
  },

  // Immediate Loading
  /// Int immediate
  LoadIntImm {
    rd: BOperand,
    imm: i32,
  },
  /// Float immediate
  LoadFloatImm {
    rd: BOperand,
    imm: f32,
  },

  /// La for Bss/RoData/Data
  LoadAddress {
    rd: BOperand,
    addr: BOperand,
  },

  /// Control flow
  /// Call has no return value in Lower IR.
  Call {
    func: BOperand,
  },
  Br {
    cond: BOperand,
    then_bb: BOperand,
    else_bb: BOperand,
  },
  Jump {
    target_bb: BOperand,
  },
  Ret,
}

impl LOpData {
  pub fn is_rel(&self) -> bool {
    matches!(
      self,
      LOpData::SNe { .. }
        | LOpData::SEq { .. }
        | LOpData::SGt { .. }
        | LOpData::SLt { .. }
        | LOpData::SGe { .. }
        | LOpData::SLe { .. }
        | LOpData::ONe { .. }
        | LOpData::OEq { .. }
        | LOpData::OGt { .. }
        | LOpData::OLt { .. }
        | LOpData::OGe { .. }
        | LOpData::OLe { .. }
    )
  }

  pub fn is_impure(&self) -> bool {
    matches!(
      self,
      LOpData::Store { .. }
        | LOpData::Call { .. }
        | LOpData::Br { .. }
        | LOpData::Jump { .. }
        | LOpData::Ret
    )
  }
}

impl std::fmt::Display for LOpData {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      LOpData::AddI { rd, lhs, rhs } => write!(f, "addI {rd}, {lhs}, {rhs}"),
      LOpData::SubI { rd, lhs, rhs } => write!(f, "subI {rd}, {lhs}, {rhs}"),
      LOpData::MulI { rd, lhs, rhs } => write!(f, "mulI {rd}, {lhs}, {rhs}"),
      LOpData::DivI { rd, lhs, rhs } => write!(f, "divI {rd}, {lhs}, {rhs}"),
      LOpData::ModI { rd, lhs, rhs } => write!(f, "modI {rd}, {lhs}, {rhs}"),
      LOpData::Xor { rd, lhs, rhs } => write!(f, "xor {rd}, {lhs}, {rhs}"),
      LOpData::SNe { rd, lhs, rhs } => write!(f, "sne {rd}, {lhs}, {rhs}"),
      LOpData::SEq { rd, lhs, rhs } => write!(f, "seq {rd}, {lhs}, {rhs}"),
      LOpData::SGt { rd, lhs, rhs } => write!(f, "sgt {rd}, {lhs}, {rhs}"),
      LOpData::SLt { rd, lhs, rhs } => write!(f, "slt {rd}, {lhs}, {rhs}"),
      LOpData::SGe { rd, lhs, rhs } => write!(f, "sge {rd}, {lhs}, {rhs}"),
      LOpData::SLe { rd, lhs, rhs } => write!(f, "sle {rd}, {lhs}, {rhs}"),
      LOpData::Shl { rd, lhs, rhs } => write!(f, "shl {rd}, {lhs}, {rhs}"),
      LOpData::Shr { rd, lhs, rhs } => write!(f, "shr {rd}, {lhs}, {rhs}"),
      LOpData::Sar { rd, lhs, rhs } => write!(f, "sar {rd}, {lhs}, {rhs}"),
      LOpData::AddF { rd, lhs, rhs } => write!(f, "addF {rd}, {lhs}, {rhs}"),
      LOpData::SubF { rd, lhs, rhs } => write!(f, "subF {rd}, {lhs}, {rhs}"),
      LOpData::MulF { rd, lhs, rhs } => write!(f, "mulF {rd}, {lhs}, {rhs}"),
      LOpData::DivF { rd, lhs, rhs } => write!(f, "divF {rd}, {lhs}, {rhs}"),
      LOpData::ONe { rd, lhs, rhs } => write!(f, "one {rd}, {lhs}, {rhs}"),
      LOpData::OEq { rd, lhs, rhs } => write!(f, "oeq {rd}, {lhs}, {rhs}"),
      LOpData::OGt { rd, lhs, rhs } => write!(f, "ogt {rd}, {lhs}, {rhs}"),
      LOpData::OLt { rd, lhs, rhs } => write!(f, "olt {rd}, {lhs}, {rhs}"),
      LOpData::OGe { rd, lhs, rhs } => write!(f, "oge {rd}, {lhs}, {rhs}"),
      LOpData::OLe { rd, lhs, rhs } => write!(f, "ole {rd}, {lhs}, {rhs}"),
      LOpData::Sitofp { rd, value } => write!(f, "sitofp {rd}, {value}"),
      LOpData::Fptosi { rd, value } => write!(f, "fptosi {rd}, {value}"),
      LOpData::Store { addr, value, .. } => write!(f, "store {addr}, {value}"),
      LOpData::Load { rd, addr } => write!(f, "load {rd}, {addr}"),
      LOpData::Move { rd, src } => write!(f, "move {rd}, {src}"),
      LOpData::LoadIntImm { rd, imm } => write!(f, "loadIntImm {rd}, {imm}"),
      LOpData::LoadFloatImm { rd, imm } => write!(f, "loadFloatImm {rd}, {imm}"),
      LOpData::LoadAddress { rd, addr } => write!(f, "loadAddress {rd}, {addr}"),
      LOpData::Call { func } => write!(f, "call {func}"),
      LOpData::Br {
        cond,
        then_bb,
        else_bb,
      } => write!(f, "br {cond}, {then_bb}, {else_bb}"),
      LOpData::Jump { target_bb } => write!(f, "jump {target_bb}"),
      LOpData::Ret => write!(f, "ret"),
    }
  }
}

impl From<LOpData> for BOpData {
  fn from(op_data: LOpData) -> Self {
    BOpData::L(op_data)
  }
}

impl From<BOpData> for LOpData {
  fn from(op_data: BOpData) -> Self {
    match op_data {
      BOpData::L(l_op_data) => l_op_data,
      _ => panic!("Cannot convert MOpData to LOpData"),
    }
  }
}
