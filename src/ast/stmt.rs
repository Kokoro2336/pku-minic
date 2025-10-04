use crate::ast::ast::LVal;
use crate::ast::exp::{Exp, ParseResult, Expression};
use crate::ast::decl::{GLOBAL_CONST_TABLE, GLOBAL_VAR_TABLE};
use crate::koopa_ir::koopa_ir::{DataFlowGraph, InstId, InstData, Operand};
use crate::koopa_ir::config::KoopaOpCode;

pub trait Statement {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<DataFlowGraph>);
}

#[derive(Debug, Clone)]
pub enum Stmt {
    RegularStmt { l_val: LVal, exp: Exp },
    ReturnStmt { exp: Exp },
}

impl Statement for Stmt {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<DataFlowGraph>) {
        match self {
            Stmt::RegularStmt { l_val, exp } => {
                let result = exp.parse_var_exp(inst_list, dfg);

                GLOBAL_VAR_TABLE
                    .lock()
                    .unwrap()
                    .insert(l_val.ident.clone(), result.clone());
            }

            Stmt::ReturnStmt { exp } => {
                let result = exp.parse_var_exp(inst_list, dfg);

                let inst_id = dfg.push_inst(InstData::new(
                    KoopaOpCode::RET,
                    vec![match result {
                        ParseResult::InstId(id) => Operand::InstId(id),
                        ParseResult::Const(value) => Operand::Const(value),
                        ParseResult::None => panic!("Return expression resulted in None"),
                    }],
                ));

                inst_list.push(inst_id);
            }
        }
    }
}
