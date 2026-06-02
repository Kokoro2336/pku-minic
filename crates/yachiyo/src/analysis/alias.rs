//! Alias Analysis.

use crate::base::Type;
use crate::ir::mid::Operand;

use rustc_hash::FxHashMap;
use std::cmp::PartialEq;
use std::ops::{AddAssign, SubAssign};

#[derive(Default, Clone)]
pub struct MemLoc {
  pub base: Operand,
  pub offset: AffineExpr,
  /// type won't change once MemLoc is created.
  pub size: u32,
}

impl MemLoc {
  pub fn new(ty: Type) -> Self {
    Self {
      size: ty.size(),
      ..Self::default()
    }
  }

  pub fn set_unknown(&mut self, op: Operand) {
    self.base = Operand::Undefined;
    self.offset = AffineExpr::Unknown(op);
  }
}

impl PartialEq for MemLoc {
  fn eq(&self, other: &Self) -> bool {
    let sub_res = self.offset.sub(&other.offset);
    self.base == other.base && sub_res == AffineExpr::zero() && self.size == other.size
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum AliasResult {
  NoAlias,
  MayAlias,
  MustAlias,
}

#[derive(Clone, PartialEq, Eq)]
pub enum AffineExpr {
  L(LinearExpr),
  /// Operand: The source of unknown offset
  Unknown(Operand),
}

impl Default for AffineExpr {
  fn default() -> Self {
    AffineExpr::L(LinearExpr::default())
  }
}

impl From<LinearExpr> for AffineExpr {
  fn from(expr: LinearExpr) -> Self {
    AffineExpr::L(expr)
  }
}

impl AffineExpr {
  pub fn zero() -> Self {
    AffineExpr::L(LinearExpr::zero())
  }

  pub fn from_gep(typ: Type, indices: Vec<Operand>) -> Self {
    LinearExpr::from_gep(typ, indices).into()
  }

  pub fn add_assign(&mut self, other: &AffineExpr) {
    match (self, other) {
      (AffineExpr::L(e1), AffineExpr::L(e2)) => e1.add_assign(e2),
      (this, AffineExpr::Unknown(op)) => *this = AffineExpr::Unknown(*op),
      _ => {}
    }
  }

  pub fn add(&self, other: &AffineExpr) -> Self {
    let mut result = self.clone();
    result.add_assign(other);
    result
  }

  pub fn sub_assign(&mut self, other: &AffineExpr) {
    match (self, other) {
      (AffineExpr::L(e1), AffineExpr::L(e2)) => e1.sub_assign(e2),
      (this, AffineExpr::Unknown(op)) => *this = AffineExpr::Unknown(*op),
      _ => {}
    }
  }

  pub fn sub(&self, other: &AffineExpr) -> Self {
    let mut result = self.clone();
    result.sub_assign(other);
    result
  }

  pub fn mul_const(&self, coeff: i64) -> Self {
    match self {
      AffineExpr::L(e) => AffineExpr::L(e.mul_const(coeff)),
      AffineExpr::Unknown(op) => AffineExpr::Unknown(*op),
    }
  }

  pub fn diff_constant(&self, other: &Self) -> Option<i64> {
    match (self, other) {
      (AffineExpr::L(e1), AffineExpr::L(e2)) => e1.diff_constant(e2),
      _ => None,
    }
  }

  pub fn get_keys(&self) -> Option<impl Iterator<Item = &Operand>> {
    match self {
      AffineExpr::L(e) => Some(e.get_keys()),
      AffineExpr::Unknown(_) => None,
    }
  }
}

impl AddAssign<&AffineExpr> for AffineExpr {
  fn add_assign(&mut self, rhs: &AffineExpr) {
    AffineExpr::add_assign(self, rhs);
  }
}

impl SubAssign<&AffineExpr> for AffineExpr {
  fn sub_assign(&mut self, rhs: &AffineExpr) {
    AffineExpr::sub_assign(self, rhs);
  }
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct LinearExpr {
  pub constant: i64,
  pub terms: FxHashMap<Operand, i64>,
}

impl LinearExpr {
  pub fn zero() -> Self {
    Self {
      constant: 0,
      terms: FxHashMap::default(),
    }
  }

  /// Param:
  /// - `typ`: The type of the pointer operand used by GEP.
  /// - `indices`: The indices of GEP instruction.
  pub fn from_gep(typ: Type, indices: Vec<Operand>) -> Self {
    let mut result = Self::zero();
    let mut cur_typ = typ;

    for index in indices {
      let (step_size, next_typ) = match cur_typ {
        Type::Pointer { base } => {
          let pointee = *base;
          (pointee.size() as i64, pointee)
        }
        Type::Array { base, dims } => {
          if dims.is_empty() {
            panic!("GEP array type should have at least one dimension");
          }

          let arr_typ = Type::Array {
            base: base.clone(),
            dims: dims.clone(),
          };
          let step_size = arr_typ.subarr_size(1) as i64;
          let next_typ = if dims.len() == 1 {
            *base
          } else {
            Type::Array {
              base,
              dims: dims[1..].to_vec(),
            }
          };
          (step_size, next_typ)
        }
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::Char
        | Type::Void
        | Type::Function { .. }
        | Type::Vector { .. } => {
          panic!("GEP index out of bounds for type: {:?}", cur_typ)
        }
      };

      result.add_scaled_index(index, step_size);
      cur_typ = next_typ;
    }

    result
  }

  pub fn constant(constant: i64) -> Self {
    Self {
      constant,
      terms: FxHashMap::default(),
    }
  }

  pub fn add_term(&mut self, val: Operand, coeff: i64) {
    if coeff == 0 {
      return;
    }
    let entry = self.terms.entry(val).or_insert(0);
    *entry += coeff;

    if *entry == 0 {
      self.terms.remove(&val);
    }
  }

  fn add_scaled_index(&mut self, index: Operand, scale: i64) {
    match index {
      Operand::Int(i) => self.constant += i as i64 * scale,
      Operand::Bool(b) => self.constant += b as i64 * scale,
      Operand::Value(_) | Operand::Param(_) => self.add_term(index, scale),
      Operand::Float(_)
      | Operand::Global(_)
      | Operand::Func(_)
      | Operand::BB(_)
      | Operand::Undefined => panic!("Invalid GEP index operand: {:?}", index),
    }
  }

  pub fn normalize(&mut self) {
    self.terms.retain(|_, &mut coeff| coeff != 0);
  }

  pub fn var(val: Operand) -> Self {
    let mut terms = FxHashMap::default();
    terms.insert(val, 1);
    Self { constant: 0, terms }
  }

  pub fn add_assign(&mut self, other: &LinearExpr) {
    self.constant += other.constant;
    for (val, coeff) in &other.terms {
      self.add_term(*val, *coeff);
    }
  }

  pub fn add(&self, other: &LinearExpr) -> Self {
    let mut result = self.clone();
    result.add_assign(other);
    result
  }

  pub fn sub_assign(&mut self, other: &LinearExpr) {
    self.constant -= other.constant;
    for (val, coeff) in &other.terms {
      self.add_term(*val, -*coeff);
    }
  }

  pub fn sub(&self, other: &LinearExpr) -> Self {
    let mut result = self.clone();
    result.sub_assign(other);
    result
  }

  pub fn mul_const(&self, coeff: i64) -> Self {
    if coeff == 0 {
      return Self::zero();
    }
    let mut result = self.clone();
    result.constant *= coeff;
    for coeff in result.terms.values_mut() {
      *coeff *= *coeff;
    }
    result
  }

  pub fn neg(&self) -> Self {
    self.mul_const(-1)
  }

  pub fn is_constant(&self) -> bool {
    self.terms.is_empty()
  }

  pub fn diff_constant(&self, other: &Self) -> Option<i64> {
    let diff_res = self.sub(other);
    if diff_res.is_constant() {
      Some(diff_res.constant)
    } else {
      None
    }
  }

  pub fn get_keys(&self) -> impl Iterator<Item = &Operand> {
    self.terms.keys()
  }
}

impl AddAssign<&LinearExpr> for LinearExpr {
  fn add_assign(&mut self, rhs: &LinearExpr) {
    LinearExpr::add_assign(self, rhs);
  }
}

impl SubAssign<&LinearExpr> for LinearExpr {
  fn sub_assign(&mut self, rhs: &LinearExpr) {
    LinearExpr::sub_assign(self, rhs);
  }
}

/// For MayAlias and NoAlias.
pub enum RangeRelation {
  Overlap,
  Disjoint,
  Unknown,
}

impl RangeRelation {
  pub fn check(a: &MemLoc, b: &MemLoc) -> Self {
    b.offset
      .diff_constant(&a.offset)
      .map(|diff| {
        if diff >= a.size as i64 || diff + b.size as i64 <= 0 {
          RangeRelation::Disjoint
        } else {
          RangeRelation::Overlap
        }
      })
      .unwrap_or(RangeRelation::Unknown)
  }
}
