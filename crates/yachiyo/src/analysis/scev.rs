//! Scalar Evolution(SCEV).

use super::LoopId;
use crate::ir::mid::Operand;
use crate::utils::IndexedArena;

use rustc_hash::FxHashMap;
use std::ops::{Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SCEVId(usize);

impl From<usize> for SCEVId {
  fn from(value: usize) -> Self {
    SCEVId(value)
  }
}

impl From<SCEVId> for usize {
  fn from(id: SCEVId) -> Self {
    id.0
  }
}

#[derive(Debug, Clone)]
pub struct AddRecInfo {
  pub start: SCEVId,
  pub step: SCEVId,
  pub iv: Operand,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SCEVExpr {
  Const(i64),
  Unknown(Operand),
  Add(Vec<SCEVId>),
  Mul(Vec<SCEVId>),
  AddRec {
    start: SCEVId,
    step: SCEVId,
    loop_id: LoopId,
    phi_id: Operand,
  },
}

impl SCEVExpr {
  pub fn is_const(&self) -> bool {
    matches!(self, SCEVExpr::Const(_))
  }

  pub fn as_const(&self) -> Option<i64> {
    if let SCEVExpr::Const(c) = self {
      Some(*c)
    } else {
      None
    }
  }

  pub fn zero() -> Self {
    SCEVExpr::Const(0)
  }

  pub fn one() -> Self {
    SCEVExpr::Const(1)
  }

  pub fn is_unknown(&self) -> bool {
    matches!(self, SCEVExpr::Unknown(_))
  }

  pub fn as_unknown(&self) -> Option<Operand> {
    if let SCEVExpr::Unknown(op) = self {
      Some(*op)
    } else {
      None
    }
  }
}

pub struct SCEVArena {
  exprs: IndexedArena<SCEVExpr>,
  expr_to_id: FxHashMap<SCEVExpr, SCEVId>,
  pub zero: SCEVId,
  pub one: SCEVId,
}

impl Index<SCEVId> for SCEVArena {
  type Output = SCEVExpr;

  fn index(&self, index: SCEVId) -> &Self::Output {
    &self.exprs[index.0]
  }
}

impl IndexMut<SCEVId> for SCEVArena {
  fn index_mut(&mut self, index: SCEVId) -> &mut Self::Output {
    &mut self.exprs[index.0]
  }
}

impl Index<SCEVId> for IndexedArena<SCEVExpr> {
  type Output = SCEVExpr;

  fn index(&self, index: SCEVId) -> &Self::Output {
    &self[index.0]
  }
}

impl IndexMut<SCEVId> for IndexedArena<SCEVExpr> {
  fn index_mut(&mut self, index: SCEVId) -> &mut Self::Output {
    &mut self[index.0]
  }
}

impl Default for SCEVArena {
  fn default() -> Self {
    let mut scev = SCEVArena {
      exprs: IndexedArena::new(),
      expr_to_id: FxHashMap::default(),
      zero: SCEVId(0),
      one: SCEVId(0),
    };
    scev.zero = scev.dedup(SCEVExpr::zero());
    scev.one = scev.dedup(SCEVExpr::one());
    scev
  }
}

impl SCEVArena {
  pub fn const_from(&mut self, c: i64) -> SCEVId {
    match c {
      0 => self.zero,
      1 => self.one,
      _ => self.dedup(SCEVExpr::Const(c)),
    }
  }

  pub fn add(&mut self, terms: impl Iterator<Item = SCEVId>) -> SCEVId {
    let mut flat_terms = Vec::new();
    let mut const_term = 0;

    for term in terms {
      match &self[term] {
        SCEVExpr::Const(c) => const_term += c,
        SCEVExpr::Add(inner_terms) => flat_terms.extend(inner_terms.iter().cloned()),
        _ => flat_terms.push(term),
      }
    }

    if const_term != 0 {
      let const_id = self.const_from(const_term);
      flat_terms.push(const_id);
    }

    // Canonicalize
    flat_terms.retain(|&term| !matches!(self[term], SCEVExpr::Const(0)));
    flat_terms.sort_by_key(|&SCEVId(id)| id);

    match flat_terms.len() {
      0 => self.zero,
      1 => flat_terms[0],
      _ => self.dedup(SCEVExpr::Add(flat_terms)),
    }
  }

  pub fn mul(&mut self, factors: impl Iterator<Item = SCEVId>) -> SCEVId {
    let mut flat_factors = Vec::new();
    let mut const_factor = 1;

    for factor in factors {
      match &self[factor] {
        SCEVExpr::Const(c) => {
          if *c == 0 {
            return self.zero;
          } else {
            const_factor *= c;
          }
        }
        SCEVExpr::Mul(inner_factors) => flat_factors.extend(inner_factors.iter().cloned()),
        _ => flat_factors.push(factor),
      }
    }

    if const_factor == 0 {
      return self.zero;
    }

    if const_factor != 1 {
      let const_id = self.const_from(const_factor);
      flat_factors.push(const_id);
    }

    // Canonicalize
    flat_factors.retain(|&factor| !matches!(self[factor], SCEVExpr::Const(1)));
    flat_factors.sort_by_key(|&SCEVId(id)| id);

    match flat_factors.len() {
      0 => self.one,
      1 => flat_factors[0],
      _ => self.dedup(SCEVExpr::Mul(flat_factors)),
    }
  }

  pub fn neg(&mut self, a: SCEVId) -> SCEVId {
    let neg_one = self.const_from(-1);
    self.mul([neg_one, a].into_iter())
  }

  pub fn sub(&mut self, a: SCEVId, b: SCEVId) -> SCEVId {
    let neg_b = self.neg(b);
    self.add([a, neg_b].into_iter())
  }

  pub fn dedup(&mut self, expr: SCEVExpr) -> SCEVId {
    if let Some(&id) = self.expr_to_id.get(&expr) {
      id
    } else {
      let id = self.exprs.alloc(expr.clone());
      let scev_id = SCEVId(id);
      self.expr_to_id.insert(expr, scev_id);
      scev_id
    }
  }

  pub fn add_rec(
    &mut self,
    loop_id: LoopId,
    start: SCEVId,
    step: SCEVId,
    phi_id: Operand,
  ) -> SCEVId {
    // No step forward
    if step == self.zero {
      return start;
    }

    self.dedup(SCEVExpr::AddRec {
      loop_id,
      start,
      step,
      phi_id,
    })
  }
}
