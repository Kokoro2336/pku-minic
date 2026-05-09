//! Strength Reduction, replace expensive operations with cheaper ones.

use yachiyo::ir::back::{BOp, BOperand, BackIR, LOpData, Reg, XReg};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::r#match::match_some;

#[derive(Default)]
pub struct StrengthReduct<'a> {
  cx: BPassContext<'a>,
}

impl StrengthReduct<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  /// 2^x. For exmaple, 8, replaced with 1 << 3.
  #[inline(always)]
  fn pow2_shift(x: i32) -> Option<u32> {
    if x > 0 && (x as u32).is_power_of_two() {
      Some(x.trailing_zeros())
    } else {
      None
    }
  }

  /// 2^x + 1. For exmaple, 9, replaced with 1 << 3 + 1.
  #[inline(always)]
  fn pow2_shift_plus_one(x: i32) -> Option<u32> {
    if x > 1 && ((x - 1) as u32).is_power_of_two() {
      Some((x - 1).trailing_zeros())
    } else {
      None
    }
  }

  /// 2^x - 1. For exmaple, 7, replaced with 1 << 3 - 1.
  #[inline(always)]
  fn pow2_shift_minus_one(x: i32) -> Option<u32> {
    if x > 1 && ((x + 1) as u32).is_power_of_two() {
      Some((x + 1).trailing_zeros())
    } else {
      None
    }
  }

  /// -(2^x). For exmaple, -8, replaced with -(1 << 3).
  #[inline(always)]
  fn neg_pow2_shift(x: i32) -> Option<u32> {
    if x < 0 && ((-x) as u32).is_power_of_two() {
      Some(((-x) as u32).trailing_zeros())
    } else {
      None
    }
  }

  /// -(2^x + 1). For exmaple, -9, replaced with -(1 << 3 + 1).
  #[inline(always)]
  fn neg_pow2_shift_plus_one(x: i32) -> Option<u32> {
    if x < -1 && ((-(x + 1)) as u32).is_power_of_two() {
      Some(((-(x + 1)) as u32).trailing_zeros())
    } else {
      None
    }
  }

  /// -(2^x - 1). For exmaple, -7, replaced with 1 - 1 << 3.
  #[inline(always)]
  fn neg_pow2_shift_minus_one(x: i32) -> Option<u32> {
    if x < -1 && ((-(x - 1)) as u32).is_power_of_two() {
      Some(((-(x - 1)) as u32).trailing_zeros())
    } else {
      None
    }
  }

  pub fn run(&mut self) {
    let func_id = self.cx.current_func();
    let bb_ids = self.cx.get_func(func_id).cfg.dpo();
    for &bb_id in bb_ids.iter().rev() {
      let bb_id = BOperand::BB(bb_id);
      self.cx.set_current_block(bb_id);
      let inst_ids = self.cx.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        self.cx.set_before_inst(Some(inst_id));
        let lop = &self.cx.get_func(func_id).dfg[inst_id];
        let lop_data: LOpData = lop.data.clone().into();
        let (typ, attrs) = (lop.typ.clone(), lop.attrs.clone());

        match_some! {
          target: lop_data,
          enu: LOpData,
          minor_arms: {
            LOpData::MulI { rd, lhs, rhs } => if let BOperand::IntImm(imm) = rhs {
              let bb_id = self.cx.op_bb(inst_id);
              if imm == 0 {
                self.cx.replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
              } else if imm == 1 {
                self.cx.replace_all_uses(inst_id, lhs);
              } else if imm == -1 {
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs: BOperand::Reg(Reg::X(XReg::Zero)), rhs: lhs }.into()
                ));
              } else if let Some(shift) = Self::pow2_shift(imm) {
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::Shl { rd, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
              } else if let Some(shift) = Self::pow2_shift_plus_one(imm) {
                let shift_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Shl { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
                let shift_vreg_id = *self.cx.get_rd(shift_op_id).unwrap();
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::AddI { rd, lhs: shift_vreg_id, rhs: lhs }.into()
                ));
              } else if let Some(shift) = Self::pow2_shift_minus_one(imm) {
                let shift_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Shl { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
                let shift_vreg_id = *self.cx.get_rd(shift_op_id).unwrap();
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs: shift_vreg_id, rhs: lhs }.into()
                ));
              } else if let Some(shift) = Self::neg_pow2_shift(imm) {
                let shift_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Shl { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
                let shift_vreg_id = *self.cx.get_rd(shift_op_id).unwrap();
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs: BOperand::Reg(Reg::X(XReg::Zero)), rhs: shift_vreg_id }.into()
                ));
              } else if let Some(shift) = Self::neg_pow2_shift_plus_one(imm) {
                let shift_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Shl { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
                let shift_vreg_id = *self.cx.get_rd(shift_op_id).unwrap();
                let add_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::AddI { rd: BOperand::Undef, lhs: shift_vreg_id, rhs: lhs }.into()
                ));
                let add_vreg_id = *self.cx.get_rd(add_op_id).unwrap();
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs: BOperand::Reg(Reg::X(XReg::Zero)), rhs: add_vreg_id }.into()
                ));
              } else if let Some(shift) = Self::neg_pow2_shift_minus_one(imm) {
                let shift_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Shl { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
                let shift_vreg_id = *self.cx.get_rd(shift_op_id).unwrap();
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs, rhs: shift_vreg_id }.into()
                ));
              }
            },
            LOpData::DivI { rd, lhs, rhs } => if let BOperand::IntImm(imm) = rhs {
              if imm == 1 {
                self.cx.replace_all_uses(inst_id, lhs);
              } else if imm == -1 {
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::SubI { rd, lhs: BOperand::Reg(Reg::X(XReg::Zero)), rhs: lhs }.into()
                ));
              } else if let Some(shift) = Self::pow2_shift(imm) {
                // Create bias
                let shift_bias_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::Sar { rd: BOperand::Undef, lhs, rhs: BOperand::IntImm(31) }.into()
                ));
                let shift_bias_vreg_id = *self.cx.get_rd(shift_bias_op_id).unwrap();
                // And with mask
                let bias_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::And { rd: BOperand::Undef, lhs: shift_bias_vreg_id, rhs: BOperand::IntImm((1 << shift) - 1) }.into()
                ));
                let bias_vreg_id = *self.cx.get_rd(bias_op_id).unwrap();
                // Add bias
                let add_op_id = self.cx.create(BOp::new(
                  typ.clone(),
                  attrs.clone(),
                  LOpData::AddI { rd: BOperand::Undef, lhs, rhs: bias_vreg_id }.into()
                ));
                let add_vreg_id = *self.cx.get_rd(add_op_id).unwrap();
                // Shift
                self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
                  typ,
                  attrs,
                  LOpData::Sar { rd, lhs: add_vreg_id, rhs: BOperand::IntImm(shift as i32) }.into()
                ));
              }
              // TODO: Convert the division to multiplication.
            },
            LOpData::ModI { rhs, .. } => if let BOperand::IntImm(imm) = rhs {
              if imm == 1 || imm == -1 {
                self.cx.replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
              }
              // TODO: Can we prove that lhs is non-negative?
              // else if let Some(shift) = Self::pow2_shift(imm) {
              //   self.cx.replace_op_no_rauw(inst_id, bb_id, BOp::new(
              //     typ,
              //     attrs,
              //     LOpData::And { rd, lhs, rhs: BOperand::IntImm((1 << shift) - 1) }.into()
              //   ));
              // }
            },
            LOpData::AddI { lhs, rhs, .. }
            | LOpData::SubI { lhs, rhs, .. } => if let BOperand::IntImm(imm) = rhs {
              if imm == 0 {
                self.cx.replace_all_uses(inst_id, lhs);
              }
            }
            LOpData::Shl { lhs, rhs, .. }
            | LOpData::Shr { lhs, rhs, .. }
            | LOpData::Sar { lhs, rhs, .. } => if let BOperand::IntImm(imm) = rhs {
              if imm == 0 {
                self.cx.replace_all_uses(inst_id, lhs);
              }
            }
          },
          uni_ops: [
            AddF, SubF, MulF, DivF,
            SNe, SEq, SGt, SLt, SGe, SLe,
            Xor, And, Shl, Shr, Sar,
            ONe, OEq, OGt, OLt, OGe, OLe,
            Sitofp, Fptosi, Store, Load, Move,
            LoadIntImm, LoadFloatImm, LoadAddress,
            Call, Br, Jump, Ret
          ],
          uni_arm: {}
        }
      }
    }
  }
}

impl<'a> BPass<'a> for StrengthReduct<'a> {
  fn name(&self) -> &str {
    "StrengthReduct"
  }

  fn mount(&mut self, ir: &'a mut BackIR) {
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.init(BOperand::Func(func_id));
      self.run();
    }
  }
}
