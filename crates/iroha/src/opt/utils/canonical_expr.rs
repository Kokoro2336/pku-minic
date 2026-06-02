//! Canonicalized Expression View.

use yachiyo::ir::mid::{OpData, Operand, PhiIncoming};
use yachiyo::pass::PassContext;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CanonicalExpr {
  // We don't consider defining separate operations for int and float.
  Add(Operand, Operand),
  Mul(Operand, Operand),
  Sub(Operand, Operand),
  Div(Operand, Operand),
  Mod(Operand, Operand),
  Xor(Operand, Operand),
  Shl(Operand, Operand),
  Shr(Operand, Operand),
  Sar(Operand, Operand),
  Eq(Operand, Operand),
  Ne(Operand, Operand),
  Lt(Operand, Operand),
  Le(Operand, Operand),
  Sitofp(Operand),
  Fptosi(Operand),
  Uitofp(Operand),
  Zext(Operand),
  /// Phi's operands are sorted by the block id.
  Phi(Vec<PhiIncoming>),
  #[allow(clippy::upper_case_acronyms)]
  GEP(Operand, Vec<Operand>),
  Call(Operand, Vec<Operand>),

  Load(Operand),
  Store(Operand, Operand),

  // TODO: When we can determine whether a function has side effects, we can add Call here.
  /// For other operations that we don't consider, we represent then as None.
  None,
}

impl From<&OpData> for CanonicalExpr {
  fn from(op_data: &OpData) -> Self {
    let swap = |lhs: Operand, rhs: Operand| {
      if lhs < rhs {
        (lhs, rhs)
      } else {
        (rhs, lhs)
      }
    };
    match op_data {
      // Canonicalize commutative operations by sorting their operands.
      OpData::AddI { lhs, rhs } | OpData::AddF { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Add(lhs, rhs)
      }
      OpData::MulI { lhs, rhs } | OpData::MulF { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Mul(lhs, rhs)
      }
      OpData::Xor { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Xor(lhs, rhs)
      }
      OpData::SEq { lhs, rhs } | OpData::OEq { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Eq(lhs, rhs)
      }
      OpData::SNe { lhs, rhs } | OpData::ONe { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Ne(lhs, rhs)
      }

      // These operantions are not commutative, so we keep their operands in order.
      OpData::SubI { lhs, rhs } | OpData::SubF { lhs, rhs } => CanonicalExpr::Sub(*lhs, *rhs),
      OpData::DivI { lhs, rhs } | OpData::DivF { lhs, rhs } => CanonicalExpr::Div(*lhs, *rhs),
      OpData::SLt { lhs, rhs } | OpData::OLt { lhs, rhs } => CanonicalExpr::Lt(*lhs, *rhs),
      OpData::SLe { lhs, rhs } | OpData::OLe { lhs, rhs } => CanonicalExpr::Le(*lhs, *rhs),
      OpData::ModI { lhs, rhs } => CanonicalExpr::Mod(*lhs, *rhs),
      OpData::Shl { lhs, rhs } => CanonicalExpr::Shl(*lhs, *rhs),
      OpData::Shr { lhs, rhs } => CanonicalExpr::Shr(*lhs, *rhs),
      OpData::Sar { lhs, rhs } => CanonicalExpr::Sar(*lhs, *rhs),

      // We can canonicalize `>` and `>=` by swapping their operands and changing them to `<` and `<=`.
      OpData::SGt { lhs, rhs } | OpData::OGt { lhs, rhs } => CanonicalExpr::Lt(*rhs, *lhs),
      OpData::SGe { lhs, rhs } | OpData::OGe { lhs, rhs } => CanonicalExpr::Le(*rhs, *lhs),

      // These operations are unary, so we keep their operand as is.
      OpData::Sitofp { value } => CanonicalExpr::Sitofp(*value),
      OpData::Fptosi { value } => CanonicalExpr::Fptosi(*value),
      OpData::Uitofp { value } => CanonicalExpr::Uitofp(*value),
      OpData::Zext { value } => CanonicalExpr::Zext(*value),

      OpData::Phi { incomings } => {
        let mut sorted_incomings = incomings.clone();
        sorted_incomings.sort_by_key(|incoming| match incoming {
          PhiIncoming::Data { bb, .. } => *bb,
          PhiIncoming::None => unreachable!(),
        });
        CanonicalExpr::Phi(sorted_incomings)
      }

      OpData::GEP { base, indices } => CanonicalExpr::GEP(*base, indices.clone()),
      OpData::Call { func, args } => CanonicalExpr::Call(*func, args.clone()),

      OpData::Alloca(_)
      | OpData::Declare { .. }
      | OpData::Load { .. }
      | OpData::Store { .. }
      | OpData::Ret { .. }
      | OpData::Br { .. }
      | OpData::Jump { .. }
      | OpData::Splat { .. }
      | OpData::VBuild4 { .. }
      | OpData::VReduceAddI { .. }
      | OpData::VReduceAddF { .. }
      | OpData::GlobalAlloca(_) => CanonicalExpr::None,
    }
  }
}

impl CanonicalExpr {
  // For deep pattern matching.
  pub fn deep_canonicalize(&mut self, ctx: &mut PassContext) {
    match self {
      CanonicalExpr::Add(lhs @ Operand::Value(_), rhs @ Operand::Value(_))
      | CanonicalExpr::Mul(lhs @ Operand::Value(_), rhs @ Operand::Value(_))
      | CanonicalExpr::Xor(lhs @ Operand::Value(_), rhs @ Operand::Value(_))
      | CanonicalExpr::Eq(lhs @ Operand::Value(_), rhs @ Operand::Value(_))
      | CanonicalExpr::Ne(lhs @ Operand::Value(_), rhs @ Operand::Value(_)) => {
        let lhs_data = ctx.get_op_data(*lhs);
        let rhs_data = ctx.get_op_data(*rhs);
        if lhs_data > rhs_data {
          std::mem::swap(lhs, rhs);
        }
      }
      _ => { /*Don't swap*/ }
    }
  }
}
