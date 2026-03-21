//! Instruction Selection (ISel).
//! Translating Lower IR to Machine IR.

use yachiyo::ir::back::*;
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::{match_full_ops, match_ops};

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
        match_ops! {
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
                        LOpData::ONe { .. } => BOperand::IntImm((l != r) as i32),
                        LOpData::OEq { .. } => BOperand::IntImm((l == r) as i32),
                        LOpData::OGt { .. } => BOperand::IntImm((l > r) as i32),
                        LOpData::OLt { .. } => BOperand::IntImm((l < r) as i32),
                        LOpData::OGe { .. } => BOperand::IntImm((l >= r) as i32),
                        LOpData::OLe { .. } => BOperand::IntImm((l <= r) as i32),
                        _ => unreachable!(),
                    }
                } else if let (BOperand::FloatImm(l), BOperand::FloatImm(r)) = (lhs.clone(), rhs.clone()) {
                    match lop_data {
                        LOpData::AddF { .. } => BOperand::FloatImm(l + r),
                        LOpData::SubF { .. } => BOperand::FloatImm(l - r),
                        LOpData::MulF { .. } => BOperand::FloatImm(l * r),
                        LOpData::DivF { .. } => BOperand::FloatImm(l / r),
                        _ => unreachable!(),
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
                LOpData::Ret { .. } => {
                    unreachable!("Constant folding for non-binary/unary ops is not allowed here")
                }
            }
        }
    }

    fn cast(lop_data: LOpData) -> BOperand {
        match_ops! {
            target: &lop_data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: LOpData { lhs, rhs } => {
                unreachable!("Casting for binary ops is not allowed here")
            },
            un_ops: [Sitofp, Fptosi],
            un_arm: LOpData { value } => {
                match lop_data {
                    LOpData::Sitofp { .. } => {
                        if let BOperand::IntImm(i) = value {
                            BOperand::FloatImm(*i as f32)
                        } else {
                            panic!("Expected an integer immediate for Sitofp, but got {:?}", value);
                        }
                    }
                    LOpData::Fptosi { .. } => {
                        if let BOperand::FloatImm(f) = value {
                            BOperand::IntImm(*f as i32)
                        } else {
                            panic!("Expected a float immediate for Fptosi, but got {:?}", value);
                        }
                    }
                    _ => unreachable!(),
                }
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
                LOpData::Ret { .. } => {
                    unreachable!("Casting for non-unary ops is not allowed here")
                }
            }
        }
    }

    // ======== Atomic Operations ========

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

    fn select(&mut self, op_id: BOperand) {
        let func_id = self
            .builder
            .current_function
            .clone()
            .expect("ISel: not in a function");
        let func = &self.ir.as_ref().unwrap().funcs[func_id];
        let bop = &func.dfg[op_id.clone()];
        let (lop_data, typ) = (bop.data.clone().into(), bop.typ.clone());

        match_full_ops! {
            target: lop_data,
            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: LOpData { lhs, rhs, rd } => {
                match (lhs.is_literal(), rhs.is_literal()) {
                    // If both operands are literals, we can fold the operation at compile time.
                    (true, true) => {
                        let folded = Self::fold(lop_data);
                        self.ir.as_mut().unwrap().replace_all_uses(Some(func_id), rd, folded);
                    }
                    // If one of 'em is literal, we use XxxI operation and canonicalize the operands.
                    (true, false) | (false, true) => {
                        let op_id = self.ir.as_mut().unwrap().replace_op(&mut self.builder, Some(func_id), op_id, self.builder.current_block.clone().unwrap(), BOp::new(
                            typ,
                            vec![],
                            BOpData::M(MOpData { value: Self::cast(lop_data), rd: rd.clone() }),
                        ));
                    }
                    (false, false) => 
                }
            },
            un_ops: [Sitofp, Fptosi],
            un_arm: LOpData { value, rd } => {

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
                LOpData::Ret { .. } => {
                    // For non-binary/unary ops, we simply emit them as is.
                    self.builder.emit(op_id.clone(), lop_data.clone());
                }
            }
        }
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
