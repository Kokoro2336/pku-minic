//! Type definitions of BackIR.

use crate::base::Type;
use crate::config::RISCV_BITS;
use crate::ir::back::BType::{V4F32, V4I32};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BType {
  Void,
  I32,
  F32,
  /// For saving of fp registers.
  F64,
  /// For pointer
  U64,
  /// Vector
  V4I32,
  V4F32,
  /// For array
  Array {
    base: Box<BType>,
    num: u32,
  },
}

impl BType {
  #[inline(always)]
  pub fn size(&self) -> u32 {
    match self {
      BType::Void => 1, // align to 1 byte for void type
      BType::I32 => 4,
      BType::F32 => 4,
      BType::F64 => RISCV_BITS / 8,
      BType::U64 => RISCV_BITS / 8,
      BType::V4F32 | BType::V4I32 => 128,
      BType::Array { base, num } => base.size() * num,
    }
  }
  #[inline(always)]
  pub fn is_float(&self) -> bool {
    matches!(self, BType::F32 | BType::F64)
  }
  #[inline(always)]
  pub fn align(&self) -> u32 {
    match self {
      BType::Void => 1,
      BType::I32 => 4,
      BType::F32 => 4,
      BType::F64 => RISCV_BITS / 8,
      BType::U64 => RISCV_BITS / 8,
      BType::V4F32 | BType::V4I32 => 128,
      BType::Array { base, .. } => base.align(),
    }
  }
}

impl From<Type> for BType {
  fn from(ty: Type) -> Self {
    match ty {
      Type::Int => BType::I32,
      Type::Float => BType::F32,
      Type::Void => BType::Void,
      Type::Bool => BType::I32, // bool is represented as i32 in machine code
      Type::Vector { base, elems: 4 } => match *base {
        Type::Int | Type::Bool => V4I32,
        Type::Float => V4F32,
        _ => unimplemented!(),
      },
      Type::Array { base, dims } => BType::Array {
        base: Box::new(BType::from(*base)),
        num: dims.iter().product::<u32>(),
      },
      Type::Pointer { .. } => BType::U64,
      Type::Char => BType::I32, // char is represented as i32 in machine code
      other => {
        unimplemented!("{:?} is not supported in BType", other)
      }
    }
  }
}
