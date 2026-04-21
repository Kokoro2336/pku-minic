//! Type definitions.

use std::cmp::PartialEq;
use std::hash::{Hash, Hasher};

const RISCV_BITS: u32 = 64;

/// type of value
#[derive(Debug, Clone, Eq, PartialOrd, Ord)]
pub enum Type {
  Int,
  Void,
  Float,
  Bool,
  Array {
    base: Box<Type>,
    dims: Vec<u32>,
  },
  Pointer {
    base: Box<Type>,
  },
  Function {
    return_type: Box<Type>,
    param_types: Vec<Type>,
  },
  // only occurs in SysY lib function
  Char, /*u8*/
}

impl PartialEq for Type {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (Type::Int, Type::Int) => true,
      (Type::Float, Type::Float) => true,
      (Type::Void, Type::Void) => true,
      (Type::Bool, Type::Bool) => true,
      (
        Type::Array {
          base: base1,
          dims: dims1,
        },
        Type::Array {
          base: base2,
          dims: dims2,
        },
      ) => base1 == base2 && dims1 == dims2,
      (
        Type::Function {
          return_type: return_type1,
          param_types: param_types1,
        },
        Type::Function {
          return_type: return_type2,
          param_types: param_types2,
        },
      ) => return_type1 == return_type2 && param_types1 == param_types2,
      (Type::Char, Type::Char) => true,

      (Type::Pointer { base: base1 }, Type::Pointer { base: base2 }) => {
        // Special treatment for pointer type: if the base type is array, we only compare the base type of the array, ignoring the dimensions.
        let base1 = if let Type::Array { base: arr_base, .. } = (**base1).clone() {
          *arr_base
        } else {
          (**base1).clone()
        };
        let base2 = if let Type::Array { base: arr_base, .. } = (**base2).clone() {
          *arr_base
        } else {
          (**base2).clone()
        };
        base1 == base2
      }
      _ => false,
    }
  }
}

impl Hash for Type {
  fn hash<H: Hasher>(&self, state: &mut H) {
    // Hash the discriminant of the enum to distinguish different variants
    std::mem::discriminant(self).hash(state);

    // Hash the fields of the enum variant
    match self {
      Type::Int | Type::Void | Type::Float | Type::Bool | Type::Char => { /*do nothing for scalar*/
      }
      Type::Array { base, dims } => {
        base.hash(state);
        dims.hash(state);
      }
      Type::Function {
        return_type,
        param_types,
      } => {
        return_type.hash(state);
        param_types.hash(state);
      }
      Type::Pointer { base } => {
        // Special treatment for pointer type: if the base type is array, we only hash the base type of the array, ignoring the dimensions.
        let base = if let Type::Array { base: arr_base, .. } = (**base).clone() {
          *arr_base
        } else {
          (**base).clone()
        };
        base.hash(state);
      }
    }
  }
}

impl std::fmt::Display for Type {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Type::Int => write!(f, "int"),
      Type::Float => write!(f, "float"),
      Type::Void => write!(f, "void"),
      Type::Bool => write!(f, "bool"),
      Type::Array { base, dims } => {
        write!(f, "{}", base)?;
        for dim in dims {
          write!(f, "[{}]", dim)?;
        }
        Ok(())
      }
      Type::Pointer { base } => {
        write!(f, "{}*", base)
      }
      Type::Function {
        return_type,
        param_types,
      } => {
        write!(f, "fn(")?;
        for (i, param) in param_types.iter().enumerate() {
          write!(f, "{}", param)?;
          if i != param_types.len() - 1 {
            write!(f, ", ")?;
          }
        }
        write!(f, ") -> {}", return_type)
      }
      Type::Char => {
        write!(f, "char")
      }
    }
  }
}

impl Type {
  #[inline(always)]
  pub fn size(&self) -> u32 {
    match self {
      Type::Bool => 1,
      Type::Int => 4,
      Type::Float => 4,
      Type::Void => 0,
      Type::Array { base, dims } => base.size() * dims.iter().product::<u32>(),
      Type::Pointer { .. } => RISCV_BITS / 8,
      Type::Function { .. } => panic!("Function type has no size"),
      Type::Char => 1,
    }
  }
  #[inline(always)]
  pub fn align(&self) -> u32 {
    match self {
      Type::Bool => 1,
      Type::Int => 4,
      Type::Float => 4,
      Type::Void => 1, // align to 1 byte for void type
      Type::Array { base, .. } => base.align(),
      Type::Pointer { .. } => RISCV_BITS / 8,
      Type::Function { .. } => panic!("Function type has no alignment"),
      Type::Char => 1,
    }
  }
  #[inline(always)]
  pub fn is_scalar(&self) -> bool {
    matches!(self, Type::Int | Type::Float | Type::Char | Type::Bool)
  }
  /// Compute the size of the subarray starting from the given dimension index.
  pub fn subarr_size(&self, dim_idx: usize) -> u32 {
    match self {
      Type::Array { base, dims } => {
        if dim_idx > dims.len() {
          panic!(
            "Dimension index out of bounds. Array has only {} dimensions, but got index {}.",
            dims.len(),
            dim_idx
          );
        } else if dim_idx == dims.len() {
          return base.size();
        }
        base.size() * dims[dim_idx..].iter().product::<u32>()
      }
      _ => panic!("subarr_size can only be called on array types"),
    }
  }
}
