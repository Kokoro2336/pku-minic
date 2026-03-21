//! Lower IR module defintion, with graph structure variation APIs.

use super::{
    BBuilder, BBuilderGuard, BOp, BOpData, BOperand, DataInfo, LOpData, MOpData, Reg, RoDataInfo,
    VirtReg, BCFG, BCG, BDFG,
};
use crate::utils::arena::ArenaItem;
use crate::utils::r#match::{match_minor, match_ops, match_rd};

pub struct BackIR {
    pub data_info: DataInfo,
    pub rodata_info: RoDataInfo,
    pub funcs: BCG,
}

impl Default for BackIR {
    fn default() -> Self {
        Self::new()
    }
}

impl BackIR {
    pub fn new() -> Self {
        Self {
            data_info: DataInfo::new(),
            rodata_info: RoDataInfo::new(),
            funcs: BCG::new(),
        }
    }

    pub(crate) fn cfg_mut_or_panic(
        &mut self,
        current_function: Option<BOperand>,
        msg: &str,
    ) -> &mut BCFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].cfg
    }

    fn dfg_mut_or_panic(&mut self, current_function: Option<BOperand>, msg: &str) -> &mut BDFG {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        &mut self.funcs[idx].dfg
    }

    fn cfg_dfg_mut_or_panic(
        &mut self,
        current_function: Option<BOperand>,
        msg: &str,
    ) -> (&mut BCFG, &mut BDFG) {
        let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
        let func = &mut self.funcs[idx];
        (&mut func.cfg, &mut func.dfg)
    }

    pub fn add_uses(&mut self, current_function: Option<BOperand>, op: BOperand) {
        let dfg = self.dfg_mut_or_panic(
            current_function.clone(),
            "BackIR add_uses: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
        match data {
            BOpData::L(data) => match_ops! {
                target: data,
                bin_ops: [
                    AddI, SubI, MulI, DivI, ModI,
                    SNe, SEq, SGt, SLt, SGe, SLe,
                    Xor, Shl, Shr, Sar,
                    AddF, SubF, MulF, DivF,
                    ONe, OEq, OGt, OLt, OGe, OLe
                ],
                bin_arm: LOpData { lhs, rhs } => {
                    vregs.add_use(lhs, op.clone());
                    vregs.add_use(rhs, op);
                },
                un_ops: [Sitofp, Fptosi],
                un_arm: LOpData { value } => {
                    vregs.add_use(value, op);
                },
                fallback: {
                    LOpData::Load { addr, .. } => {
                        vregs.add_use(addr, op);
                    }
                    LOpData::Store { addr, value } => {
                        vregs.add_use(addr, op.clone());
                        vregs.add_use(value, op);
                    }
                    LOpData::Br { cond, .. } => {
                        vregs.add_use(cond, op);
                    }
                    LOpData::Move { src, .. } => {
                        vregs.add_use(src, op);
                    }
                    LOpData::Call { .. }
                    | LOpData::Jump { .. }
                    | LOpData::Ret
                    | LOpData::LoadIntImm { .. }
                    | LOpData::LoadFloatImm { .. } => {}
                }
            },
            BOpData::M(data) => match_ops! {
                target: data,
                bin_ops: [
                    Addw, Subw, Mulw, Divw, Remw,
                    Sllw, Srlw, Sraw,
                    Slt, Sltu, Xor,
                    FaddS, FsubS, FmulS, FdivS,
                    FeqS, FltS, FleS, FneS, FgtS, FgeS,
                    Beq, Bne, Blt, Bge, Bltu, Bgeu
                ],
                bin_arm: MOpData { rs1, rs2 } => {
                    vregs.add_use(rs1, op.clone());
                    vregs.add_use(rs2, op);
                },
                un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
                un_arm: MOpData { rs } => {
                    vregs.add_use(rs, op);
                },
                fallback: {
                    MOpData::Slti { rs1, imm, .. }
                    | MOpData::Sltiu { rs1, imm, .. }
                    | MOpData::Addiw { rs1, imm, .. }
                    | MOpData::Subiw { rs1, imm, .. }
                    | MOpData::Muliw { rs1, imm, .. }
                    | MOpData::Diviw { rs1, imm, .. }
                    | MOpData::Remiw { rs1, imm, .. }
                    | MOpData::Slliw { rs1, imm, .. }
                    | MOpData::Srliw { rs1, imm, .. }
                    | MOpData::Sraiw { rs1, imm, .. }
                    | MOpData::Xori { rs1, imm, .. } => {
                        vregs.add_use(rs1, op.clone());
                        vregs.add_use(imm, op);
                    }
                    MOpData::Lw { base, offset, .. }
                    | MOpData::Flw { base, offset, .. }
                    | MOpData::Ld { base, offset, .. } => {
                        vregs.add_use(base, op.clone());
                        vregs.add_use(offset, op);
                    }
                    MOpData::Sw { rs, base, offset }
                    | MOpData::Fsw { rs, base, offset }
                    | MOpData::Sd { rs, base, offset } => {
                        vregs.add_use(rs, op.clone());
                        vregs.add_use(base, op.clone());
                        vregs.add_use(offset, op);
                    }
                    MOpData::Li { .. } => {}
                    MOpData::La { .. } => {}
                    MOpData::J { .. } => {}
                    MOpData::Bnez { rs, .. } => {
                        vregs.add_use(rs, op.clone());
                    }
                    MOpData::Call { .. } => {}
                    MOpData::Ret => {}
                }
            },
        }
    }

    /// Remove the use of the operand's vreg.
    pub fn remove_uses(&mut self, current_function: Option<BOperand>, op: BOperand) {
        let dfg = self.dfg_mut_or_panic(
            current_function.clone(),
            "BackIR remove_uses: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
        match data {
            BOpData::L(data) => match_ops! {
                target: data,
                bin_ops: [
                    AddI, SubI, MulI, DivI, ModI,
                    SNe, SEq, SGt, SLt, SGe, SLe,
                    Xor, Shl, Shr, Sar,
                    AddF, SubF, MulF, DivF,
                    ONe, OEq, OGt, OLt, OGe, OLe
                ],
                bin_arm: LOpData { lhs, rhs } => {
                    vregs.remove_use(lhs, op.clone());
                    vregs.remove_use(rhs, op);
                },
                un_ops: [Sitofp, Fptosi],
                un_arm: LOpData { value } => {
                    vregs.remove_use(value, op);
                },
                fallback: {
                    LOpData::Load { addr, .. } => {
                        vregs.remove_use(addr, op);
                    }
                    LOpData::Store { addr, value } => {
                        vregs.remove_use(addr, op.clone());
                        vregs.remove_use(value, op);
                    }
                    LOpData::Br { cond, .. } => {
                        vregs.remove_use(cond, op);
                    }
                    LOpData::Move { src, .. } => {
                        vregs.remove_use(src, op);
                    }
                    LOpData::Call { .. }
                    | LOpData::Jump { .. }
                    | LOpData::Ret
                    | LOpData::LoadIntImm { .. }
                    | LOpData::LoadFloatImm { .. } => {}
                }
            },
            BOpData::M(data) => match_ops! {
                target: data,
                bin_ops: [
                    Addw, Subw, Mulw, Divw, Remw,
                    Sllw, Srlw, Sraw,
                    Slt, Sltu, Xor,
                    FaddS, FsubS, FmulS, FdivS,
                    FeqS, FltS, FleS, FneS, FgtS, FgeS,
                    Beq, Bne, Blt, Bge, Bltu, Bgeu
                ],
                bin_arm: MOpData { rs1, rs2 } => {
                    vregs.remove_use(rs1, op.clone());
                    vregs.remove_use(rs2, op);
                },
                un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
                un_arm: MOpData { rs } => {
                    vregs.remove_use(rs, op);
                },
                fallback: {
                    MOpData::Slti { rs1, imm, .. }
                    | MOpData::Sltiu { rs1, imm, .. }
                    | MOpData::Addiw { rs1, imm, .. }
                    | MOpData::Subiw { rs1, imm, .. }
                    | MOpData::Muliw { rs1, imm, .. }
                    | MOpData::Diviw { rs1, imm, .. }
                    | MOpData::Remiw { rs1, imm, .. }
                    | MOpData::Slliw { rs1, imm, .. }
                    | MOpData::Srliw { rs1, imm, .. }
                    | MOpData::Sraiw { rs1, imm, .. }
                    | MOpData::Xori { rs1, imm, .. } => {
                        vregs.remove_use(rs1, op.clone());
                        vregs.remove_use(imm, op);
                    }
                    MOpData::Lw { base, offset, .. }
                    | MOpData::Flw { base, offset, .. }
                    | MOpData::Ld { base, offset, .. } => {
                        vregs.remove_use(base, op.clone());
                        vregs.remove_use(offset, op);
                    }
                    MOpData::Sw { rs, base, offset }
                    | MOpData::Fsw { rs, base, offset }
                    | MOpData::Sd { rs, base, offset } => {
                        vregs.remove_use(rs, op.clone());
                        vregs.remove_use(base, op.clone());
                        vregs.remove_use(offset, op);
                    }
                    MOpData::Li { .. } => {}
                    MOpData::La { .. } => {}
                    MOpData::J { .. } => {}
                    MOpData::Bnez { rs, .. } => {
                        vregs.remove_use(rs, op.clone());
                    }
                    MOpData::Call { .. } => {}
                    MOpData::Ret => {}
                }
            },
        }
    }

    pub fn remove_def(&mut self, current_function: Option<BOperand>, op: BOperand) {
        let dfg = self.dfg_mut_or_panic(
            current_function.clone(),
            "BackIR remove_def: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        let vregs = &mut self.funcs[current_function.unwrap()].vregs;
        match data {
            BOpData::L(lop_data) => match_rd! {
                target: lop_data,
                op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, LoadFloatImm, LoadIntImm, Move],
                rd_arm: LOpData(rd) => {
                    vregs.remove_def(rd, op);
                },
                fallback: {
                    // For other LOpData which doesn't have rd field (e.g. Call and Store), we return Undef.
                    LOpData::Store {..}
                    | LOpData::Call {..}
                    | LOpData::Br {..}
                    | LOpData::Jump {..}
                    | LOpData::Ret => {},
                }
            },
            BOpData::M(mop_data) => match_rd! {
                target: mop_data,
                op_with_rds: [
                    Li, La, Mv, FmvS,
                    Addw, Subw, Mulw, Divw, Remw,
                    Slliw, Srliw, Sraiw,
                    Sllw, Srlw, Sraw,
                    Slt, Slti, Sltu, Sltiu,
                    Addiw, Subiw, Muliw, Diviw, Remiw,
                    Xor, Xori,
                    FaddS, FsubS, FmulS, FdivS,
                    FeqS, FltS, FleS, FneS, FgtS, FgeS,
                    FcvtWS, FcvtSW, FmvWX, FmvXW,
                    Lw, Flw, Ld
                ],
                rd_arm: MOpData(rd) => {
                    vregs.remove_def(rd, op);
                },
                fallback: {
                    // For other MOpData which doesn't have rd field (e.g. J and Call), we return Undef.
                    | MOpData::Sw {..}
                    | MOpData::Fsw {..}
                    | MOpData::Sd {..}
                    | MOpData::J {..}
                    | MOpData::Bnez {..}
                    | MOpData::Call {..}
                    | MOpData::Ret
                    | MOpData::Beq {..}
                    | MOpData::Bne {..}
                    | MOpData::Blt {..}
                    | MOpData::Bge {..}
                    | MOpData::Bltu {..}
                    | MOpData::Bgeu {..} => {},
                }
            },
        }
    }

    pub fn replace_all_uses(
        &mut self,
        current_function: Option<BOperand>,
        old: BOperand,
        new: BOperand,
    ) {
        let vregs = &mut self.funcs[current_function.clone().unwrap()].vregs;
        let uses = vregs[old.clone()].uses.clone();

        for use_op in uses {
            let op_id = match_minor! {
                target: use_op,
                minor_arms: {
                    BOperand::Inst(op_id) => op_id,
                },
                uni_ops: [
                    BOperand::Reg,
                    BOperand::IntImm,
                    BOperand::FloatImm,
                    BOperand::Func,
                    BOperand::Slot,
                    BOperand::Data,
                    BOperand::RoData,
                    BOperand::BB,
                    BOperand::Undef
                ],
                other_patterns: [],
                uni_arm: return
            };

            let dfg = self.dfg_mut_or_panic(
                current_function.clone(),
                "BackIR replace_all_uses: no current function",
            );
            let op = &mut dfg[op_id];
            match &mut op.data {
                BOpData::L(data) => match_ops! {
                    target: data,
                    bin_ops: [
                        AddI, SubI, MulI, DivI, ModI,
                        SNe, SEq, SGt, SLt, SGe, SLe,
                        Xor, Shl, Shr, Sar,
                        AddF, SubF, MulF, DivF,
                        ONe, OEq, OGt, OLt, OGe, OLe
                    ],
                    bin_arm: LOpData { lhs, rhs } => {
                        if *lhs == old {
                            *lhs = new.clone();
                        }
                        if *rhs == old {
                            *rhs = new.clone();
                        }
                    },
                    un_ops: [Sitofp, Fptosi],
                    un_arm: LOpData { value } => {
                        if *value == old {
                            *value = new.clone();
                        }
                    },
                    fallback: {
                        LOpData::Store { addr, value } => {
                            if *addr == old {
                                *addr = new.clone();
                            }
                            if *value == old {
                                *value = new.clone();
                            }
                        }
                        LOpData::Load { addr, .. } => {
                            if *addr == old {
                                *addr = new.clone();
                            }
                        }
                        LOpData::Move { src, .. } => {
                            if *src == old {
                                *src = new.clone();
                            }
                        }
                        LOpData::Br { cond, .. } => {
                            if *cond == old {
                                *cond = new.clone();
                            }
                        }
                        LOpData::Call { .. }
                        | LOpData::Jump { .. }
                        | LOpData::Ret
                        | LOpData::LoadIntImm { .. }
                        | LOpData::LoadFloatImm { .. } => {}
                    }
                },
                BOpData::M(data) => match_ops! {
                    target: data,
                    bin_ops: [
                        Addw, Subw, Mulw, Divw, Remw,
                        Sllw, Srlw, Sraw,
                        Slt, Sltu, Xor,
                        FaddS, FsubS, FmulS, FdivS,
                        FeqS, FltS, FleS, FneS, FgtS, FgeS,
                        Beq, Bne, Blt, Bge, Bltu, Bgeu
                    ],
                    bin_arm: MOpData { rs1, rs2 } => {
                        if *rs1 == old {
                            *rs1 = new.clone();
                        }
                        if *rs2 == old {
                            *rs2 = new.clone();
                        }
                    },
                    un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
                    un_arm: MOpData { rs } => {
                        if *rs == old {
                            *rs = new.clone();
                        }
                    },
                    fallback: {
                        MOpData::Slti { rs1, imm, .. }
                        | MOpData::Sltiu { rs1, imm, .. }
                        | MOpData::Addiw { rs1, imm, .. }
                        | MOpData::Subiw { rs1, imm, .. }
                        | MOpData::Muliw { rs1, imm, .. }
                        | MOpData::Diviw { rs1, imm, .. }
                        | MOpData::Remiw { rs1, imm, .. }
                        | MOpData::Slliw { rs1, imm, .. }
                        | MOpData::Srliw { rs1, imm, .. }
                        | MOpData::Sraiw { rs1, imm, .. }
                        | MOpData::Xori { rs1, imm, .. } => {
                            if *rs1 == old {
                                *rs1 = new.clone();
                            }
                            if *imm == old {
                                *imm = new.clone();
                            }
                        }
                        MOpData::Lw { base, offset, .. }
                        | MOpData::Flw { base, offset, .. }
                        | MOpData::Ld { base, offset, .. } => {
                            if *base == old {
                                *base = new.clone();
                            }
                            if *offset == old {
                                *offset = new.clone();
                            }
                        }
                        MOpData::Sw { rs, base, offset }
                        | MOpData::Fsw { rs, base, offset }
                        | MOpData::Sd { rs, base, offset } => {
                            if *rs == old {
                                *rs = new.clone();
                            }
                            if *base == old {
                                *base = new.clone();
                            }
                            if *offset == old {
                                *offset = new.clone();
                            }
                        }
                        MOpData::Li { .. } => {}
                        MOpData::La { .. } => {}
                        MOpData::J { .. } => {}
                        MOpData::Bnez { rs, .. } => {
                            if *rs == old {
                                *rs = new.clone();
                            }
                        }
                        MOpData::Call { .. } => {}
                        MOpData::Ret => {}
                    }
                },
            }

            let vregs = &mut self.funcs[current_function.clone().unwrap()].vregs;
            vregs.remove_use(old.clone(), use_op.clone());
            vregs.add_use(new.clone(), use_op);
        }
    }

    pub fn add_control_flow(
        &mut self,
        current_function: Option<BOperand>,
        op: BOperand,
        bb: BOperand,
    ) {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "BackIR add_control_flow: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        match data {
            BOpData::L(data) => {
                match_minor! {
                    target: data,
                    minor_arms: {
                        LOpData::Br {
                            then_bb, else_bb, ..
                        } => {
                            cfg.add_pred(then_bb.clone(), bb.clone());
                            cfg.add_succ(bb.clone(), then_bb);
                            cfg.add_pred(else_bb.clone(), bb.clone());
                            cfg.add_succ(bb, else_bb);
                        }
                        LOpData::Jump { target_bb } => {
                            cfg.add_pred(target_bb.clone(), bb.clone());
                            cfg.add_succ(bb, target_bb);
                        }
                    },
                    uni_ops: [
                        LOpData::AddI,
                        LOpData::SubI,
                        LOpData::MulI,
                        LOpData::DivI,
                        LOpData::ModI,
                        LOpData::SNe,
                        LOpData::SEq,
                        LOpData::SGt,
                        LOpData::SLt,
                        LOpData::SGe,
                        LOpData::SLe,
                        LOpData::Xor,
                        LOpData::Shl,
                        LOpData::Shr,
                        LOpData::Sar,
                        LOpData::AddF,
                        LOpData::SubF,
                        LOpData::MulF,
                        LOpData::DivF,
                        LOpData::ONe,
                        LOpData::OEq,
                        LOpData::OGt,
                        LOpData::OLt,
                        LOpData::OGe,
                        LOpData::OLe,
                        LOpData::Sitofp,
                        LOpData::Fptosi,
                        LOpData::Store,
                        LOpData::Load,
                        LOpData::Move,
                        LOpData::Call,
                        LOpData::LoadIntImm,
                        LOpData::LoadFloatImm
                    ],
                    other_patterns: [LOpData::Ret],
                    uni_arm: {}
                }
            }
            BOpData::M(data) => {
                match_minor! {
                    target: data,
                    minor_arms: {
                        MOpData::J { target } => {
                            cfg.add_pred(target.clone(), bb.clone());
                            cfg.add_succ(bb, target);
                        }
                        MOpData::Bnez { target, .. } => {
                            cfg.add_pred(target.clone(), bb.clone());
                            cfg.add_succ(bb, target);
                        }
                        MOpData::Beq { offset, .. }
                        | MOpData::Bne { offset, .. }
                        | MOpData::Blt { offset, .. }
                        | MOpData::Bge { offset, .. }
                        | MOpData::Bltu { offset, .. }
                        | MOpData::Bgeu { offset, .. } => {
                            cfg.add_pred(offset.clone(), bb.clone());
                            cfg.add_succ(bb, offset);
                        }
                    },
                    uni_ops: [
                        MOpData::Li,
                        MOpData::La,
                        MOpData::Mv,
                        MOpData::FmvS,
                        MOpData::Addw,
                        MOpData::Subw,
                        MOpData::Mulw,
                        MOpData::Divw,
                        MOpData::Remw,
                        MOpData::Addiw,
                        MOpData::Subiw,
                        MOpData::Muliw,
                        MOpData::Diviw,
                        MOpData::Remiw,
                        MOpData::Slliw,
                        MOpData::Srliw,
                        MOpData::Sraiw,
                        MOpData::Sllw,
                        MOpData::Srlw,
                        MOpData::Sraw,
                        MOpData::Slt,
                        MOpData::Slti,
                        MOpData::Sltu,
                        MOpData::Sltiu,
                        MOpData::Xor,
                        MOpData::Xori,
                        MOpData::FaddS,
                        MOpData::FsubS,
                        MOpData::FmulS,
                        MOpData::FdivS,
                        MOpData::FeqS,
                        MOpData::FltS,
                        MOpData::FleS,
                        MOpData::FneS,
                        MOpData::FgtS,
                        MOpData::FgeS,
                        MOpData::FcvtWS,
                        MOpData::FcvtSW,
                        MOpData::FmvWX,
                        MOpData::FmvXW,
                        MOpData::Lw,
                        MOpData::Sw,
                        MOpData::Flw,
                        MOpData::Fsw,
                        MOpData::Ld,
                        MOpData::Sd,
                        MOpData::Call,
                        MOpData::Ret
                    ],
                    other_patterns: [],
                    uni_arm: {}
                }
            }
        }
    }

    pub fn remove_control_flow(
        &mut self,
        current_function: Option<BOperand>,
        op: BOperand,
        bb: BOperand,
    ) {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function,
            "BackIR remove_control_flow: no current function",
        );
        let data = dfg[op.get_inst_id()].data.clone();

        match data {
            BOpData::L(data) => {
                match_minor! {
                    target: data,
                    minor_arms: {
                        LOpData::Br {
                            then_bb, else_bb, ..
                        } => {
                            cfg.remove_pred(then_bb.clone(), bb.clone());
                            cfg.remove_succ(bb.clone(), then_bb);
                            cfg.remove_pred(else_bb.clone(), bb.clone());
                            cfg.remove_succ(bb, else_bb);
                        }
                        LOpData::Jump { target_bb } => {
                            cfg.remove_pred(target_bb.clone(), bb.clone());
                            cfg.remove_succ(bb, target_bb);
                        }
                    },
                    uni_ops: [
                        LOpData::AddI,
                        LOpData::SubI,
                        LOpData::MulI,
                        LOpData::DivI,
                        LOpData::ModI,
                        LOpData::SNe,
                        LOpData::SEq,
                        LOpData::SGt,
                        LOpData::SLt,
                        LOpData::SGe,
                        LOpData::SLe,
                        LOpData::Xor,
                        LOpData::Shl,
                        LOpData::Shr,
                        LOpData::Sar,
                        LOpData::AddF,
                        LOpData::SubF,
                        LOpData::MulF,
                        LOpData::DivF,
                        LOpData::ONe,
                        LOpData::OEq,
                        LOpData::OGt,
                        LOpData::OLt,
                        LOpData::OGe,
                        LOpData::OLe,
                        LOpData::Sitofp,
                        LOpData::Fptosi,
                        LOpData::Store,
                        LOpData::Load,
                        LOpData::Move,
                        LOpData::Call,
                        LOpData::LoadIntImm,
                        LOpData::LoadFloatImm,
                        LOpData::Ret
                    ],
                    other_patterns: [],
                    uni_arm: {}
                }
            }
            BOpData::M(data) => {
                match_minor! {
                    target: data,
                    minor_arms: {
                        MOpData::J { target } => {
                            cfg.remove_pred(target.clone(), bb.clone());
                            cfg.remove_succ(bb, target);
                        }
                        MOpData::Bnez { target, .. } => {
                            cfg.remove_pred(target.clone(), bb.clone());
                            cfg.remove_succ(bb, target);
                        }
                        MOpData::Beq { offset, .. }
                        | MOpData::Bne { offset, .. }
                        | MOpData::Blt { offset, .. }
                        | MOpData::Bge { offset, .. }
                        | MOpData::Bltu { offset, .. }
                        | MOpData::Bgeu { offset, .. } => {
                            cfg.remove_pred(offset.clone(), bb.clone());
                            cfg.remove_succ(bb, offset);
                        }
                    },
                    uni_ops: [
                        MOpData::Li,
                        MOpData::La,
                        MOpData::Mv,
                        MOpData::FmvS,
                        MOpData::Addw,
                        MOpData::Subw,
                        MOpData::Mulw,
                        MOpData::Divw,
                        MOpData::Remw,
                        MOpData::Addiw,
                        MOpData::Subiw,
                        MOpData::Muliw,
                        MOpData::Diviw,
                        MOpData::Remiw,
                        MOpData::Slliw,
                        MOpData::Srliw,
                        MOpData::Sraiw,
                        MOpData::Sllw,
                        MOpData::Srlw,
                        MOpData::Sraw,
                        MOpData::Slt,
                        MOpData::Slti,
                        MOpData::Sltu,
                        MOpData::Sltiu,
                        MOpData::Xor,
                        MOpData::Xori,
                        MOpData::FaddS,
                        MOpData::FsubS,
                        MOpData::FmulS,
                        MOpData::FdivS,
                        MOpData::FeqS,
                        MOpData::FltS,
                        MOpData::FleS,
                        MOpData::FneS,
                        MOpData::FgtS,
                        MOpData::FgeS,
                        MOpData::FcvtWS,
                        MOpData::FcvtSW,
                        MOpData::FmvWX,
                        MOpData::FmvXW,
                        MOpData::Lw,
                        MOpData::Sw,
                        MOpData::Flw,
                        MOpData::Fsw,
                        MOpData::Ld,
                        MOpData::Sd,
                        MOpData::Call,
                        MOpData::Ret
                    ],
                    other_patterns: [],
                    uni_arm: {}
                }
            }
        }
    }

    pub fn create(
        &mut self,
        builder: &BBuilder,
        current_function: Option<BOperand>,
        op: BOp,
    ) -> BOperand {
        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function.clone(),
            "BackIR create: no current function",
        );

        let new_id = dfg.alloc(op);
        let current_block = if let Some(block) = &builder.current_block {
            block.get_bb_id()
        } else {
            panic!("BackIR create: current_block is None");
        };
        let bb = &mut cfg[current_block];

        let op_id = if let Some(current_inst) = &builder.current_inst {
            let pos = bb
                .cur
                .iter()
                .position(|id| id.get_inst_id() == current_inst.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "BackIR create: current_inst {:?} not found in current_block {:?}",
                        current_inst, builder.current_block
                    )
                });
            let op_id = BOperand::Inst(new_id);
            bb.cur.insert(pos, op_id.clone());
            op_id
        } else {
            let op_id = BOperand::Inst(new_id);
            bb.cur.push(op_id.clone());
            op_id
        };

        self.bind(current_function.clone(), op_id.clone());
        self.add_uses(current_function.clone(), op_id.clone());
        let current_block = builder
            .current_block
            .clone()
            .unwrap_or_else(|| panic!("BackIR create: current_block is None"));
        self.add_control_flow(current_function, op_id.clone(), current_block);
        op_id
    }

    /// Bind the operation with its rd.
    /// If rd is BOperand::Undef, it means we need to create a new virtual register and bind the operation with it.
    /// Else if rd is BOperand::Reg, we do nothing for it.
    /// Else panic and report invalid rd.
    pub fn bind(&mut self, current_function: Option<BOperand>, op: BOperand) {
        let func = &mut self.funcs[current_function.unwrap()];
        let data = &mut func.dfg[op.clone()].data;
        let vregs = &mut func.vregs;

        match data {
            BOpData::L(lop_data) => match_rd! {
                target: lop_data,
                op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, Move, LoadFloatImm, LoadIntImm],
                rd_arm: LOpData(rd) => {
                    match rd {
                        BOperand::Reg(_) => {/*do nothing*/}
                        BOperand::Undef => {
                            let new_vreg = vregs.alloc(VirtReg::default());
                            // Bind the new vreg with the operation.
                            *rd = BOperand::Reg(Reg::Virt(new_vreg));
                            // Bind the operation with the virt reg.
                            vregs.add_def(BOperand::Reg(Reg::Virt(new_vreg)), op.clone());
                        }
                        BOperand::Data(_)
                        | BOperand::RoData(_)
                        | BOperand::BB(_)
                        | BOperand::Slot(_)
                        | BOperand::IntImm(_)
                        | BOperand::FloatImm(_)
                        | BOperand::Func(_)
                        | BOperand::Inst(_) => unreachable!("Invalid rd operand {:?} in LOpData", rd),
                    }
                },
                fallback: {
                    // Only Move can be binded with vreg, since other LOp with rd field are not created for temp values.
                    LOpData::Br {..}
                    | LOpData::Jump {..}
                    | LOpData::Store {..}
                    | LOpData::Call {..}
                    | LOpData::Ret => {/*do nothing*/},
                }
            },

            BOpData::M(mop_data) => match_rd! {
                target: mop_data,
                op_with_rds: [
                    Li, La, Mv, FmvS,
                    Addw, Subw, Mulw, Divw, Remw,
                    Slliw, Srliw, Sraiw,
                    Sllw, Srlw, Sraw,
                    Slt, Slti, Sltu, Sltiu,
                    Addiw, Subiw, Muliw, Diviw, Remiw,
                    Xor, Xori,
                    FaddS, FsubS, FmulS, FdivS,
                    FeqS, FltS, FleS, FneS, FgtS, FgeS,
                    FcvtWS, FcvtSW, FmvWX, FmvXW,
                    Lw, Flw, Ld
                ],
                rd_arm: MOpData(rd) => {
                    match rd {
                        BOperand::Reg(_) => {/*do nothing*/}
                        BOperand::Undef => {
                            let new_vreg = vregs.alloc(VirtReg::default());
                            // Bind the new vreg with the operation.
                            *rd = BOperand::Reg(Reg::Virt(new_vreg));
                            // Bind the operation with the virt reg.
                            vregs.add_def(BOperand::Reg(Reg::Virt(new_vreg)), op.clone());
                        }
                        BOperand::Data(_)
                        | BOperand::RoData(_)
                        | BOperand::BB(_)
                        | BOperand::Slot(_)
                        | BOperand::IntImm(_)
                        | BOperand::FloatImm(_)
                        | BOperand::Func(_)
                        | BOperand::Inst(_) => unreachable!("Invalid rd operand {:?} in MOpData", rd),
                    }
                },
                fallback: {
                    // For other MOpData which doesn't have rd field, we return Undef.
                    MOpData::Sw { .. }
                    | MOpData::Fsw { .. }
                    | MOpData::Sd { .. }
                    | MOpData::J { .. }
                    | MOpData::Bnez { .. }
                    | MOpData::Call { .. }
                    | MOpData::Ret
                    | MOpData::Beq { .. }
                    | MOpData::Bne { .. }
                    | MOpData::Blt { .. }
                    | MOpData::Bge { .. }
                    | MOpData::Bltu { .. }
                    | MOpData::Bgeu { .. } => {/*do nothing*/},
                }
            },
        };
    }

    pub fn create_at_head(
        &mut self,
        builder: &mut BBuilder,
        current_function: Option<BOperand>,
        op: BOp,
    ) -> BOperand {
        let bb_id = match &builder.current_block {
            Some(block) => block.get_bb_id(),
            None => panic!("BackIR create_at_head: current_block is None"),
        };

        let inst_id = {
            let cfg = self.cfg_mut_or_panic(
                current_function.clone(),
                "BackIR create_at_head: no current function",
            );
            let bb = &cfg[bb_id];
            if bb.cur.is_empty() {
                None
            } else {
                Some(bb.cur[0].clone())
            }
        };

        builder.set_before_inst(self, current_function.clone(), inst_id);
        self.create(builder, current_function, op)
    }

    pub fn create_new_block(&mut self, current_function: Option<BOperand>) -> BOperand {
        let cfg = self.cfg_mut_or_panic(
            current_function.clone(),
            "BackIR create_new_block: no current function",
        );
        let bb_id = cfg.alloc(super::BBasicBlock::default());
        BOperand::BB(bb_id)
    }

    pub fn remove_op(
        &mut self,
        current_function: Option<BOperand>,
        op: BOperand,
        bb: Option<BOperand>,
    ) -> BOp {
        self.remove_def(current_function.clone(), op.clone());
        self.remove_uses(current_function.clone(), op.clone());
        if let Some(bb_id) = bb.clone() {
            self.remove_control_flow(current_function.clone(), op.clone(), bb_id);
        }

        let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
            current_function.clone(),
            "BackIR remove_op: no current function",
        );

        let op_id = op.get_inst_id();
        let bb_id = bb
            .unwrap_or_else(|| {
                panic!(
                    "BackIR remove_op: bb is None when removing instruction {:?}",
                    op
                )
            })
            .get_bb_id();
        let bb = &mut cfg[bb_id];

        if let Some(pos) = bb.cur.iter().position(|id| id.get_inst_id() == op_id) {
            bb.cur.remove(pos);
        } else {
            panic!(
                "BackIR remove_op: instruction {:?} not found in block {:?}",
                op, bb_id
            );
        }

        let removed_op = match std::mem::replace(&mut dfg.storage[op_id], ArenaItem::None) {
            ArenaItem::Data(data) => data,
            _ => panic!("BackIR remove_op: dfg slot {} is not data", op_id),
        };

        // We don't check whether the old vreg's uses are all removed, since the vreg might be defined my multiple operations.
        // TODO: The vreg will be automatically removed if it has not uses in vregs.gc(), at which point the vregs will check the uses of vreg.
        removed_op
    }

    pub fn replace_op(
        &mut self,
        builder: &mut BBuilder,
        current_function: Option<BOperand>,
        op_id: BOperand,
        bb_id: BOperand,
        new_op: BOp,
    ) -> BOperand {
        let pos = {
            let cfg = self.cfg_mut_or_panic(
                current_function.clone(),
                "BackIR replace_op: no current function",
            );
            let bb = &cfg[bb_id.clone()];
            bb.cur
                .iter()
                .position(|id| id.get_inst_id() == op_id.get_inst_id())
                .unwrap_or_else(|| {
                    panic!(
                        "BackIR replace_op: instruction {:?} not found in block {:?}",
                        op_id, bb_id
                    )
                })
        };

        let next_inst = {
            let cfg = self.cfg_mut_or_panic(
                current_function.clone(),
                "BackIR replace_op: no current function",
            );
            let bb = &cfg[bb_id.get_bb_id()];
            bb.cur.get(pos + 1).cloned()
        };

        {
            let mut guard = BBuilderGuard::new(builder);
            guard.set_current_block(bb_id.clone());
            // We won't bind the new operation with the old vreg. We create a new one directly.
            guard.set_before_inst(self, current_function.clone(), next_inst);
            let new_op_id = self.create(&guard, current_function.clone(), new_op);
            // RAUW
            self.replace_all_uses(current_function.clone(), op_id.clone(), new_op_id.clone());
            // Remove the old operation.
            self.remove_op(current_function, op_id, Some(bb_id));
            new_op_id
        }
    }

    pub fn move_op_to_bb_at(
        &mut self,
        current_function: Option<BOperand>,
        op: BOperand,
        old_bb: BOperand,
        new_bb: BOperand,
        pos: Option<BOperand>,
    ) {
        let cfg = self.cfg_mut_or_panic(
            current_function,
            "BackIR move_op_to_bb_at: no current function",
        );

        let op_id = op.get_inst_id();
        let old_bb_id = old_bb.get_bb_id();

        let old_bb_ref = &mut cfg[old_bb_id];
        if let Some(pos) = old_bb_ref
            .cur
            .iter()
            .position(|id| id.get_inst_id() == op_id)
        {
            old_bb_ref.cur.remove(pos);
        } else {
            panic!(
                "BackIR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
                op, old_bb
            );
        }

        let new_bb_id = new_bb.get_bb_id();
        let new_bb_ref = &mut cfg[new_bb_id];
        if let Some(pos) = pos {
            let pos_id = pos.get_inst_id();
            if let Some(pos) = new_bb_ref
                .cur
                .iter()
                .position(|id| id.get_inst_id() == pos_id)
            {
                new_bb_ref.cur.insert(pos, op);
            } else {
                panic!(
                    "BackIR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
                    pos, new_bb
                );
            }
        } else {
            new_bb_ref.cur.push(op);
        }
    }

    pub fn get_rd(&self, current_function: Option<BOperand>, lop_id: BOperand) -> Option<BOperand> {
        let current_function = current_function.expect("No current function");
        let bop = &self.funcs[current_function].dfg[lop_id.clone()];

        match &bop.data {
            BOpData::L(lop_data) => match_rd! {
                target: lop_data,
                op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, LoadFloatImm, LoadIntImm, Move],
                rd_arm: LOpData(rd) => {
                    Some(rd.clone())
                },
                fallback: {
                    // For other LOpData which doesn't have rd field (e.g. Call and Store), we return Undef.
                    LOpData::Store {..}
                    | LOpData::Call {..}
                    | LOpData::Br {..}
                    | LOpData::Jump {..}
                    | LOpData::Ret => None,
                }
            },
            BOpData::M(mop_data) => match_rd! {
                target: mop_data,
                op_with_rds: [
                    Li, La, Mv, FmvS,
                    Addw, Subw, Mulw, Divw, Remw,
                    Slliw, Srliw, Sraiw,
                    Sllw, Srlw, Sraw,
                    Slt, Slti, Sltu, Sltiu,
                    Addiw, Subiw, Muliw, Diviw, Remiw,
                    Xor, Xori,
                    FaddS, FsubS, FmulS, FdivS,
                    FeqS, FltS, FleS, FneS, FgtS, FgeS,
                    FcvtWS, FcvtSW, FmvWX, FmvXW,
                    Lw, Flw, Ld
                ],
                rd_arm: MOpData(rd) => {
                    Some(rd.clone())
                },
                fallback: {
                    // For other MOpData which doesn't have rd field (e.g. J and Call), we return Undef.
                    MOpData::Sw {..}
                    | MOpData::Fsw {..}
                    | MOpData::Sd {..}
                    | MOpData::J {..}
                    | MOpData::Bnez {..}
                    | MOpData::Call {..}
                    | MOpData::Ret
                    | MOpData::Beq {..}
                    | MOpData::Bne {..}
                    | MOpData::Blt {..}
                    | MOpData::Bge {..}
                    | MOpData::Bltu {..}
                    | MOpData::Bgeu {..} => None,
                }
            },
        }
    }
}
