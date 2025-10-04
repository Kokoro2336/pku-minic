use crate::ast::ast::LVal;
use crate::ast::decl::{GLOBAL_CONST_TABLE, GLOBAL_VAR_TABLE};
use crate::ast::op::*;
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{DataFlowGraph, InstData, InstId};

use std::cell::RefMut;
use std::collections::HashMap;

pub enum ParseResult {
    InstId(InstId),
    Const(i32),
    None,
}

impl ParseResult {
    pub fn get_value(&self) -> i32 {
        match self {
            ParseResult::Const(v) => *v,
            _ => panic!("Not a constant value"),
        }
    }

    pub fn get_id(&self) -> InstId {
        match self {
            ParseResult::InstId(id) => *id,
            _ => panic!("Not an instruction ID"),
        }
    }
}

pub trait Expression {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult;

    fn parse_const_exp(&self) -> ParseResult;
}

pub trait Expression {
    pub fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult;

    pub fn parse_const_exp(&self) -> i32;
}

#[derive(Debug, Clone)]
pub enum Exp {
    LOrExp { lor_exp: Box<LOrExp> },
}

impl Expression for Exp {
    /// parse_unary_exp
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            Exp::LOrExp { lor_exp } => return lor_exp.parse_var_exp(inst_list, dfg),
        }
    }

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            Exp::LOrExp { lor_exp } => return lor_exp.parse_const_exp(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LOrExp {
    LAndExp {
        land_exp: Box<LAndExp>,
    },
    LOrExp {
        lor_exp: Box<LOrExp>,
        lor_op: LOrOp,
        land_exp: Box<LAndExp>,
    },
}

impl Expression for LOrExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            LOrExp::LAndExp { land_exp } => {
                return land_exp.parse_var_exp(inst_list, dfg);
            }
            LOrExp::LOrExp {
                lor_exp,
                lor_op,
                land_exp,
            } => {
                let left = lor_exp.parse_var_exp(inst_list, dfg);
                let right = land_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            LOrExp::LAndExp { land_exp } => {
                return land_exp.parse_const_exp();
            }
            LOrExp::LOrExp {
                lor_exp,
                lor_op: _,
                land_exp,
            } => {
                panic!("A const expression couldn't be a logical OR expression");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum LAndExp {
    EqExp {
        eq_exp: Box<EqExp>,
    },
    LAndExp {
        land_exp: Box<LAndExp>,
        land_op: LAndOp,
        eq_exp: Box<EqExp>,
    },
}

impl Expression for LAndExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            LAndExp::EqExp { eq_exp } => {
                return eq_exp.parse_var_exp(inst_list, dfg);
            }
            LAndExp::LAndExp {
                land_exp,
                land_op,
                eq_exp,
            } => {
                let left = land_exp.parse_var_exp(inst_list, dfg);
                let right = eq_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            LAndExp::EqExp { eq_exp } => {
                return eq_exp.parse_const_exp();
            }
            LAndExp::LAndExp {
                land_exp,
                land_op: _,
                eq_exp,
            } => {
                panic!("A const expression couldn't be a logical AND expression");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum EqExp {
    RelExp {
        rel_exp: Box<RelExp>,
    },
    EqExp {
        eq_exp: Box<EqExp>,
        eq_op: EqOp,
        rel_exp: Box<RelExp>,
    },
}

impl Expression for EqExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            EqExp::RelExp { rel_exp } => {
                return rel_exp.parse_var_exp(inst_list, dfg);
            }
            EqExp::EqExp {
                eq_exp,
                eq_op,
                rel_exp,
            } => {
                let left = eq_exp.parse_var_exp(inst_list, dfg);
                let right = rel_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            EqExp::RelExp { rel_exp } => {
                return rel_exp.parse_const_exp();
            }
            EqExp::EqExp {
                eq_exp,
                eq_op: _,
                rel_exp,
            } => {
                panic!("A const expression couldn't be an equality expression");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RelExp {
    AddExp {
        add_exp: Box<AddExp>,
    },
    RelExp {
        rel_exp: Box<RelExp>,
        rel_op: RelOp,
        add_exp: Box<AddExp>,
    },
}

impl Expression for RelExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            RelExp::AddExp { add_exp } => {
                return add_exp.parse_var_exp(inst_list, dfg);
            }
            RelExp::RelExp {
                rel_exp,
                rel_op,
                add_exp,
            } => {
                let left = rel_exp.parse_var_exp(inst_list, dfg);
                let right = add_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            RelExp::AddExp { add_exp } => {
                return add_exp.parse_const_exp();
            }
            RelExp::RelExp {
                rel_exp,
                rel_op: _,
                add_exp,
            } => {
                panic!("A const expression couldn't be a relational expression");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum UnaryExp {
    PrimaryExp {
        exp: Box<PrimaryExp>,
    },
    UnaryExp {
        unary_op: UnaryOp,
        unary_exp: Box<UnaryExp>,
    },
}

impl Expression for UnaryExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            // handle primary expression
            UnaryExp::PrimaryExp { exp } => {
                return exp.parse_var_exp(inst_list, dfg);
            }

            // handle unary operation
            UnaryExp::UnaryExp {
                unary_op,
                unary_exp,
            } => {
                let parse_result = unary_exp.parse_var_exp(inst_list, dfg);

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
                    }
                }
            }
        }
    }

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            UnaryExp::PrimaryExp { exp } => {
                return exp.parse_const_exp();
            }
            UnaryExp::UnaryExp {
                unary_op,
                unary_exp,
            } => {
                let inner = unary_exp.parse_const_exp();
                match inner {
                    ParseResult::Const(v) => match unary_op {
                        UnaryOp::Plus => ParseResult::Const(v),
                        UnaryOp::Minus => ParseResult::Const(-v),
                        UnaryOp::Not => panic!("A const expression couldn't be a NOT expression"),
                    },
                    _ => panic!("Non-constant in const expression"),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MulExp {
    UnaryExp {
        unary_exp: Box<UnaryExp>,
    },
    MulExp {
        mul_exp: Box<MulExp>,
        mul_op: MulOp,
        unary_exp: Box<UnaryExp>,
    },
}

impl Expression for MulExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            MulExp::UnaryExp { unary_exp } => {
                return unary_exp.parse_var_exp(inst_list, dfg);
            }
            MulExp::MulExp {
                mul_exp,
                mul_op,
                unary_exp,
            } => {
                let left = mul_exp.parse_var_exp(inst_list, dfg);
                let right = unary_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            MulExp::UnaryExp { unary_exp } => {
                return unary_exp.parse_const_exp();
            }
            MulExp::MulExp {
                mul_exp,
                mul_op,
                unary_exp,
            } => {
                let left = mul_exp.parse_const_exp();
                let right = unary_exp.parse_const_exp();
                match (left, right) {
                    (ParseResult::Const(l), ParseResult::Const(r)) => {
                        let res = match mul_op {
                            MulOp::Mul => l * r,
                            MulOp::Div => l / r,
                            MulOp::Mod => l % r,
                        };
                        ParseResult::Const(res)
                    }
                    _ => panic!("Non-constant in const expression"),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AddExp {
    MulExp {
        mul_exp: Box<MulExp>,
    },
    AddExp {
        add_exp: Box<AddExp>,
        add_op: AddOp,
        mul_exp: Box<MulExp>,
    },
}

impl Expression for AddExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            AddExp::MulExp { mul_exp } => {
                return mul_exp.parse_var_exp(inst_list, dfg);
            }
            AddExp::AddExp {
                add_exp,
                add_op,
                mul_exp,
            } => {
                let left = add_exp.parse_var_exp(inst_list, dfg);
                let right = mul_exp.parse_var_exp(inst_list, dfg);

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

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            AddExp::MulExp { mul_exp } => {
                return mul_exp.parse_const_exp();
            }
            AddExp::AddExp {
                add_exp,
                add_op,
                mul_exp,
            } => {
                let left = add_exp.parse_const_exp();
                let right = mul_exp.parse_const_exp();
                match (left, right) {
                    (ParseResult::Const(l), ParseResult::Const(r)) => {
                        let res = match add_op {
                            AddOp::Add => l + r,
                            AddOp::Sub => l - r,
                        };
                        ParseResult::Const(res)
                    }
                    _ => panic!("Non-constant in const expression"),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum PrimaryExp {
    Number { value: i32 },
    LVal { l_val: LVal },
    Exp { exp: Box<Exp> },
}

impl Expression for PrimaryExp {
    fn parse_var_exp(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match self {
            PrimaryExp::Number { value } => ParseResult::Const(*value),
            PrimaryExp::Exp { exp } => exp.parse_var_exp(inst_list, dfg),

            PrimaryExp::LVal { l_val } => {
                if GLOBAL_VAR_TABLE.lock().unwrap().contains_key(&l_val.ident) {
                    GLOBAL_VAR_TABLE.lock().unwrap().get(&l_val.ident)
                } else if GLOBAL_CONST_TABLE
                    .lock()
                    .unwrap()
                    .contains_key(&l_val.ident)
                {
                    GLOBAL_CONST_TABLE.lock().unwrap().get(&l_val.ident)
                } else {
                    panic!("LVal not found in var table, maybe the ident is not defined");
                }
            }
        }
    }

    fn parse_const_exp(&self) -> ParseResult {
        match self {
            PrimaryExp::Number { value } => ParseResult::Const(*value),
            PrimaryExp::Exp { exp } => exp.parse_const_exp(),

            PrimaryExp::LVal { l_val } => {
                if (GLOBAL_CONST_TABLE
                    .lock()
                    .unwrap()
                    .contains_key(&l_val.ident))
                {
                    GLOBAL_CONST_TABLE
                        .lock()
                        .unwrap()
                        .get(&l_val.ident)
                        .unwrap()
                } else {
                    panic!("LVal not found in const table, maybe the ident is for a variable or not defined");
                }
            }
        }
    }
}
