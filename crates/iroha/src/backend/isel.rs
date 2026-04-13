//! Instruction Selection (ISel).
//! Translating Lower IR into Machine IR.

use yachiyo::base::Type;
use yachiyo::ir::back::*;
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::{match_full_ops, match_some};

#[derive(Default)]
pub struct ISel<'a> {
  ir: Option<&'a mut BackIR>,
  builder: BBuilder,
}

impl ISel<'_> {
  pub fn init(&mut self, func_id: usize) {
    self.builder.set_current_func(BOperand::Func(func_id));
  }

  // ======== Atomic Operations ========

  #[inline(always)]
  fn alloc_rodata(&mut self, rodata: RoData) -> BOperand {
    BOperand::RoData(self.ir.as_mut().unwrap().rodata_info.alloc(rodata))
  }

  #[inline(always)]
  fn create(&mut self, bop: BOp) -> BOperand {
    self.builder.create(
      self.ir.as_mut().unwrap(),
      self.builder.current_function,
      bop,
    )
  }

  #[inline(always)]
  fn get_vreg_id(&self, op_id: BOperand) -> BOperand {
    let func_id = self
      .builder
      .current_function
      .expect("ISel: not in a function");
    self
      .ir
      .as_ref()
      .unwrap()
      .get_rd(Some(func_id), op_id)
      .cloned()
      .unwrap()
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

  pub fn select(&mut self, lop_id: BOperand) {
    let func_id = self
      .builder
      .current_function
      .expect("ISel: not in a function");
    let func = &self.ir.as_ref().unwrap().funcs[func_id];
    let bop = &func.dfg[lop_id];
    let (lop_data, is_phi_move, typ) = (
      bop.data.clone().into(),
      bop.attrs.contains(&BAttr::PhiMove),
      bop.typ.clone(),
    );

    // Set before current inst.
    self.builder.set_current_inst(lop_id);

    // For non-phi instructions, we still try to keep SSA form.
    match_full_ops! {
        target: &lop_data,
        bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
        bin_arm: LOpData { lhs, rhs, rd } => {
            match (lhs.is_literal(), rhs.is_literal()) {
                // We've canonicalized our operations.
                (true, true)
                | (true, false) => unreachable!("Unexpected pettern: lhs: {:?}, rhs: {:?}.", lhs, rhs),
                (false, true) => {
                    let (rs1, imm) = (*lhs, *rhs);
                    let mop_data = match_some! {
                        target: lop_data,
                        enu: LOpData,
                        minor_arms: {
                            // Xxxw operations extend the operand automatically.
                            // TODO: For now only 64-bits operation of Add is required. We might extend the support to other operations in the future.
                            LOpData::AddI { .. } => if typ == BType::I32 { MOpData::Addiw { rd: BOperand::Undef, rs1, imm } } else { MOpData::Addi { rd: BOperand::Undef, rs1, imm } },
                            LOpData::SubI { .. } => if typ == BType::I32 { MOpData::Addiw { rd: BOperand::Undef, rs1, imm: imm.negate_literal() } } else { MOpData::Addi { rd: BOperand::Undef, rs1, imm: imm.negate_literal() } },

                            LOpData::Shl { .. } => MOpData::Slliw { rd: BOperand::Undef, rs1, imm },
                            LOpData::Shr { .. } => MOpData::Srliw { rd: BOperand::Undef, rs1, imm },
                            LOpData::Sar { .. } => MOpData::Sraiw { rd: BOperand::Undef, rs1, imm },

                            LOpData::MulI { .. } => {
                                // Create li
                                let li_op_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Li { rd: BOperand::Undef, imm: imm.get_int_imm() }.into(),
                                    )
                                );
                                let li_vreg_id = self.get_vreg_id(li_op_id);
                                // Create mulw
                                MOpData::Mulw { rd: BOperand::Undef, rs1, rs2: li_vreg_id }
                            }

                            LOpData::DivI { .. }
                            | LOpData::ModI { .. } => unreachable!("{:?} should have been legalized in canonicalization", lop_data),

                            LOpData::Xor { .. } => {
                                // RISC-V doesn't have Xoriw, but we can still use Xori and let the upper bits be folded by the next instruction.
                                let xori_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Xori { rd: BOperand::Undef, rs1, imm }.into(),
                                    )
                                );
                                let xori_vreg_id = self.get_vreg_id(xori_mop_id);
                                // Extend the higher bits via Addiw.
                                // We don't need to fetch the vreg, since we reuse rd.
                                MOpData::Addw { rd: BOperand::Undef, rs1: xori_vreg_id, rs2: BOperand::Reg(Reg::X(XReg::Zero)) }
                            },

                            LOpData::SNe { .. } => {
                                // Create add
                                let addiw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Addiw { rd: BOperand::Undef, rs1, imm: imm.negate_literal() }.into(),
                                    )
                                );
                                let addiw_vreg_id = self.get_vreg_id(addiw_mop_id);
                                // Create sltiu with imm = 1, which is equivalent to checking whether the result of sub is non-zero.
                                MOpData::Sltu { rd: BOperand::Undef, rs1: BOperand::Reg(Reg::X(XReg::Zero)), rs2: addiw_vreg_id }
                            },
                            LOpData::SEq { .. } => {
                                // Create add
                                let addiw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Addiw { rd: BOperand::Undef, rs1, imm: imm.negate_literal() }.into(),
                                    )
                                );
                                let addiw_vreg_id = self.get_vreg_id(addiw_mop_id);
                                // Create sltiu with imm = 1, which is equivalent to checking whether the result of add is negative.
                                MOpData::Sltiu { rd: BOperand::Undef, rs1: addiw_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SGt { .. } => {
                                // x > 10 == x >= 11 == !(x < 11)
                                let imm = match imm {
                                    BOperand::IntImm(i) => BOperand::IntImm(i + 1),
                                    BOperand::FloatImm(_)
                                    | BOperand::Reg(_)
                                    | BOperand::BB(_)
                                    | BOperand::Func(_)
                                    | BOperand::Inst(_)
                                    | BOperand::Slot(_)
                                    | BOperand::Data(_)
                                    | BOperand::RoData(_)
                                    | BOperand::Bss(_)
                                    | BOperand::Undef => panic!("Expected an integer immediate for SGt, but got {:?}", imm),
                                };
                                // Create slti
                                let slti_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Slti { rd: BOperand::Undef, rs1, imm }.into(),
                                    )
                                );
                                let slti_vreg_id = self.get_vreg_id(slti_mop_id);
                                // Create Xori to flip the result.
                                MOpData::Xori { rd: BOperand::Undef, rs1: slti_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SLt { .. } => {
                                // Create slti
                                MOpData::Slti { rd: BOperand::Undef, rs1, imm }
                            }
                            LOpData::SGe { .. } => {
                                // Reuse slti
                                let slti_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Slti { rd: BOperand::Undef, rs1, imm }.into(),
                                    )
                                );
                                let slti_vreg_id = self.get_vreg_id(slti_mop_id);
                                // Create Xori
                                MOpData::Xori { rd: BOperand::Undef, rs1: slti_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SLe { .. } => {
                                // x <= 10 == x < 11
                                let imm = match imm {
                                    BOperand::IntImm(i) => BOperand::IntImm(i + 1),
                                    BOperand::FloatImm(_)
                                    | BOperand::Reg(_)
                                    | BOperand::BB(_)
                                    | BOperand::Func(_)
                                    | BOperand::Inst(_)
                                    | BOperand::Slot(_)
                                    | BOperand::Data(_)
                                    | BOperand::RoData(_)
                                    | BOperand::Bss(_)
                                    | BOperand::Undef => unreachable!("Unexpected operand: {:?}", imm),
                                };
                                // Create slti
                                MOpData::Slti { rd: BOperand::Undef, rs1, imm }
                            }
                        },
                        // Since we've legalized float immediates in lowering, lhs and rhs can't be literals.
                        uni_ops: [Sitofp, Fptosi, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, LoadAddress, Ret, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
                        uni_arm: {
                            unreachable!("{:?} should have been legalized in legalization", lop_data)
                        }
                    };

                    self.replace_op_rauw(lop_id, BOp::new(typ.clone(), vec![], mop_data.into()));
                },
                (false, false) => {
                    let (rs1, rs2) = (*lhs, *rhs);
                    let mop_data = match_some! {
                        target: lop_data,
                        enu: LOpData,
                        minor_arms: {
                            LOpData::AddI { .. } => if typ == BType::I32 { MOpData::Addw { rd: BOperand::Undef, rs1, rs2 } } else { MOpData::Add { rd: BOperand::Undef, rs1, rs2 } },
                            LOpData::SubI { .. } => if typ == BType::I32 { MOpData::Subw { rd: BOperand::Undef, rs1, rs2 } } else { MOpData::Sub { rd: BOperand::Undef, rs1, rs2 } },

                            LOpData::MulI { .. } => MOpData::Mulw { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::DivI { .. } => MOpData::Divw { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::ModI { .. } => MOpData::Remw { rd: BOperand::Undef, rs1, rs2 },

                            LOpData::Shl { .. } => MOpData::Sllw { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::Shr { .. } => MOpData::Srlw { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::Sar { .. } => MOpData::Sraw { rd: BOperand::Undef, rs1, rs2 },

                            // If the operand is a 32-bit immediate, RISC-V will automatically fill the higher bits with 1, so Xxxw is not needed.
                            LOpData::AddF { .. } => MOpData::FaddS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::SubF { .. } => MOpData::FsubS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::MulF { .. } => MOpData::FmulS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::DivF { .. } => MOpData::FdivS { rd: BOperand::Undef, rs1, rs2 },

                            LOpData::SNe { .. } => {
                                // Create sub
                                let subw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Subw { rd: BOperand::Undef, rs1, rs2 }.into(),
                                    )
                                );
                                let subw_vreg_id = self.get_vreg_id(subw_mop_id);
                                // Create sltu with imm = 1, which is equivalent to checking whether the result of sub is non-zero.
                                MOpData::Sltu { rd: BOperand::Undef, rs1: BOperand::Reg(Reg::X(XReg::Zero)), rs2: subw_vreg_id }
                            }
                            LOpData::SEq { .. } => {
                                // Create sub
                                let subw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Subw { rd: BOperand::Undef, rs1, rs2 }.into(),
                                    )
                                );
                                let subw_vreg_id = self.get_vreg_id(subw_mop_id);
                                // Create sltu with imm = 1, which is equivalent to checking whether the result of sub is zero.
                                MOpData::Sltiu { rd: BOperand::Undef, rs1: subw_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SGt { .. } => {
                                // x > y == x >= y + 1 == !(x < y + 1)
                                // Create addi to calculate y + 1
                                let imm = BOperand::IntImm(1);
                                let addiw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Addiw { rd: BOperand::Undef, rs1: rs2, imm }.into(),
                                    )
                                );
                                let addiw_vreg_id = self.get_vreg_id(addiw_mop_id);
                                // Create slt
                                let slt_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Slt { rd: BOperand::Undef, rs1, rs2: addiw_vreg_id }.into(),
                                    )
                                );
                                let slt_vreg_id = self.get_vreg_id(slt_mop_id);
                                // Create Xori to flip the result.
                                MOpData::Xori { rd: BOperand::Undef, rs1: slt_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SLt { .. } => {
                                // Create slt
                                MOpData::Slt { rd: BOperand::Undef, rs1, rs2 }
                            }
                            LOpData::SGe { .. } => {
                                // Reuse slt
                                let slt_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Slt { rd: BOperand::Undef, rs1: rs2, rs2: rs1 }.into(),
                                    )
                                );
                                let slt_vreg_id = self.get_vreg_id(slt_mop_id);
                                // Create Xori
                                MOpData::Xori { rd: BOperand::Undef, rs1: slt_vreg_id, imm: BOperand::IntImm(1) }
                            }
                            LOpData::SLe { .. } => {
                                // x <= y == x < y + 1
                                // Create addi to calculate y + 1
                                let imm = BOperand::IntImm(1);
                                let addiw_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Addiw { rd: BOperand::Undef, rs1: rs2, imm }.into(),
                                    )
                                );
                                let addiw_vreg_id = self.get_vreg_id(addiw_mop_id);
                                // Create slt
                                MOpData::Slt { rd: BOperand::Undef, rs1, rs2: addiw_vreg_id }
                            }

                            LOpData::Xor { .. } => {
                                let xor_mop_id = self.create(
                                    BOp::new(
                                        typ.clone(),
                                        vec![],
                                        MOpData::Xor { rd: BOperand::Undef, rs1, rs2 }.into(),
                                    )
                                );
                                let xor_vreg_id = self.get_vreg_id(xor_mop_id);
                                MOpData::Addw { rd: BOperand::Undef, rs1: xor_vreg_id, rs2: BOperand::Reg(Reg::X(XReg::Zero)) }
                            }

                            // For relational ops with Float, we use the pseudo ops.
                            LOpData::ONe { .. } => MOpData::FneS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::OEq { .. } => MOpData::FeqS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::OGt { .. } => MOpData::FgtS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::OLt { .. } => MOpData::FltS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::OGe { .. } => MOpData::FgeS { rd: BOperand::Undef, rs1, rs2 },
                            LOpData::OLe { .. } => MOpData::FleS { rd: BOperand::Undef, rs1, rs2 },
                        },
                        uni_ops: [Sitofp, Fptosi, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, LoadAddress, Ret],
                        uni_arm: {
                            unreachable!("{:?} should have been legalized in legalization", lop_data)
                        }
                    };

                    self.replace_op_rauw(lop_id, BOp::new(typ.clone(), vec![], mop_data.into()));
                }
            }
        },
        un_ops: [Sitofp, Fptosi],
        un_arm: LOpData { rd, value } => {
            let mop_data = match_some! {
                target: lop_data,
                enu: LOpData,
                minor_arms: {
                    LOpData::Sitofp { .. } => MOpData::FcvtSW { rd: BOperand::Undef, rs: *value },
                    LOpData::Fptosi { .. } => MOpData::FcvtWS { rd: BOperand::Undef, rs: *value },
                },
                uni_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, Ret, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, LoadAddress],
                uni_arm: {
                    unreachable!("{:?} should have been legalized in legalization", lop_data)
                }
            };

            self.replace_op_rauw(lop_id, BOp::new(typ.clone(), vec![], mop_data.into()));
        },
        fallback: {
            LOpData::Store {..}
            | LOpData::Load {..} => {/*do nothing. Store/Load will be lowered in Post-RA later. */}

            LOpData::Call { func } => {
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::Call { target: *func }.into(),
                    ),
                );
            }

            LOpData::Br { cond, then_bb, else_bb } => {
                self.create(
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::Bnez { rs: *cond, target: *then_bb }.into(),
                    )
                );
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::J { target: *else_bb }.into(),
                    ),
                );
            }

            LOpData::Jump { target_bb } => {
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::J { target: *target_bb }.into()
                    ),
                );
            }

            LOpData::Move { rd, src } => {
                match rd {
                    // If the destination is a physical register, we must reuse it and never perform rauw on it.
                    BOperand::Reg(Reg::X(_))
                    | BOperand::Reg(Reg::F(_)) => {
                        // For register destination, we can directly use Mv/Fmv.
                        let mop_data = match typ {
                            BType::I32
                            | BType::U64
                            | BType::Array { .. } => MOpData::Mv { rd: *rd, rs: *src },
                            BType::F32 => MOpData::FmvS { rd: *rd, rs: *src },
                            BType::Void => unreachable!("Move with void type doesn't make sense"),
                        };
                        self.replace_op_no_rauw(
                            lop_id,
                            BOp::new(
                                typ.clone(),
                                vec![],
                                // For Move, we still use the original rd.
                                mop_data.into(),
                            ),
                        );
                    },
                    // Else if the destination is a virtual register, we
                    BOperand::Reg(Reg::Virt(_)) => {
                        let rd = if is_phi_move { *rd } else { BOperand::Undef };
                        let mop_data = match typ {
                            BType::I32
                            | BType::U64
                            | BType::Array { .. } => MOpData::Mv { rd, rs: *src },
                            BType::F32 => MOpData::FmvS { rd, rs: *src },
                            BType::Void => unreachable!("Move with void type doesn't make sense"),
                        };
                        if is_phi_move {
                            self.replace_op_no_rauw(
                                lop_id,
                                BOp::new(
                                    typ.clone(),
                                    vec![],
                                    mop_data.into(),
                                ),
                            );
                        } else {
                            self.replace_op_rauw(
                                lop_id,
                                BOp::new(
                                    typ.clone(),
                                    vec![],
                                    mop_data.into(),
                                ),
                            );
                        }
                    },
                    BOperand::Slot(_) | BOperand::Data(_) | BOperand::RoData(_) | BOperand::Bss(_) | BOperand::Func(_) | BOperand::BB(_) | BOperand::Inst(_) | BOperand::Undef | BOperand::FloatImm(_) | BOperand::IntImm(_) => unreachable!("Unexpected destination operand for Move: {:?}", rd),
                };
            }

            LOpData::LoadFloatImm { imm, .. } => {
                // Add it to the constant pool first.
                let rodata_id = self.alloc_rodata(RoData::new(
                    Type::Float,
                    vec![BOperand::FloatImm(imm.to_bits())],
                ));
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        BType::F32,
                        vec![],
                        // CAUTION: Create LOpData::Load here.
                        LOpData::Load { rd: BOperand::Undef, addr: rodata_id }.into(),
                    )
                );
            }

            LOpData::LoadIntImm { imm, .. } => {
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::Li { rd: BOperand::Undef, imm: *imm }.into(),
                    )
                );
            }

            LOpData::LoadAddress { addr, .. } => {
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        typ.clone(),
                        vec![],
                        MOpData::La { rd: BOperand::Undef, target: *addr }.into(),
                    )
                );
            }

            LOpData::Ret => {
                // For non-binbinary/unary ops, we simply emit them as is.
                self.replace_op_rauw(
                    lop_id,
                    BOp::new(
                        BType::Void,
                        vec![],
                        MOpData::Ret.into(),
                    ),
                );
            }
        }
    };
  }
}

impl<'a> BPass<'a> for ISel<'a> {
  fn name(&self) -> &str {
    "ISel"
  }

  fn mount(&mut self, program: &'a mut BackIR) {
    self.ir = Some(program);
  }

  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      self.init(func_id);

      let ids = {
        let func = &self.ir.as_ref().unwrap().funcs[func_id];
        func.cfg.ids()
      };
      for bb_id in ids {
        self.builder.set_current_block(BOperand::BB(bb_id));
        let cur = {
          let func = &self.ir.as_ref().unwrap().funcs[func_id];
          let bb = &func.cfg[bb_id];
          bb.cur.clone()
        };
        for op_id in cur {
          self.select(op_id);
        }
      }
    }
  }
}
