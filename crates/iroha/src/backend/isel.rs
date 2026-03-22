//! Instruction Selection (ISel).
//! Translating Lower IR into Machine IR.

use yachiyo::ir::back::*;
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::{match_full_ops, match_some, match_src};

#[derive(Default)]
pub struct ISel<'a> {
    ir: Option<&'a mut BackIR>,
    builder: BBuilder,
}

impl ISel<'_> {
    pub fn init(&mut self, func_id: usize) {
        self.builder.set_current_func(BOperand::Func(func_id));
    }

    fn fold(lop_data: LOpData) -> BOperand {
        match_src! {
            target: &lop_data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: LOpData { lhs, rhs } => {
                if let (BOperand::IntImm(l), BOperand::IntImm(r)) = (lhs.clone(), rhs.clone()) {
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
                } else if let (BOperand::FloatImm(l), BOperand::FloatImm(r)) = (lhs.clone(), rhs.clone()) {
                    match lop_data {
                        LOpData::AddF { .. } => BOperand::FloatImm(l + r),
                        LOpData::SubF { .. } => BOperand::FloatImm(l - r),
                        LOpData::MulF { .. } => BOperand::FloatImm(l * r),
                        LOpData::DivF { .. } => BOperand::FloatImm(l / r),
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
                LOpData::Ret => {
                    unreachable!("Constant folding for non-binary/unary ops is not allowed here")
                }
            }
        }
    }

    // ======== Atomic Operations ========

    #[inline(always)]
    fn alloc_rodata(&mut self, rodata: RoData) -> BOperand {
        BOperand::RoData(self.ir.as_mut().unwrap().rodata_info.alloc(rodata))
    }

    #[inline(always)]
    fn create(&mut self, bop: BOp) -> BOperand {
        let func_id = self
            .builder
            .current_function
            .clone()
            .expect("ISel: not in a function");
        let func = &mut self.ir.as_mut().unwrap().funcs[func_id.get_func_id()];
        let bop_id = func.dfg.alloc(bop);
        BOperand::Inst(bop_id)
    }

    #[inline(always)]
    fn get_vreg_id(&self, op_id: BOperand) -> BOperand {
        let func_id = self
            .builder
            .current_function
            .clone()
            .expect("ISel: not in a function");
        self.ir
            .as_ref()
            .unwrap()
            .get_rd(Some(func_id), op_id)
            .unwrap()
    }

    #[inline(always)]
    fn replace_op(&mut self, old_id: BOperand, new_op: BOp) {
        let func_id = self
            .builder
            .current_function
            .clone()
            .expect("ISel: not in a function");
        let current_block = self
            .builder
            .current_block
            .clone()
            .expect("Not current block found");
        self.ir.as_mut().unwrap().replace_op(
            &mut self.builder,
            Some(func_id),
            old_id,
            current_block,
            new_op,
        );
    }

    pub fn select(&mut self, lop_id: BOperand) {
        let func_id = self
            .builder
            .current_function
            .clone()
            .expect("ISel: not in a function");
        let func = &self.ir.as_ref().unwrap().funcs[func_id.clone()];
        let bop = &func.dfg[lop_id.clone()];
        let (lop_data, typ) = (bop.data.clone().into(), bop.typ.clone());

        match_full_ops! {
            target: &lop_data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: LOpData { lhs, rhs, rd } => {
                match (lhs.is_literal(), rhs.is_literal()) {
                    // If both operands are literals, we can fold the operation at compile time.
                    (true, true) => {
                        let folded = Self::fold(lop_data);
                        // RAUW
                        self.ir.as_mut().unwrap().replace_all_uses(Some(func_id), lop_id, folded)
                    }
                    // If one of 'em is literal, we use XxxI operation and canonicalize the operands.
                    (true, false) | (false, true) => {
                        // Canonicalize and obscure the original lop_data.
                        let (lop_data, rs1, imm) = if let (true, false) = (lhs.is_literal(), rhs.is_literal()) {
                            if !lop_data.is_rel() {
                                // If lhs is literal and rhs is not, we swap them to maintain the canonical form.
                                (lop_data.clone(), rhs, lhs)
                            } else {
                                // For relational operations, we reverse the operation while swapping the operands
                                // since the order of operands matters for the semantics of the operation.
                                let lop_data = match lop_data {
                                    LOpData::SGt { .. } => LOpData::SLt { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::SGe { .. } => LOpData::SLe { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::SLt { .. } => LOpData::SGt { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::SLe { .. } => LOpData::SGe { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::OGt { .. } => LOpData::OLt { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::OGe { .. } => LOpData::OLe { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::OLt { .. } => LOpData::OGt { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    LOpData::OLe { .. } => LOpData::OGe { rd: rd.clone(), lhs: rhs.clone(), rhs: lhs.clone() },
                                    _ => lop_data.clone(),
                                };
                                (lop_data, rhs, lhs)
                            }
                        } else {
                            (lop_data.clone(), lhs, rhs)
                        };

                        let mop_data = match_some! {
                            target: lop_data,
                            enu: LOpData,
                            minor_arms: {
                                // Xxxw operations extend the operand automatically.
                                LOpData::AddI { .. } => MOpData::Addiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::SubI { .. } => MOpData::Subiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::MulI { .. } => MOpData::Muliw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::DivI { .. } => MOpData::Diviw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::ModI { .. } => MOpData::Remiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },

                                LOpData::Shl { .. } => MOpData::Slliw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::Shr { .. } => MOpData::Srliw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },
                                LOpData::Sar { .. } => MOpData::Sraiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() },

                                LOpData::Xor { .. } => {
                                    // RISC-V doesn't have Xoriw, but we can still use Xori and let the upper bits be folded by the next instruction.
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Xori { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() }.into(),
                                        )
                                    );
                                    // Extend the higher bits via Addiw.
                                    // We don't need to fetch the vreg, since we reuse rd.
                                    MOpData::Addw { rd: rd.clone(), rs1: rd.clone(), rs2: BOperand::Reg(Reg::X(XReg::Zero)) }
                                },

                                LOpData::SNe { .. } => {
                                    // Create sub
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Subiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() }.into(),
                                        )
                                    );
                                    // Create sltiu with imm = 1, which is equivalent to checking whether the result of sub is non-zero.
                                    MOpData::Sltu { rd: rd.clone(), rs1: BOperand::Reg(Reg::X(XReg::Zero)), rs2: rd.clone() }
                                },
                                LOpData::SEq { .. } => {
                                    // Create sub
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Subiw { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() }.into(),
                                        )
                                    );
                                    // Create sltiu with imm = 1, which is equivalent to checking whether the result of sub is negative.
                                    MOpData::Sltiu { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
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
                                        | BOperand::Undef => panic!("Expected an integer immediate for SGt, but got {:?}", imm),
                                    };
                                    // Create slti
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Slti { rd: rd.clone(), rs1: rs1.clone(), imm: imm.clone() }.into(),
                                        )
                                    );
                                    // Create Xori to flip the result.
                                    MOpData::Xori { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
                                }
                                LOpData::SLt { .. } => {
                                    // Create slti
                                    MOpData::Slti { rd: rd.clone(), rs1: rhs.clone(), imm: imm.clone() }
                                }
                                LOpData::SGe { .. } => {
                                    // Reuse slti
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Slti { rd: rd.clone(), rs1: rhs.clone(), imm: imm.clone() }.into(),
                                        )
                                    );
                                    // Create Xori
                                    MOpData::Xori { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
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
                                        | BOperand::Undef => panic!("Expected an integer immediate for SLe, but got {:?}", imm),
                                    };
                                    // Create slti
                                    MOpData::Slti { rd: rd.clone(), rs1: rhs.clone(), imm: imm.clone() }
                                }
                            },
                            // Since we've legalized float immediates in lowering, lhs and rhs can't be literals.
                            uni_ops: [Sitofp, Fptosi, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, Ret, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
                            uni_arm: {
                                unreachable!("{:?} should have been legalized in legalization", lop_data)
                            }
                        };

                        self.replace_op(lop_id.clone(), BOp::new(typ.clone(), vec![], mop_data.into()));
                    },
                    (false, false) => {
                        let (rs1, rs2) = (lhs, rhs);
                        let mop_data = match_some! {
                            target: lop_data,
                            enu: LOpData,
                            minor_arms: {
                                LOpData::AddI { .. } => MOpData::Addw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::SubI { .. } => MOpData::Subw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::MulI { .. } => MOpData::Mulw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::DivI { .. } => MOpData::Divw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::ModI { .. } => MOpData::Remw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },

                                LOpData::Shl { .. } => MOpData::Sllw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::Shr { .. } => MOpData::Srlw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::Sar { .. } => MOpData::Sraw { rd: rd.clone(), rs1: rs1.clone(), rs2:	rs2.clone() },

                                // If the operand is a 32-bit immediate, RISC-V will automatically fill the higher bits with 1, so Xxxw is not needed.
                                LOpData::AddF { .. } => MOpData::FaddS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::SubF { .. } => MOpData::FsubS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::MulF { .. } => MOpData::FmulS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::DivF { .. } => MOpData::FdivS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },

                                LOpData::SNe { .. } => {
                                    // Create sub
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Subw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() }.into(),
                                        )
                                    );
                                    // Create sltu with imm = 1, which is equivalent to checking whether the result of sub is non-zero.
                                    MOpData::Sltu { rd: rd.clone(), rs1: BOperand::Reg(Reg::X(XReg::Zero)), rs2: rd.clone() }
                                }
                                LOpData::SEq { .. } => {
                                    // Create sub
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Subw { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() }.into(),
                                        )
                                    );
                                    // Create sltu with imm = 1, which is equivalent to checking whether the result of sub is zero.
                                    MOpData::Sltiu { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
                                }
                                LOpData::SGt { .. } => {
                                    // x > y == x >= y + 1 == !(x < y + 1)
                                    // Create addi to calculate y + 1
                                    let imm = BOperand::IntImm(1);
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Addiw { rd: rd.clone(), rs1: rhs.clone(), imm }.into(),
                                        )
                                    );
                                    // Create slt
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Slt { rd: rd.clone(), rs1: rs1.clone(), rs2: rhs.clone() }.into(),
                                        )
                                    );
                                    // Create Xori to flip the result.
                                    MOpData::Xori { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
                                }
                                LOpData::SLt { .. } => {
                                    // Create slt
                                    MOpData::Slt { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() }
                                }
                                LOpData::SGe { .. } => {
                                    // Reuse slt
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Slt { rd: rd.clone(), rs1: rhs.clone(), rs2: lhs.clone() }.into(),
                                        )
                                    );
                                    // Create Xori
                                    MOpData::Xori { rd: rd.clone(), rs1: rd.clone(), imm: BOperand::IntImm(1) }
                                }
                                LOpData::SLe { .. } => {
                                    // x <= y == x < y + 1
                                    // Create addi to calculate y + 1
                                    let imm = BOperand::IntImm(1);
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Addiw { rd: rd.clone(), rs1: rhs.clone(), imm }.into(),
                                        )
                                    );
                                    // Create slt
                                    MOpData::Slt { rd: rd.clone(), rs1: rs1.clone(), rs2: rhs.clone() }
                                }

                                LOpData::Xor { .. } => {
                                    self.create(
                                        BOp::new(
                                            typ.clone(),
                                            vec![],
                                            MOpData::Xor { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() }.into(),
                                        )
                                    );
                                    MOpData::Addw { rd: rd.clone(), rs1: rd.clone(), rs2: BOperand::Reg(Reg::X(XReg::Zero)) }
                                }

                                // For relational ops with Float, we use the pseudo ops.
                                LOpData::ONe { .. } => MOpData::FneS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::OEq { .. } => MOpData::FeqS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::OGt { .. } => MOpData::FgtS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::OLt { .. } => MOpData::FltS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::OGe { .. } => MOpData::FgeS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                                LOpData::OLe { .. } => MOpData::FleS { rd: rd.clone(), rs1: rs1.clone(), rs2: rs2.clone() },
                            },
                            uni_ops: [Sitofp, Fptosi, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, Ret],
                            uni_arm: {
                                unreachable!("{:?} should have been legalized in legalization", lop_data)
                            }
                        };

                        self.replace_op(lop_id.clone(), BOp::new(typ.clone(), vec![], mop_data.into()));
                    }
                }
            },
            un_ops: [Sitofp, Fptosi],
            un_arm: LOpData { rd, value } => {
                let mop_data = match_some! {
                    target: lop_data,
                    enu: LOpData,
                    minor_arms: {
                        LOpData::Sitofp { .. } => MOpData::FcvtSW { rd: rd.clone(), rs: value.clone() },
                        LOpData::Fptosi { .. } => MOpData::FcvtWS { rd: rd.clone(), rs: value.clone() },
                    },
                    uni_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, Store, Load, Call, Br, Jump, Move, LoadFloatImm, LoadIntImm, Ret, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
                    uni_arm: {
                        unreachable!("{:?} should have been legalized in legalization", lop_data)
                    }
                };

                self.replace_op(lop_id.clone(), BOp::new(typ.clone(), vec![], mop_data.into()));
            },
            fallback: {
                LOpData::Store {..}
                | LOpData::Load {..} => {/*do nothing. Store/Load should not be rewritten in ISel, since concrete offset is known. */}

                LOpData::Call { func } => {
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            typ.clone(),
                            vec![],
                            MOpData::Call { target: func.clone() }.into(),
                        ),
                    );
                }

                LOpData::Br { cond, then_bb, else_bb } => {
                    self.create(
                        BOp::new(
                            typ.clone(),
                            vec![],
                            MOpData::Bnez { rs: cond.clone(), target: then_bb.clone() }.into(),
                        )
                    );
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            typ.clone(),
                            vec![],
                            MOpData::J { target: else_bb.clone() }.into(),
                        ),
                    );
                }

                LOpData::Jump { target_bb } => {
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            typ.clone(),
                            vec![],
                            MOpData::J { target: target_bb.clone() }.into()
                        ),
                    );
                }

                LOpData::Move { rd, src } => {
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            typ.clone(),
                            vec![],
                            match typ {
                                BType::I32
                                | BType::U64 => MOpData::Mv { rd: rd.clone(), rs: src.clone() }.into(),
                                BType::F32 => MOpData::FmvS { rd: rd.clone(), rs: src.clone() }.into(),
                                BType::Void => unreachable!("Move with void type doesn't make sense"),
                            }
                        ),
                    );
                }

                LOpData::LoadFloatImm { rd, imm } => {
                    // Add it to the constant pool first.
                    let rodata_id = self.alloc_rodata(RoData::new(
                        typ.clone(),
                        vec![BOperand::FloatImm(*imm)],
                    ));
                    let load_lop_id = self.create(
                        BOp::new(
                            BType::I32,
                            vec![],
                            // CAUTION: Create LOpData::Load here.
                            LOpData::Load { rd: rd.clone(), addr: rodata_id.clone() }.into(),
                        )
                    );
                    let load_vreg_id = self.get_vreg_id(load_lop_id);
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            BType::F32,
                            vec![],
                            MOpData::FmvWX { rd: rd.clone(), rs: load_vreg_id }.into(),
                        ),
                    );
                }

                LOpData::LoadIntImm { rd, imm } => {
                    self.replace_op(
                        lop_id.clone(),
                        BOp::new(
                            typ.clone(),
                            vec![],
                            MOpData::Li { rd: rd.clone(), imm: *imm }.into(),
                        )
                    );
                }

                LOpData::Ret => {
                    // For non-binbinary/unary ops, we simply emit them as is.
                    self.replace_op(
                        lop_id.clone(),
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
        for func_id in self.ir.as_ref().unwrap().funcs.ids() {
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
