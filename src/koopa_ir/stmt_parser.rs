use crate::ast::{Exp, PrimaryExp, UnaryExp, UnaryOp, MulExp, AddExp, MulOp, AddOp, LOrExp, LOrOp, LAndExp, LAndOp, EqExp, EqOp, RelExp, RelOp};
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{DataFlowGraph, InstData, InstId};
use std::cell::RefMut;

pub enum ParseResult {
    InstId(InstId),
    Const(i32),
}

/// parse_unary_exp
pub fn parse_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, exp: &Exp) -> ParseResult {
    match exp {
        Exp::LOrExp { lor_exp } => return parse_lor_exp(inst_list, dfg, lor_exp),
    }
}

fn parse_lor_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, lor_exp: &LOrExp) -> ParseResult {
    match lor_exp {
        LOrExp::LAndExp { land_exp } => {
            parse_land_exp(inst_list, dfg, land_exp)
        }
        LOrExp::LOrExp { lor_exp, lor_op, land_exp } => {
            let left = parse_lor_exp(inst_list, dfg, lor_exp);
            let right = parse_land_exp(inst_list, dfg, land_exp);

            let koopa_op = match lor_op {
                LOrOp::Or => KoopaOpCode::OR,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_land_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, land_exp: &LAndExp) -> ParseResult {
    match land_exp {
        LAndExp::EqExp { eq_exp } => {
            parse_eq_exp(inst_list, dfg, eq_exp)
        }
        LAndExp::LAndExp { land_exp, land_op, eq_exp } => {
            let left = parse_land_exp(inst_list, dfg, land_exp);
            let right = parse_eq_exp(inst_list, dfg, eq_exp);

            let koopa_op = match land_op {
                LAndOp::And => KoopaOpCode::AND,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_eq_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, eq_exp: &EqExp) -> ParseResult {
    match eq_exp {
        EqExp::RelExp { rel_exp } => {
            parse_rel_exp(inst_list, dfg, rel_exp)
        }
        EqExp::EqExp { eq_exp, eq_op, rel_exp } => {
            let left = parse_eq_exp(inst_list, dfg, eq_exp);
            let right = parse_rel_exp(inst_list, dfg, rel_exp);

            let koopa_op = match eq_op {
                EqOp::Eq => KoopaOpCode::EQ,
                EqOp::Ne => KoopaOpCode::NE,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_rel_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, rel_exp: &RelExp) -> ParseResult {
    match rel_exp {
        RelExp::AddExp { add_exp } => {
            parse_add_exp(inst_list, dfg, add_exp)
        }
        RelExp::RelExp { rel_exp, rel_op, add_exp } => {
            let left = parse_rel_exp(inst_list, dfg, rel_exp);
            let right = parse_add_exp(inst_list, dfg, add_exp);

            let koopa_op = match rel_op {
                RelOp::Lt => KoopaOpCode::LT,
                RelOp::Gt => KoopaOpCode::GT,
                RelOp::Le => KoopaOpCode::LE,
                RelOp::Ge => KoopaOpCode::GE,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_mul_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, mul_exp: &MulExp) -> ParseResult {
    match mul_exp {
        MulExp::UnaryExp { unary_exp } => {
            parse_unary_exp(inst_list, dfg, unary_exp)
        }
        MulExp::MulExp { mul_exp, mul_op, unary_exp } => {
            let left = parse_mul_exp(inst_list, dfg, mul_exp);
            let right = parse_unary_exp(inst_list, dfg, unary_exp);

            let koopa_op = match mul_op {
                MulOp::Mul => KoopaOpCode::MUL,
                MulOp::Div => KoopaOpCode::DIV,
                MulOp::Mod => KoopaOpCode::MOD,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_add_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, add_exp: &AddExp) -> ParseResult {
    match add_exp {
        AddExp::MulExp { mul_exp } => {
            parse_mul_exp(inst_list, dfg, mul_exp)
        },

        AddExp::AddExp { add_exp, add_op, mul_exp } => {
            let left = parse_add_exp(inst_list, dfg, add_exp);
            let right = parse_mul_exp(inst_list, dfg, mul_exp);

            let koopa_op = match add_op {
                AddOp::Add => KoopaOpCode::ADD,
                AddOp::Sub => KoopaOpCode::SUB,
            };

            let inst_id = dfg.insert_inst(InstData::new(
                koopa_op,
                vec![
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(left),
                    crate::koopa_ir::koopa_ir::Operand::from_parse_result(right),
                ],
            ));

            inst_list.push(InstId::from(inst_id));
            ParseResult::InstId(inst_id)
        }
    }
}

fn parse_unary_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, unary_exp: &UnaryExp) -> ParseResult {
    match unary_exp {
        // handle primary expression
        UnaryExp::PrimaryExp { exp } => {
            parse_primary_exp(inst_list, dfg, exp)
        }

        // handle unary operation
        UnaryExp::UnaryExp {
            unary_op,
            unary_exp,
        } => {
            let parse_result = parse_unary_exp(inst_list, dfg, unary_exp);

            match unary_op {
                UnaryOp::Plus => parse_result,
                UnaryOp::Minus | UnaryOp::Not => {
                    let inst_id = dfg.insert_inst(InstData::new(
                        match unary_op {
                            UnaryOp::Minus => KoopaOpCode::SUB,
                            UnaryOp::Not => KoopaOpCode::EQ,
                            _ => unreachable!(),
                        },
                        vec![
                            crate::koopa_ir::koopa_ir::Operand::Const(0),
                            crate::koopa_ir::koopa_ir::Operand::from_parse_result(parse_result),
                        ],
                    ));
                    inst_list.push(InstId::from(inst_id));
                    ParseResult::InstId(inst_id)
                },
            }
        }
    }
}

fn parse_primary_exp(inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>, primary_exp: &PrimaryExp) -> ParseResult {
    match primary_exp {
        PrimaryExp::Number { value } => {
            ParseResult::Const(*value)
        }
        PrimaryExp::Exp { exp } => {
            parse_exp(inst_list, dfg, exp)
        }
    }
}
