//! Type definitions of BackIR.

use crate::base::Type;
use crate::config::RISCV_BITS;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BType {
  Void,
  I32,
  F32,
  // For saving of fp registers.
  F64,
  // For pointer
  U64,
  // For array
  Array { base: Box<BType>, num: u32 },
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
      BType::Array { base, num } => base.size() * num,
    }
  }
  #[inline(always)]
  pub fn align(&self) -> u32 {
    match self {
      BType::Void => 1,
      BType::I32 => 4,
      BType::F32 => 4,
      BType::F64 => RISCV_BITS / 8,
      BType::U64 => RISCV_BITS / 8,
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
      Type::Array { base, dims } => BType::Array {
        base: Box::new(BType::from(*base)),
        num: dims.iter().product::<u32>(),
      },
      Type::Function { .. } => {
        unimplemented!("Function type is not supported in BType")
      }
      Type::Pointer { .. } => BType::U64,
      Type::Char => BType::I32, // char is represented as i32 in machine code
    }
  }
}
