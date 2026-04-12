//! Canonicalization (and Legalization) .
//! - Constant folding for binary operations with literal operands.
//! - Reordering of operands to ensure literals are on the right side; adjusting the operator
//! - Inserting LoadIntImm/LoadFloatImm instructions is necessary.

use yachiyo::base::Type;
use yachiyo::config::{INT_IMM_MAX, INT_IMM_MIN};
use yachiyo::ir::back::{BBuilder, BFunction, BOp, BOperand, BType, BackIR, LOpData, Reg, Slot};
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::{match_some, match_src};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum LegalizeOption {
  ForceImmLoad, // Force loading the operand into a register.
  NoLoad,       // Do not load mem.
  Default,
}

#[derive(Default)]
pub struct Canonicalize<'a> {
  ir: Option<&'a mut BackIR>,
  builder: BBuilder,
}

impl Canonicalize<'_> {
  #[inline(always)]
  pub fn init(&mut self, func_id: BOperand) {
    self.builder.set_current_func(func_id);
  }

  #[inline(always)]
  pub fn get_func(&self, func_id: BOperand) -> &BFunction {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  pub fn get_func_mut(&mut self, func_id: BOperand) -> &mut BFunction {
    &mut self.ir.as_mut().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_rd(&self, bop_id: BOperand) -> Option<&BOperand> {
    let func_id = self.builder.current_function.expect("No current function");
    self.ir.as_ref().unwrap().get_rd(Some(func_id), bop_id)
  }

  #[inline(always)]
  fn get_operand_type(&self, operand: BOperand) -> BType {
    let func_id = self.builder.current_function.unwrap();

    match operand {
      BOperand::Inst(id) => {
        let op = &self.get_func(func_id).dfg[id];
        op.typ.clone()
      }
      BOperand::Reg(reg) => match reg {
        Reg::X(_) => BType::I32,
        Reg::F(_) => BType::F32,
        Reg::Virt(_) => self.get_func(func_id).vregs[operand].typ.clone(),
      },
      BOperand::IntImm(_) => BType::I32,
      BOperand::FloatImm(_) => BType::F32,
      BOperand::Undef => BType::Void,

      BOperand::Slot(_) => match &self.get_func(func_id).frame_info[operand] {
        Slot::CalleeSaved { typ, .. }
        | Slot::Local { typ, .. }
        | Slot::Param { typ, .. }
        | Slot::Arg { typ, .. } => typ.clone(),
      },
      BOperand::Data(_) => self.ir.as_ref().unwrap().data_info[operand].typ.clone(),
      BOperand::RoData(_) => self.ir.as_ref().unwrap().rodata_info[operand].typ.clone(),
      BOperand::Bss(_) => self.ir.as_ref().unwrap().bss_info[operand].typ.clone(),

      BOperand::Func(_) | BOperand::BB(_) => unreachable!(),
    }
  }

  #[inline(always)]
  fn replace_op_rauw(&mut self, old_id: BOperand, new_op: BOp) -> BOperand {
    let func_id = self
      .builder
      .current_function
      .expect("ISel: not in a function");
    let current_block = self.builder.current_block.expect("Not current block found");
    self.ir.as_mut().unwrap().replace_op_rauw(
      &mut self.builder,
      Some(func_id),
      old_id,
      current_block,
      new_op,
    )
  }

  #[inline(always)]
  fn replace_op_no_rauw(&mut self, old_id: BOperand, new_op: BOp) -> BOperand {
    let func_id = self
      .builder
      .current_function
      .expect("ISel: not in a function");
    let current_block = self.builder.current_block.expect("Not current block found");
    self.ir.as_mut().unwrap().replace_op_no_rauw(
      &mut self.builder,
      Some(func_id),
      old_id,
      current_block,
      new_op,
    )
  }

  fn legalize(&mut self, boperand: BOperand, option: LegalizeOption) -> BOperand {
    match_some! {
      target: boperand,
      enu: BOperand,
      minor_arms: {
          BOperand::IntImm(imm) => if option == LegalizeOption::ForceImmLoad {
            // If force loading required, create a new LoadIntImm instruction and return the LOpId.
            let lop_id = self.create(BOp::new(
                Type::Int.into(),
                vec![],
                LOpData::LoadIntImm {
                    rd: BOperand::Undef,
                    imm,
                }
                .into(),
            ));
            *self.get_rd(lop_id).unwrap()
          } else {
            if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&imm) {
                // create a new LoadIntImm instruction and return the LOpId.
                let lop_id = self.create(BOp::new(
                    Type::Int.into(),
                    vec![],
                    LOpData::LoadIntImm {
                        rd: BOperand::Undef,
                        imm,
                    }
                    .into(),
                ));
                *self.get_rd(lop_id).unwrap()
            } else {
                BOperand::IntImm(imm)
            }
          },
          BOperand::FloatImm(imm) => {
            // Float can never reside in immediate field of any instrucitons,
            // So we always create a new LoadFloatImm instruction and return the LOpId.
            let lop_id = self.create(BOp::new(
                Type::Float.into(),
                vec![],
                LOpData::LoadFloatImm {
                    rd: BOperand::Undef,
                    imm: f32::from_bits(imm),
                }
                .into(),
            ));
            *self.get_rd(lop_id).unwrap()
          },
          // Non-load ops using a mem space must load it first.
          BOperand::Data(_)
          | BOperand::RoData(_)
          | BOperand::Bss(_) => {
            // Always force load for global memory operand.
            let la_op_id = self.create(BOp::new(
              BType::U64,
              vec![],
              LOpData::LoadAddress {
                  rd: BOperand::Undef,
                  addr: boperand,
              }
              .into(),
            ));
            let la_op_rd = *self.get_rd(la_op_id).unwrap();
            if option == LegalizeOption::NoLoad {
              return la_op_rd;
            }
            let typ = self.get_operand_type(boperand);
            let lop_id = self.create(BOp::new(
                typ,
                vec![],
                LOpData::Load {
                    rd: BOperand::Undef,
                    addr: la_op_rd,
                }
                .into(),
            ));
            *self.get_rd(lop_id).unwrap()
          }

          BOperand::Slot(_) => {
            // For Load/Store
            if option == LegalizeOption::NoLoad {
              return boperand;
            }
            let typ = self.get_operand_type(boperand);
            let lop_id = self.create(BOp::new(
                typ,
                vec![],
                LOpData::Load {
                    rd: BOperand::Undef,
                    addr: boperand,
                }
                .into(),
            ));
            *self.get_rd(lop_id).unwrap()
          },
          BOperand::Inst(_) => unreachable!("Inst should never be used as an operand in get()"),
      },
      uni_ops: [Undef, Reg, Func, BB],
      uni_arm: {
          boperand
      }
    }
  }

  #[inline(always)]
  fn create(&mut self, bop: BOp) -> BOperand {
    self.builder.create(
      self.ir.as_mut().unwrap(),
      self.builder.current_function,
      bop,
    )
  }

  fn fold(lop_data: LOpData) -> BOperand {
    match_src! {
        target: &lop_data,
        bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
        bin_arm: LOpData { lhs, rhs } => {
            if let (BOperand::IntImm(l), BOperand::IntImm(r)) = (*lhs, *rhs) {
                match lop_data {
                    LOpData::AddI { .. } => BOperand::IntImm(l + r),
                    LOpData::SubI { .. } => BOperand::IntImm(l - r),
                    LOpData::MulI { .. } => BOperand::IntImm(l * r),
                    LOpData::DivI { .. } => BOperand::IntImm(l / r),
                    LOpData::ModI { .. } => BOperand::IntImm(l % r),
                    LOpData::SNe { .. } => BOperand::IntImm((l != r) as i32),
                    LOpData::SEq { .. } => BOperand::IntImm((l == r) as i32),
                    LOpData::SGt { .. } => BOperand::IntImm((l > r) as i32),
                    LOpData::SLt { .. } => BOperand::IntImm((l < r) as i32),
                    LOpData::SGe { .. } => BOperand::IntImm((l >= r) as i32),
                    LOpData::SLe { .. } => BOperand::IntImm((l <= r) as i32),
                    LOpData::Xor { .. } => BOperand::IntImm(l ^ r),
                    LOpData::Shl { .. } => BOperand::IntImm(l << r),
                    LOpData::Shr { .. } => BOperand::IntImm(l >> r),
                    LOpData::Sar { .. } => BOperand::IntImm(l >> r),
                    _ => unreachable!("{:?} doesn't support int immediate folding", lop_data),
                }
            } else if let (BOperand::FloatImm(l), BOperand::FloatImm(r)) = (*lhs, *rhs) {
                let (l, r) = (f32::from_bits(l), f32::from_bits(r));
                match lop_data {
                    LOpData::AddF { .. } => BOperand::FloatImm((l + r).to_bits()),
                    LOpData::SubF { .. } => BOperand::FloatImm((l - r).to_bits()),
                    LOpData::MulF { .. } => BOperand::FloatImm((l * r).to_bits()),
                    LOpData::DivF { .. } => BOperand::FloatImm((l / r).to_bits()),
                    LOpData::ONe { .. } => BOperand::IntImm((l != r) as i32),
                    LOpData::OEq { .. } => BOperand::IntImm((l == r) as i32),
                    LOpData::OGt { .. } => BOperand::IntImm((l > r) as i32),
                    LOpData::OLt { .. } => BOperand::IntImm((l < r) as i32),
                    LOpData::OGe { .. } => BOperand::IntImm((l >= r) as i32),
                    LOpData::OLe { .. } => BOperand::IntImm((l <= r) as i32),
                    _ => unreachable!("{:?} doesn't support float immediate folding", lop_data),
                }
            } else {
                unreachable!("Constant folding for non-literal operands should have been prevented by the caller")
            }
        },
        un_ops: [Sitofp, Fptosi],
        un_arm: LOpData { value } => {
            unreachable!("Constant folding for unary ops is not allowed here")
        },
        fallback: {
            LOpData::Store { .. } |
            LOpData::Load { .. } |
            LOpData::Call { .. } |
            LOpData::Br { .. } |
            LOpData::Jump { .. } |
            LOpData::Move { .. } |
            LOpData::LoadFloatImm { .. } |
            LOpData::LoadIntImm { .. } |
            LOpData::LoadAddress { .. } |
            LOpData::Ret => {
                unreachable!("Constant folding for non-binary/unary ops is not allowed here")
            }
        }
    }
  }

  pub fn run(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let bb_ids = self.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      self.builder.set_current_block(bb_id);

      let inst_ids = self.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        self.builder.set_before_inst(
          self.ir.as_mut().unwrap(),
          self.builder.current_function,
          Some(inst_id),
        );
        let op = &self.get_func(func_id).dfg[inst_id];
        let (lop_data, typ) = (op.data.clone().into(), op.typ.clone());

        match_src! {
          target: lop_data,
          bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, AddF, SubF, MulF, DivF, Xor, Shl, Sar, Shr],
          bin_arm: LOpData { lhs, rhs } => {
            match (lhs.is_literal(), rhs.is_literal()) {
                // If both operands are literals, we can fold the operation at compile time.
                (true, true) => {
                    let folded = Self::fold(lop_data);
                    // RAUW
                    self.ir.as_mut().unwrap().replace_all_uses(Some(func_id), inst_id, folded);
                    // Remove the old instruction.
                    self.ir.as_mut().unwrap().remove_op(Some(func_id), inst_id, Some(bb_id));
                },
                (true, false) => {
                  // Canonicalize the operands to make the literal on the right, and adjust the operator if necessary.
                  // since the order of operands matters for the semantics of the operation.
                  let new_lop_data = match lop_data {
                    // For relational operations, we reverse the operation while swapping the operands
                    LOpData::SGt { rd, .. } => LOpData::SLt { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::SGe { rd, .. } => LOpData::SLe { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::SLt { rd, .. } => LOpData::SGt { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::SLe { rd, .. } => LOpData::SGe { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::OGt { rd, .. } => LOpData::OLt { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::OGe { rd, .. } => LOpData::OLe { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::OLt { rd, .. } => LOpData::OGt { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },
                    LOpData::OLe { rd, .. } => LOpData::OGe { rd, lhs: rhs, rhs: self.legalize(lhs, LegalizeOption::Default) },

                    // For Sub/Div/Mod/Shift, Operands can't be swapped, so we have to load lhs individually.
                    LOpData::SubF { rd, lhs: imm, rhs } =>
                      LOpData::SubF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::SubI { rd, lhs: imm, rhs } =>
                      LOpData::SubI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::DivF { rd, lhs: imm, rhs } =>
                      LOpData::DivF { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::DivI { rd, lhs: imm, rhs } =>
                      LOpData::DivI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::ModI { rd, lhs: imm, rhs } =>
                      LOpData::ModI { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::Shl { rd, lhs: imm, rhs } =>
                      LOpData::Shl { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::Shr { rd, lhs: imm, rhs } =>
                      LOpData::Shr { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },
                    LOpData::Sar { rd, lhs: imm, rhs } =>
                      LOpData::Sar { rd, lhs: self.legalize(imm, LegalizeOption::ForceImmLoad), rhs },

                    LOpData::AddI { rd, lhs: imm, rhs } =>
                      LOpData::AddI { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::AddF { rd, lhs: imm, rhs } =>
                      LOpData::AddF { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::MulI { rd, lhs: imm, rhs } =>
                      LOpData::MulI { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::MulF { rd, lhs: imm, rhs } =>
                      LOpData::MulF { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SNe { rd, lhs: imm, rhs } =>
                      LOpData::SNe { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SEq { rd, lhs: imm, rhs } =>
                      LOpData::SEq { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OEq { rd, lhs: imm, rhs } =>
                      LOpData::OEq { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::ONe { rd, lhs: imm, rhs } =>
                      LOpData::ONe { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::Xor { rd, lhs: imm, rhs } =>
                      LOpData::Xor { rd, lhs: rhs, rhs: self.legalize(imm, LegalizeOption::Default) },

                    _ => unreachable!("Unexpected op: {:?}", lop_data),
                  };
                  self.replace_op_rauw(inst_id, BOp::new(
                    typ,
                    vec![],
                    new_lop_data.into(),
                  ));
                },
                (false, true) => {
                  // No swap. Just legalize.
                  let new_lop_data = match lop_data {
                    LOpData::SGt { rd, lhs, rhs: imm } =>
                      LOpData::SGt { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SGe { rd, lhs, rhs: imm } =>
                      LOpData::SGe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SLt { rd, lhs, rhs: imm } =>
                      LOpData::SLt { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SLe { rd, lhs, rhs: imm } =>
                      LOpData::SLe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OGt { rd, lhs, rhs: imm } =>
                      LOpData::OGt { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OGe { rd, lhs, rhs: imm } =>
                      LOpData::OGe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OLt { rd, lhs, rhs: imm } =>
                      LOpData::OLt { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OLe { rd, lhs, rhs: imm } =>
                      LOpData::OLe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::AddI { rd, lhs, rhs: imm } =>
                      LOpData::AddI { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::AddF { rd, lhs, rhs: imm } =>
                      LOpData::AddF { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SNe { rd, lhs, rhs: imm } =>
                      LOpData::SNe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SEq { rd, lhs, rhs: imm } =>
                      LOpData::SEq { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::OEq { rd, lhs, rhs: imm } =>
                      LOpData::OEq { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::ONe { rd, lhs, rhs: imm } =>
                      LOpData::ONe { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::Xor { rd, lhs, rhs: imm } =>
                      LOpData::Xor { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SubF { rd, lhs, rhs: imm } =>
                      LOpData::SubF { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::SubI { rd, lhs, rhs: imm } =>
                      LOpData::SubI { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::Shl { rd, lhs, rhs: imm } =>
                      LOpData::Shl { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::Shr { rd, lhs, rhs: imm } =>
                      LOpData::Shr { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },
                    LOpData::Sar { rd, lhs, rhs: imm } =>
                      LOpData::Sar { rd, lhs, rhs: self.legalize(imm, LegalizeOption::Default) },

                    LOpData::MulI { rd, lhs, rhs: imm } =>
                      LOpData::MulI { rd, lhs, rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                    LOpData::MulF { rd, lhs, rhs: imm } =>
                      LOpData::MulF { rd, lhs, rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                    LOpData::DivF { rd, lhs, rhs: imm } =>
                      LOpData::DivF { rd, lhs, rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                    LOpData::DivI { rd, lhs, rhs: imm } =>
                      LOpData::DivI { rd, lhs, rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },
                    LOpData::ModI { rd, lhs, rhs: imm } =>
                      LOpData::ModI { rd, lhs, rhs: self.legalize(imm, LegalizeOption::ForceImmLoad) },

                    _ => unreachable!("Unexpected op with literal on the right: {:?}", lop_data),
                  };
                  self.replace_op_rauw(inst_id, BOp::new(
                    typ,
                    vec![],
                    new_lop_data.into(),
                  ));
                }
                (false, false) => {/*do nothing*/}
            }
          },
          un_ops: [Sitofp, Fptosi],
          un_arm: LOpData { value } => {
            // Legalize the operand if it's a literal, but don't fold it even if it's a constant, because folding might cause overflow which is undefined behavior in Rust.
            let new_lop_data = match lop_data {
              LOpData::Sitofp { rd, value } =>
                LOpData::Sitofp { rd, value: self.legalize(value, LegalizeOption::Default) },
              LOpData::Fptosi { rd, value } =>
                LOpData::Fptosi { rd, value: self.legalize(value, LegalizeOption::Default) },
              _ => unreachable!("Unexpected unary op: {:?}", lop_data),
            };
            self.replace_op_rauw(inst_id, BOp::new(
              typ,
              vec![],
              new_lop_data.into(),
            ));
          },
          fallback: {
            LOpData::Store { addr, value } => {
              // Mem operand should not be
              let new_lop_data = LOpData::Store { addr: self.legalize(addr, LegalizeOption::NoLoad), value: self.legalize(value, LegalizeOption::Default) };
              self.replace_op_rauw(inst_id, BOp::new(
                typ,
                vec![],
                new_lop_data.into(),
              ));
            }
            LOpData::Load { addr, rd } => {
              // Mem operand should not be legalized to Load again, otherwise it will cause infinite loop.
              let new_lop_data = LOpData::Load { addr: self.legalize(addr, LegalizeOption::NoLoad), rd };
              self.replace_op_rauw(inst_id, BOp::new(
                typ,
                vec![],
                new_lop_data.into(),
              ));
            },
            LOpData::Move { rd, src } => {
              // Move should not have literal operand, but we still legalize it just in case.
              let new_lop_data = LOpData::Move { rd, src: self.legalize(src, LegalizeOption::Default) };
              if let BOperand::Reg(Reg::X(_)) | BOperand::Reg(Reg::F(_)) = rd {
                self.replace_op_no_rauw(inst_id, BOp::new(
                  typ,
                  vec![],
                  new_lop_data.into(),
                ));
              } else {
                self.replace_op_rauw(inst_id, BOp::new(
                  typ,
                  vec![],
                  new_lop_data.into(),
                ));
              }
            }
            LOpData::Br { cond, then_bb, else_bb } => {
              let new_lop_data = LOpData::Br {
                cond: self.legalize(cond, LegalizeOption::Default),
                then_bb,
                else_bb,
              };
              self.replace_op_rauw(inst_id, BOp::new(
                typ,
                vec![],
                new_lop_data.into(),
              ));
            },
            LOpData::Call {..}
            | LOpData::Jump {..}
            | LOpData::Ret => {/*do nothing*/},
            LOpData::LoadAddress {..}
            | LOpData::LoadIntImm {..}
            | LOpData::LoadFloatImm {..} => unreachable!(),
          }
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
    self.ir = Some(program);
  }
  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = BOperand::Func(func_id);
      self.init(func_id);
      self.run();
    }
  }
}
