//! Dead Code Elimation in BackIR.

use yachiyo::ir::back::{BBuilder, BFunction, BOpData, BOperand, BackIR, LOpData, MOpData, Reg};
use yachiyo::pass::BPass;
use yachiyo::utils::arena::ArenaItem;
use yachiyo::utils::r#match::match_src;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct BDCE<'a> {
    ir: Option<&'a mut BackIR>,
    builder: BBuilder,
    // Worklist of inst
    worklist: Vec<(BOperand, BOperand)>,
    // Mapping from op_id to bb_id
    op_to_bb: Vec<BOperand>,
}

impl<'a> BDCE<'a> {
    #[inline(always)]
    fn current_func(&self) -> BOperand {
        self.builder
            .current_function
            .expect("BDCE: not in a function")
    }

    #[inline(always)]
    pub fn get_func(&self, func_id: BOperand) -> &BFunction {
        let ir = self.ir.as_ref().expect("BDCE: ir is not mounted");
        &ir.funcs[func_id]
    }

    #[inline(always)]
    pub fn get_rd(&self, op_id: BOperand) -> Option<BOperand> {
        let ir = self.ir.as_ref().expect("BDCE: ir is not mounted");
        ir.get_rd(self.builder.current_function, op_id)
    }

    pub fn is_dead(&self, operand: BOperand) -> bool {
        let current_func = self.get_func(self.current_func());
        let vregs = &current_func.vregs;

        match operand {
            BOperand::Inst(_) => {
                let Some(rd) = self.get_rd(operand) else {
                    return false;
                };

                match rd {
                    BOperand::Reg(Reg::Virt(_)) => vregs[rd].uses.is_empty(),
                    BOperand::Reg(Reg::X(_) | Reg::F(_))
                    | BOperand::Undef
                    | BOperand::BB(_)
                    | BOperand::IntImm(_)
                    | BOperand::FloatImm(_)
                    | BOperand::Inst(_)
                    | BOperand::Func(_)
                    | BOperand::Data(_)
                    | BOperand::RoData(_)
                    | BOperand::Extern(_)
                    | BOperand::Slot(_) => false,
                }
            }
            BOperand::Reg(Reg::Virt(_)) => vregs[operand].uses.is_empty(),
            BOperand::Reg(Reg::X(_) | Reg::F(_))
            | BOperand::Undef
            | BOperand::BB(_)
            | BOperand::IntImm(_)
            | BOperand::FloatImm(_)
            | BOperand::Func(_)
            | BOperand::Data(_)
            | BOperand::RoData(_)
            | BOperand::Extern(_)
            | BOperand::Slot(_) => false,
        }
    }

    pub fn init(&mut self, func_id: BOperand) {
        self.builder.set_current_func(func_id);
        let ir = self.ir.as_ref().expect("BDCE: ir is not mounted");
        let func = &ir.funcs[func_id];
        self.worklist.clear();

        // map OpId to BBId
        self.op_to_bb.clear();
        self.op_to_bb
            .resize(func.dfg.storage.len(), BOperand::Undef);
        func.cfg
            .storage
            .iter()
            .enumerate()
            .for_each(|(bb_id, item)| {
                if let ArenaItem::Data(bb) = item {
                    for op_id in bb.cur.iter() {
                        self.op_to_bb[op_id.get_inst_id()] = BOperand::BB(bb_id);
                    }
                }
            });

        // Initialize the worklist
        for block_id in func.cfg.collect() {
            let block = &func.cfg[block_id];
            for inst_id in block.cur.iter() {
                let is_impure = {
                    let inst = &func.dfg[*inst_id];
                    inst.data.is_impure()
                };
                if self.is_dead(*inst_id) && !is_impure {
                    self.worklist.push((*inst_id, BOperand::BB(block_id)));
                }
            }
        }
    }
}

impl<'a> BPass<'a> for BDCE<'a> {
    fn name(&self) -> &str {
        "BDCE"
    }
    fn mount(&mut self, program: &'a mut BackIR) {
        self.ir = Some(program);
    }

    fn run(&mut self) {
        fn check(this: &mut BDCE<'_>, operand: BOperand) {
            if !this.is_dead(operand) {
                return;
            }

            match operand {
                BOperand::Inst(op_id) => {
                    let op = BOperand::Inst(op_id);
                    let bb_id = match this.op_to_bb.get(op_id).copied() {
                        Some(BOperand::BB(bb)) => BOperand::BB(bb),
                        _ => unreachable!(),
                    };

                    let should_push = {
                        let func = this.get_func(this.current_func());
                        !func.dfg[op].data.is_impure()
                    };

                    if should_push {
                        this.worklist.push((op, bb_id));
                    }
                }
                BOperand::Reg(Reg::Virt(_)) => {
                    let defs = {
                        let func = this.get_func(this.current_func());
                        func.vregs[operand].defs.clone()
                    };

                    for def in defs {
                        let def_id = def.get_inst_id();
                        let bb_id = match this.op_to_bb.get(def_id).copied() {
                            Some(BOperand::BB(bb)) => BOperand::BB(bb),
                            _ => continue,
                        };

                        let should_push = {
                            let func = this.get_func(this.current_func());
                            !func.dfg[def].data.is_impure()
                        };

                        if should_push {
                            this.worklist.push((def, bb_id));
                        }
                    }
                }
                BOperand::Reg(Reg::X(_) | Reg::F(_))
                | BOperand::Undef
                | BOperand::BB(_)
                | BOperand::IntImm(_)
                | BOperand::FloatImm(_)
                | BOperand::Func(_)
                | BOperand::Data(_)
                | BOperand::RoData(_)
                | BOperand::Extern(_)
                | BOperand::Slot(_) => {}
            }
        }

        let func_ids = self
            .ir
            .as_ref()
            .expect("BDCE: ir is not mounted")
            .funcs
            .ids();

        for func_id in func_ids {
            self.init(BOperand::Func(func_id));
            while let Some((op_id, bb_id)) = self.worklist.pop() {
                self.builder.set_current_block(bb_id);

                let removed_op = self
                    .ir
                    .as_deref_mut()
                    .expect("BDCE: ir is not mounted")
                    .remove_op(self.builder.current_function, op_id, Some(bb_id));
                self.op_to_bb[op_id.get_inst_id()] = BOperand::Undef;

                // Check the operands of the removed instruction
                match removed_op.data {
                    BOpData::L(lop_data) => match_src! {
                        target: lop_data,
                        bin_ops: [
                            AddI, SubI, MulI, DivI, ModI,
                            SNe, SEq, SGt, SLt, SGe, SLe,
                            Xor, Shl, Shr, Sar,
                            AddF, SubF, MulF, DivF,
                            ONe, OEq, OGt, OLt, OGe, OLe
                        ],
                        bin_arm: LOpData { lhs, rhs } => {
                            check(self, lhs);
                            check(self, rhs);
                        },
                        un_ops: [Sitofp, Fptosi],
                        un_arm: LOpData { value } => {
                            check(self, value);
                        },
                        fallback: {
                            LOpData::Store { addr, value } => {
                                check(self, addr);
                                check(self, value);
                            }
                            LOpData::Load { addr, .. } => {
                                check(self, addr);
                            }
                            LOpData::Move { src, .. } => {
                                check(self, src);
                            }
                            LOpData::Br { cond, .. } => {
                                check(self, cond);
                            }
                            LOpData::Call { func } => {
                                check(self, func);
                            }
                            LOpData::Jump { .. }
                            | LOpData::Ret
                            | LOpData::LoadIntImm { .. }
                            | LOpData::LoadFloatImm { .. } => {}
                        }
                    },
                    BOpData::M(mop_data) => match_src! {
                        target: mop_data,
                        bin_ops: [
                            Addw, Subw, Mulw, Divw, Remw,
                            Sllw, Srlw, Sraw,
                            Slt, Sltu, Xor,
                            FaddS, FsubS, FmulS, FdivS,
                            FeqS, FltS, FleS, FneS, FgtS, FgeS
                        ],
                        bin_arm: MOpData { rs1, rs2 } => {
                            check(self, rs1);
                            check(self, rs2);
                        },
                        un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
                        un_arm: MOpData { rs } => {
                            check(self, rs);
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
                                check(self, rs1);
                                check(self, imm);
                            }
                            MOpData::Lw { base, offset, .. }
                            | MOpData::Flw { base, offset, .. }
                            | MOpData::Ld { base, offset, .. } => {
                                check(self, base);
                                check(self, offset);
                            }
                            MOpData::Sw { rs, base, offset }
                            | MOpData::Fsw { rs, base, offset }
                            | MOpData::Sd { rs, base, offset } => {
                                check(self, rs);
                                check(self, base);
                                check(self, offset);
                            }

                            MOpData::Beq { rs1, rs2, offset }
                            | MOpData::Bne { rs1, rs2, offset }
                            | MOpData::Blt { rs1, rs2, offset }
                            | MOpData::Bge { rs1, rs2, offset }
                            | MOpData::Bltu { rs1, rs2, offset }
                            | MOpData::Bgeu { rs1, rs2, offset } => {
                                check(self, rs1);
                                check(self, rs2);
                                check(self, offset);
                            }
                            MOpData::Bnez { rs, .. } => {
                                check(self, rs);
                            }
                            MOpData::J { target } => {
                                check(self, target);
                            }
                            MOpData::Li { .. }
                            | MOpData::La { .. }
                            | MOpData::Call { .. }
                            | MOpData::Ret => {}
                        }
                    },
                }
            }
        }
    }
}
