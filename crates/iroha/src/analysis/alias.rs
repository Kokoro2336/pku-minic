//! Alias Analysis.

use yachiyo::analysis::{AliasResult, Analysis, MemLoc, RangeRelation};
use yachiyo::ir::mid::Operand;
use yachiyo::pass::PassContext;

pub struct AliasAnalysis {
  a: MemLoc,
  b: MemLoc,
}

impl Analysis for AliasAnalysis {
  /// (memory_location_a, memory_location_b)
  type Input = (MemLoc, MemLoc);
  type Output = AliasResult;

  fn name(&self) -> &str {
    "Alias Analysis"
  }

  fn new(input: Self::Input) -> Self {
    let (a, b) = input;
    Self { a, b }
  }

  fn run(&mut self) -> Self::Output {
    if self.a == self.b {
      return AliasResult::MustAlias;
    }

    let (MemLoc { base: a_base, .. }, MemLoc { base: b_base, .. }) = (&self.a, &self.b);

    if matches!(a_base, Operand::Undefined) || matches!(b_base, Operand::Undefined) {
      return AliasResult::MayAlias;
    }

    if a_base == b_base {
      return match RangeRelation::check(&self.a, &self.b) {
        RangeRelation::Disjoint => AliasResult::NoAlias,
        RangeRelation::Overlap | RangeRelation::Unknown => AliasResult::MayAlias,
      };
    }

    match (a_base, b_base) {
      (Operand::Global(_), Operand::Global(_)) => AliasResult::NoAlias,

      (Operand::Param(_), Operand::Param(_)) => AliasResult::MayAlias,

      (Operand::Param(_), Operand::Global(_)) | (Operand::Global(_), Operand::Param(_)) => {
        AliasResult::MayAlias
      }

      (Operand::Value(_), _) | (_, Operand::Value(_)) => AliasResult::MayAlias,

      _ => AliasResult::MayAlias,
    }
  }
}

pub fn is_alias(cx: &PassContext<'_>, a: Operand, b: Operand) -> AliasResult {
  let (a_loc, b_loc) = (cx.compute_mem_loc(a), cx.compute_mem_loc(b));
  AliasAnalysis::new((a_loc, b_loc)).run()
}
