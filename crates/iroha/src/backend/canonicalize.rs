//! Canonicalization.
//!
//! Fold binary constant operations and keep binary literal operands on the
//! right when the operation can be rewritten that way without changing
//! semantics.

use yachiyo::ir::back::{BAttr, BOp, BOpData, BOperand, BackIR, LOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::match_some;

#[derive(Default)]
pub struct Canonicalize<'a> {
  cx: BPassContext<'a>,
}

impl Canonicalize<'_> {
  #[inline(always)]
  pub fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  #[inline(always)]
  fn should_swap(lhs: BOperand, rhs: BOperand) -> bool {
    (lhs.is_literal() || lhs.is_zero()) && !(rhs.is_literal() || rhs.is_zero())
  }

  #[inline(always)]
  fn int_const(operand: BOperand) -> Option<i32> {
    match operand {
      BOperand::IntImm(value) => Some(value),
      BOperand::Reg(_) if operand.is_zero() => Some(0),
      _ => None,
    }
  }

  #[inline(always)]
  fn float_const(operand: BOperand) -> Option<f32> {
    match operand {
      BOperand::FloatImm(value) => Some(f32::from_bits(value)),
      _ => None,
    }
  }

  fn fold_int_bin(
    lhs: BOperand,
    rhs: BOperand,
    fold: impl FnOnce(i32, i32) -> i32,
  ) -> Option<BOperand> {
    Some(BOperand::IntImm(fold(
      Self::int_const(lhs)?,
      Self::int_const(rhs)?,
    )))
  }

  fn fold_int_div(
    lhs: BOperand,
    rhs: BOperand,
    fold: impl FnOnce(i32, i32) -> i32,
  ) -> Option<BOperand> {
    let rhs = Self::int_const(rhs)?;
    if rhs == 0 {
      return None;
    }
    Some(BOperand::IntImm(fold(Self::int_const(lhs)?, rhs)))
  }

  fn fold_int_cmp(
    lhs: BOperand,
    rhs: BOperand,
    fold: impl FnOnce(i32, i32) -> bool,
  ) -> Option<BOperand> {
    Some(BOperand::IntImm(
      fold(Self::int_const(lhs)?, Self::int_const(rhs)?) as i32,
    ))
  }

  fn fold_float_bin(
    lhs: BOperand,
    rhs: BOperand,
    fold: impl FnOnce(f32, f32) -> f32,
  ) -> Option<BOperand> {
    Some(BOperand::FloatImm(
      fold(Self::float_const(lhs)?, Self::float_const(rhs)?).to_bits(),
    ))
  }

  fn fold_float_cmp(
    lhs: BOperand,
    rhs: BOperand,
    fold: impl FnOnce(f32, f32) -> bool,
  ) -> Option<BOperand> {
    Some(BOperand::IntImm(
      fold(Self::float_const(lhs)?, Self::float_const(rhs)?) as i32,
    ))
  }

  fn fold(lop_data: &LOpData) -> Option<BOperand> {
    match_some! {
      target: lop_data.clone(),
      enu: LOpData,
      minor_arms: {
        LOpData::AddI { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l + r),
        LOpData::SubI { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l - r),
        LOpData::MulI { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l * r),
        LOpData::DivI { lhs, rhs, .. } => Self::fold_int_div(lhs, rhs, |l, r| l / r),
        LOpData::ModI { lhs, rhs, .. } => Self::fold_int_div(lhs, rhs, |l, r| l % r),
        LOpData::SNe { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l != r),
        LOpData::SEq { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l == r),
        LOpData::SGt { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l > r),
        LOpData::SLt { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l < r),
        LOpData::SGe { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l >= r),
        LOpData::SLe { lhs, rhs, .. } => Self::fold_int_cmp(lhs, rhs, |l, r| l <= r),
        LOpData::Xor { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l ^ r),
        LOpData::And { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l & r),
        LOpData::Shl { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l << r),
        LOpData::Shr { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| l >> r),
        LOpData::Sar { lhs, rhs, .. } => Self::fold_int_bin(lhs, rhs, |l, r| ((l as i64) >> r) as i32),
        LOpData::AddF { lhs, rhs, .. } => Self::fold_float_bin(lhs, rhs, |l, r| l + r),
        LOpData::SubF { lhs, rhs, .. } => Self::fold_float_bin(lhs, rhs, |l, r| l - r),
        LOpData::MulF { lhs, rhs, .. } => Self::fold_float_bin(lhs, rhs, |l, r| l * r),
        LOpData::DivF { lhs, rhs, .. } => Self::fold_float_bin(lhs, rhs, |l, r| l / r),
        LOpData::ONe { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l != r),
        LOpData::OEq { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l == r),
        LOpData::OGt { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l > r),
        LOpData::OLt { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l < r),
        LOpData::OGe { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l >= r),
        LOpData::OLe { lhs, rhs, .. } => Self::fold_float_cmp(lhs, rhs, |l, r| l <= r),
      },
      uni_ops: [Sitofp, Fptosi, Store, Load, Move, LoadIntImm, LoadFloatImm, LoadAddress, Call, Br, Jump, Ret],
      uni_arm: { None }
    }
  }

  fn canonicalize(lop_data: LOpData, attrs: &[BAttr]) -> Option<LOpData> {
    match_some! {
      target: lop_data,
      enu: LOpData,
      minor_arms: {
        LOpData::AddI { rd, lhs, rhs } if Self::should_swap(lhs, rhs) && !attrs.contains(&BAttr::PtrArith) => Some(LOpData::AddI { rd, lhs: rhs, rhs: lhs }),
        LOpData::MulI { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::MulI { rd, lhs: rhs, rhs: lhs }),
        LOpData::AddF { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::AddF { rd, lhs: rhs, rhs: lhs }),
        LOpData::MulF { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::MulF { rd, lhs: rhs, rhs: lhs }),
        LOpData::SNe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SNe { rd, lhs: rhs, rhs: lhs }),
        LOpData::SEq { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SEq { rd, lhs: rhs, rhs: lhs }),
        LOpData::OEq { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OEq { rd, lhs: rhs, rhs: lhs }),
        LOpData::ONe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::ONe { rd, lhs: rhs, rhs: lhs }),
        LOpData::Xor { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::Xor { rd, lhs: rhs, rhs: lhs }),
        LOpData::And { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::And { rd, lhs: rhs, rhs: lhs }),
        LOpData::SGt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SLt { rd, lhs: rhs, rhs: lhs }),
        LOpData::SGe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SLe { rd, lhs: rhs, rhs: lhs }),
        LOpData::SLt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SGt { rd, lhs: rhs, rhs: lhs }),
        LOpData::SLe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SGe { rd, lhs: rhs, rhs: lhs }),
        LOpData::OGt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OLt { rd, lhs: rhs, rhs: lhs }),
        LOpData::OGe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OLe { rd, lhs: rhs, rhs: lhs }),
        LOpData::OLt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OGt { rd, lhs: rhs, rhs: lhs }),
        LOpData::OLe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OGe { rd, lhs: rhs, rhs: lhs }),
      },
      uni_ops: [AddI, SubI, MulI, DivI, ModI, Xor, And, SNe, SEq, SGt, SLt, SGe, SLe, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Store, Load, Move, LoadIntImm, LoadFloatImm, LoadAddress, Call, Br, Jump, Ret],
      uni_arm: { None }
    }
  }

  fn run(&mut self) {
    let func_id = self.cx.get_current_func_id();
    let bb_ids = self.cx.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      self.cx.set_current_block(bb_id);

      let inst_ids = self.cx.get_bb(bb_id).cur.clone();
      for inst_id in inst_ids {
        let op = self.cx.get_op(inst_id);
        let (lop_data, typ, attrs) = match op.data.clone() {
          BOpData::L(lop_data) => (lop_data, op.typ.clone(), op.attrs.clone()),
          BOpData::M(mop_data) => {
            unreachable!("Unexpected machine op in Canonicalize: {:?}", mop_data)
          }
        };

        if let Some(folded) = Self::fold(&lop_data) {
          self.cx.replace_all_uses(inst_id, folded);
          self.cx.remove_op(inst_id, Some(bb_id));
          continue;
        }

        if let Some(new_lop_data) = Self::canonicalize(lop_data, &attrs) {
          self
            .cx
            .replace_op_no_rauw(inst_id, bb_id, BOp::new(typ, attrs, new_lop_data.into()));
        }
      }
    }
  }
}

impl<'a> BPass<'a> for Canonicalize<'a> {
  fn name(&self) -> &'static str {
    "Canonicalize"
  }

  fn mount(&mut self, program: &'a mut BackIR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.init(BOperand::Func(func_id));
      self.run();
    }
  }
}
