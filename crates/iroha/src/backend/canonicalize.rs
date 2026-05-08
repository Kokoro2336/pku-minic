//! Canonicalization.
//!
//! Fold binary constant operations and keep binary literal operands on the
//! right when the operation can be rewritten that way without changing
//! semantics.

use yachiyo::ir::back::{BAttr, BOp, BOpData, BOperand, BackIR, LOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::r#match::match_some;

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
    lhs.is_literal() && !rhs.is_literal()
  }

  fn fold(lop_data: &LOpData) -> Option<BOperand> {
    match_some! {
      target: lop_data.clone(),
      enu: LOpData,
      minor_arms: {
        LOpData::AddI { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l + r)),
        LOpData::SubI { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l - r)),
        LOpData::MulI { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l * r)),
        LOpData::DivI { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l / r)),
        LOpData::ModI { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l % r)),
        LOpData::SNe { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l != r) as i32)),
        LOpData::SEq { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l == r) as i32)),
        LOpData::SGt { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l > r) as i32)),
        LOpData::SLt { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l < r) as i32)),
        LOpData::SGe { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l >= r) as i32)),
        LOpData::SLe { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm((l <= r) as i32)),
        LOpData::Xor { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l ^ r)),
        LOpData::Shl { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l << r)),
        LOpData::Shr { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(l >> r)),
        LOpData::Sar { lhs: BOperand::IntImm(l), rhs: BOperand::IntImm(r), .. } => Some(BOperand::IntImm(((l as i64) >> r) as i32)),
        LOpData::AddF { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::FloatImm((f32::from_bits(l) + f32::from_bits(r)).to_bits())),
        LOpData::SubF { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::FloatImm((f32::from_bits(l) - f32::from_bits(r)).to_bits())),
        LOpData::MulF { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::FloatImm((f32::from_bits(l) * f32::from_bits(r)).to_bits())),
        LOpData::DivF { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::FloatImm((f32::from_bits(l) / f32::from_bits(r)).to_bits())),
        LOpData::ONe { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) != f32::from_bits(r)) as i32)),
        LOpData::OEq { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) == f32::from_bits(r)) as i32)),
        LOpData::OGt { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) > f32::from_bits(r)) as i32)),
        LOpData::OLt { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) < f32::from_bits(r)) as i32)),
        LOpData::OGe { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) >= f32::from_bits(r)) as i32)),
        LOpData::OLe { lhs: BOperand::FloatImm(l), rhs: BOperand::FloatImm(r), .. } => Some(BOperand::IntImm((f32::from_bits(l) <= f32::from_bits(r)) as i32)),
        LOpData::Ret => None,
      },
      uni_ops: [AddI, SubI, MulI, DivI, ModI, Xor, SNe, SEq, SGt, SLt, SGe, SLe, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Store, Load, Move, LoadIntImm, LoadFloatImm, LoadAddress, Call, Br, Jump],
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
        LOpData::SGt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SLt { rd, lhs: rhs, rhs: lhs }),
        LOpData::SGe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SLe { rd, lhs: rhs, rhs: lhs }),
        LOpData::SLt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SGt { rd, lhs: rhs, rhs: lhs }),
        LOpData::SLe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::SGe { rd, lhs: rhs, rhs: lhs }),
        LOpData::OGt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OLt { rd, lhs: rhs, rhs: lhs }),
        LOpData::OGe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OLe { rd, lhs: rhs, rhs: lhs }),
        LOpData::OLt { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OGt { rd, lhs: rhs, rhs: lhs }),
        LOpData::OLe { rd, lhs, rhs } if Self::should_swap(lhs, rhs) => Some(LOpData::OGe { rd, lhs: rhs, rhs: lhs }),
      },
      uni_ops: [AddI, SubI, MulI, DivI, ModI, Xor, SNe, SEq, SGt, SLt, SGe, SLe, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Store, Load, Move, LoadIntImm, LoadFloatImm, LoadAddress, Call, Br, Jump, Ret],
      uni_arm: { None }
    }
  }

  fn run(&mut self) {
    let func_id = self.cx.current_func();
    let bb_ids = self.cx.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      self.cx.set_current_block(bb_id);

      let inst_ids = self.cx.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        let op = &self.cx.get_func(func_id).dfg[inst_id];
        let (lop_data, typ, attrs) = match op.data.clone() {
          BOpData::L(lop_data) => (lop_data, op.typ.clone(), op.attrs.clone()),
          BOpData::M(mop_data) => unreachable!("Unexpected machine op in Canonicalize: {:?}", mop_data),
        };

        if let Some(folded) = Self::fold(&lop_data) {
          self.cx.replace_all_uses(inst_id, folded);
          self.cx.remove_op(inst_id, Some(bb_id));
          continue;
        }

        if let Some(new_lop_data) = Self::canonicalize(lop_data, &attrs) {
          self
            .cx
            .replace_op_rauw(inst_id, bb_id, BOp::new(typ, attrs, new_lop_data.into()));
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
