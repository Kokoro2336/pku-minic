//! Legalization , including:
//! - Inserting LoadIntImm/LoadFloatImm instructions if necessary.

use yachiyo::base::Type;
use yachiyo::config::{INT_IMM_MAX, INT_IMM_MIN};
use yachiyo::ir::back::{BAttr, BOp, BOperand, BType, BackIR, LOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::{match_some, match_src};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum LegalizeOption {
  ForceImmLoad, // Force loading the immediate into a register.
  NoLoad,       // Do not load mem.
  Default,
}

#[derive(Default)]
pub struct Legalize<'a> {
  cx: BPassContext<'a>,
}

impl Legalize<'_> {
  #[inline(always)]
  pub fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  fn legalize(&mut self, boperand: BOperand, option: LegalizeOption) -> BOperand {
    match_some! {
      target: boperand,
      enu: BOperand,
      minor_arms: {
          BOperand::IntImm(imm) => if option == LegalizeOption::ForceImmLoad {
            // If force loading required, create a new LoadIntImm instruction and return the LOpId.
            let lop_id = self.cx.create(BOp::new(
                Type::Int.into(),
                vec![],
                LOpData::LoadIntImm {
                    rd: BOperand::Undef,
                    imm,
                }
                .into(),
            ));
            *self.cx.get_rd(lop_id).unwrap()
          } else if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&imm) {
            // create a new LoadIntImm instruction and return the LOpId.
            let lop_id = self.cx.create(BOp::new(
                Type::Int.into(),
                vec![],
                LOpData::LoadIntImm {
                    rd: BOperand::Undef,
                    imm,
                }
                .into(),
            ));
            *self.cx.get_rd(lop_id).unwrap()
          } else {
            BOperand::IntImm(imm)
          },
          BOperand::FloatImm(imm) => {
            // Float can never reside in immediate field of any instrucitons,
            // So we always create a new LoadFloatImm instruction and return the LOpId.
            let lop_id = self.cx.create(BOp::new(
                Type::Float.into(),
                vec![],
                LOpData::LoadFloatImm {
                    rd: BOperand::Undef,
                    imm: f32::from_bits(imm),
                }
                .into(),
            ));
            *self.cx.get_rd(lop_id).unwrap()
          },
          // Non-load ops using a mem space must load it first.
          BOperand::Data(_)
          | BOperand::RoData(_)
          | BOperand::Bss(_) => {
            // Always force load for global memory operand.
            let la_op_id = self.cx.create(BOp::new(
              BType::U64,
              vec![],
              LOpData::LoadAddress {
                  rd: BOperand::Undef,
                  addr: boperand,
              }
              .into(),
            ));
            let la_op_rd = *self.cx.get_rd(la_op_id).unwrap();
            if option == LegalizeOption::NoLoad {
              return la_op_rd;
            }
            let typ = self.cx.get_operand_type(boperand);
            let lop_id = self.cx.create(BOp::new(
                typ,
                vec![],
                LOpData::Load {
                    rd: BOperand::Undef,
                    addr: la_op_rd,
                }
                .into(),
            ));
            *self.cx.get_rd(lop_id).unwrap()
          }

          BOperand::Slot(_) => {
            // For Load/Store
            if option == LegalizeOption::NoLoad {
              return boperand;
            }
            let typ = self.cx.get_operand_type(boperand);
            let lop_id = self.cx.create(BOp::new(
                typ,
                vec![],
                LOpData::Load {
                    rd: BOperand::Undef,
                    addr: boperand,
                }
                .into(),
            ));
            *self.cx.get_rd(lop_id).unwrap()
          },
          BOperand::Inst(_) => unreachable!("Inst should never be used as an operand in get()"),
      },
      uni_ops: [Undef, Reg, Func, BB],
      uni_arm: {
          boperand
      }
    }
  }

  pub fn run(&mut self) {
    let func_id = self.cx.current_func();
    let bb_ids = self.cx.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      self.cx.set_current_block(bb_id);
      let current_block = bb_id;

      let inst_ids = self.cx.get_bb(bb_id).cur.clone();
      for inst_id in inst_ids {
        self.cx.set_before_inst(Some(inst_id));
        let op = self.cx.get_op(inst_id);
        let (lop_data, typ, attrs) = (op.data.clone().into(), op.typ.clone(), op.attrs.clone());

        match_src! {
          target: lop_data,
          bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, AddF, SubF, MulF, DivF, Xor, And, Shl, Sar, Shr],
          bin_arm: LOpData { lhs, rhs } => {
            match (lhs.is_literal(), rhs.is_literal()) {
              // Canonicalize should fold two-literal binary operations first.
              (true, true) => {
                  unreachable!(
                    "Legalize should not see binary instructions with two literal operands: {:?}",
                    lop_data
                  );
              },
              (true, false) => {
                // Do not swap here; canonicalization owns operand reordering.
                let new_lop_data = match lop_data {
                  LOpData::SubI { rd, lhs: imm, rhs } => if attrs.contains(&BAttr::PtrArith) {
                    LOpData::SubI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::NoLoad) }
                  } else {
                    LOpData::SubI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) }
                  }
                  LOpData::AddI { rd, lhs: imm, rhs } => if attrs.contains(&BAttr::PtrArith) {
                    // CAUTION: DO NOT change the position of mem entities!
                    LOpData::AddI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::NoLoad) }
                  } else {
                    LOpData::AddI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) }
                  }

                  // Literal lhs operands must be materialized when they cannot be canonicalized.
                  LOpData::SubF { rd, lhs: imm, rhs } =>
                    LOpData::SubF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::DivF { rd, lhs: imm, rhs } =>
                    LOpData::DivF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::DivI { rd, lhs: imm, rhs } =>
                    LOpData::DivI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::ModI { rd, lhs: imm, rhs } =>
                    LOpData::ModI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Shl { rd, lhs: imm, rhs } =>
                    LOpData::Shl { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Shr { rd, lhs: imm, rhs } =>
                    LOpData::Shr { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Sar { rd, lhs: imm, rhs } =>
                    LOpData::Sar { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  LOpData::MulI { rd, lhs: imm, rhs } =>
                    LOpData::MulI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::MulF { rd, lhs: imm, rhs } =>
                    LOpData::MulF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  LOpData::AddF { rd, lhs: imm, rhs } =>
                    LOpData::AddF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SNe { rd, lhs: imm, rhs } =>
                    LOpData::SNe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SEq { rd, lhs: imm, rhs } =>
                    LOpData::SEq { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SGt { rd, lhs: imm, rhs } =>
                    LOpData::SGt { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SGe { rd, lhs: imm, rhs } =>
                    LOpData::SGe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SLt { rd, lhs: imm, rhs } =>
                    LOpData::SLt { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SLe { rd, lhs: imm, rhs } =>
                    LOpData::SLe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OEq { rd, lhs: imm, rhs } =>
                    LOpData::OEq { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::ONe { rd, lhs: imm, rhs } =>
                    LOpData::ONe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OGt { rd, lhs: imm, rhs } =>
                    LOpData::OGt { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OGe { rd, lhs: imm, rhs } =>
                    LOpData::OGe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OLt { rd, lhs: imm, rhs } =>
                    LOpData::OLt { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OLe { rd, lhs: imm, rhs } =>
                    LOpData::OLe { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Xor { rd, lhs: imm, rhs } =>
                    LOpData::Xor { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::And { rd, lhs: imm, rhs } =>
                    LOpData::And { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  _ => unreachable!("Unexpected op: {:?}", lop_data),
                };
                self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                  typ,
                  attrs,
                  new_lop_data.into(),
                ));
              },
              (false, true) => {
                // No swap. Just legalize.
                let new_lop_data = match lop_data {
                  // Pointer arithmetic should not load mem entities.
                  LOpData::AddI { rd, lhs, rhs: imm } => if attrs.contains(&BAttr::PtrArith) {
                    LOpData::AddI { rd, lhs: self.legalize(lhs, LegalizeOption::NoLoad), rhs: self.legalize(imm, LegalizeOption::Default) }
                  } else {
                    LOpData::AddI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) }
                  }
                  LOpData::SubI { rd, lhs, rhs: imm } => if attrs.contains(&BAttr::PtrArith) {
                    LOpData::SubI { rd, lhs: self.legalize(lhs, LegalizeOption::NoLoad), rhs: self.legalize(imm, LegalizeOption::Default) }
                  } else {
                    LOpData::SubI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) }
                  }

                  LOpData::SGt { rd, lhs, rhs: imm } =>
                    LOpData::SGt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SGe { rd, lhs, rhs: imm } =>
                    LOpData::SGe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SLt { rd, lhs, rhs: imm } =>
                    LOpData::SLt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SLe { rd, lhs, rhs: imm } =>
                    LOpData::SLe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::OGt { rd, lhs, rhs: imm } =>
                    LOpData::OGt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::OGe { rd, lhs, rhs: imm } =>
                    LOpData::OGe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::OLt { rd, lhs, rhs: imm } =>
                    LOpData::OLt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::OLe { rd, lhs, rhs: imm } =>
                    LOpData::OLe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::AddF { rd, lhs, rhs: imm } =>
                    LOpData::AddF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SNe { rd, lhs, rhs: imm } =>
                    LOpData::SNe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SEq { rd, lhs, rhs: imm } =>
                    LOpData::SEq { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::OEq { rd, lhs, rhs: imm } =>
                    LOpData::OEq { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::ONe { rd, lhs, rhs: imm } =>
                    LOpData::ONe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::Xor { rd, lhs, rhs: imm } =>
                    LOpData::Xor { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::And { rd, lhs, rhs: imm } =>
                    LOpData::And { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::SubF { rd, lhs, rhs: imm } =>
                    LOpData::SubF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::Shl { rd, lhs, rhs: imm } =>
                    LOpData::Shl { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::Shr { rd, lhs, rhs: imm } =>
                    LOpData::Shr { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },
                  LOpData::Sar { rd, lhs, rhs: imm } =>
                    LOpData::Sar { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::Default) },

                  // For Mul/Div/Mod, we still have to load the literal even if it's on the right side.
                  LOpData::MulI { rd, lhs, rhs: imm } =>
                    LOpData::MulI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                  LOpData::MulF { rd, lhs, rhs: imm } =>
                    LOpData::MulF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                  LOpData::DivF { rd, lhs, rhs: imm } =>
                    LOpData::DivF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                  LOpData::DivI { rd, lhs, rhs: imm } =>
                    LOpData::DivI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                  LOpData::ModI { rd, lhs, rhs: imm } =>
                    LOpData::ModI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },

                  _ => unreachable!("Unexpected op with literal on the right: {:?}", lop_data),
                };
                self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                  typ,
                  attrs,
                  new_lop_data.into(),
                ));
              }
              (false, false) => {
                // No swap. Just legalize both operands.
                let new_lop_data = match lop_data {
                  LOpData::AddI { rd, .. } => if attrs.contains(&BAttr::PtrArith) {
                    LOpData::AddI { rd, lhs: self.legalize(lhs, LegalizeOption::NoLoad), rhs: self.legalize(rhs, LegalizeOption::NoLoad) }
                  } else {
                    LOpData::AddI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) }
                  },
                  LOpData::SubI { rd, .. } => if attrs.contains(&BAttr::PtrArith) {
                    LOpData::SubI { rd, lhs: self.legalize(lhs, LegalizeOption::NoLoad), rhs: self.legalize(rhs, LegalizeOption::NoLoad) }
                  } else {
                    LOpData::SubI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) }
                  },

                  LOpData::SGt { rd, .. } => LOpData::SGt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SGe { rd, .. } => LOpData::SGe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SLt { rd, .. } => LOpData::SLt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SLe { rd, .. } => LOpData::SLe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OGt { rd, .. } => LOpData::OGt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OGe { rd, .. } => LOpData::OGe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OLt { rd, .. } => LOpData::OLt { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OLe { rd, .. } => LOpData::OLe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::AddF { rd, .. } => LOpData::AddF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SubF { rd, .. } => LOpData::SubF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  // No literal, no ForceImmLoad.
                  LOpData::MulI { rd, .. } => LOpData::MulI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::MulF { rd, .. } => LOpData::MulF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::DivI { rd, .. } => LOpData::DivI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::DivF { rd, .. } => LOpData::DivF { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::ModI { rd, .. } => LOpData::ModI { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  LOpData::SNe { rd, .. } => LOpData::SNe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::SEq { rd, .. } => LOpData::SEq { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::OEq { rd, .. } => LOpData::OEq { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::ONe { rd, .. } => LOpData::ONe { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Xor { rd, .. } => LOpData::Xor { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::And { rd, .. } => LOpData::And { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Shl { rd, .. } => LOpData::Shl { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Shr { rd, .. } => LOpData::Shr { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },
                  LOpData::Sar { rd, .. } => LOpData::Sar { rd, lhs: self.legalize(lhs, LegalizeOption::Default), rhs: self.legalize(rhs, LegalizeOption::Default) },

                  _ => unreachable!("Unexpected op with no literal operand: {:?}", lop_data),
                };
                self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                  typ,
                  attrs,
                  new_lop_data.into(),
                ));
              }
            }
          },
          un_ops: [Sitofp, Fptosi],
          un_arm: LOpData { value } => {
            // Legalize the operand if it's a literal, but don't fold it even if it's a constant, because folding might cause overflow which is undefined behavior in Rust.
            let new_lop_data = match lop_data {
              LOpData::Sitofp { rd, value } =>
                LOpData::Sitofp { rd, value: self.legalize(value, LegalizeOption::ForceImmLoad) },
              LOpData::Fptosi { rd, value } =>
                LOpData::Fptosi { rd, value: self.legalize(value, LegalizeOption::ForceImmLoad) },
              _ => unreachable!("Unexpected unary op: {:?}", lop_data),
            };
            self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
              typ,
              attrs,
              new_lop_data.into(),
            ));
          },
          fallback: {
            LOpData::Store { addr, value, val_typ } => {
              // Mem operand should not be
              let new_lop_data = LOpData::Store { addr: self.legalize(addr, LegalizeOption::NoLoad), value: self.legalize(value, LegalizeOption::ForceImmLoad), val_typ };
              self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                typ,
                attrs,
                new_lop_data.into(),
              ));
            }
            LOpData::Load { addr, rd } => {
              // Mem operand should not be legalized to Load again, otherwise it will cause infinite loop.
              let new_lop_data = LOpData::Load { addr: self.legalize(addr, LegalizeOption::NoLoad), rd };
              self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                typ,
                attrs,
                new_lop_data.into(),
              ));
            },
            LOpData::Move { rd, src } => {
              // Move should not have literal operand, but we still legalize it just in case.
              let new_lop_data = LOpData::Move { rd, src: self.legalize(src, LegalizeOption::ForceImmLoad) };
              self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                typ,
                attrs,
                new_lop_data.into(),
              ));
            }
            LOpData::Br { cond, then_bb, else_bb } => {
              let new_lop_data = LOpData::Br {
                cond: self.legalize(cond, LegalizeOption::ForceImmLoad),
                then_bb,
                else_bb,
              };
              self.cx.replace_op_no_rauw(inst_id, current_block, BOp::new(
                typ,
                attrs,
                new_lop_data.into(),
              ));
            },

            LOpData::LoadAddress {..}
            | LOpData::LoadIntImm {..}
            | LOpData::LoadFloatImm {..} => unreachable!(),

            LOpData::Call {..}
            | LOpData::Jump {..}
            | LOpData::Ret => {/*do nothing*/},
          }
        }
      }
    }
  }
}

impl<'a> BPass<'a> for Legalize<'a> {
  fn name(&self) -> &'static str {
    "Legalize"
  }
  fn mount(&mut self, program: &'a mut BackIR) {
    self.cx.mount(program);
  }
  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = BOperand::Func(func_id);
      self.init(func_id);
      self.run();
    }
  }
}
