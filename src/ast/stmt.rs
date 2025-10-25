use crate::ast::ast::{Block, LVal};
use crate::ast::decl::{GLOBAL_CONST_TABLE, GLOBAL_VAR_TABLE};
use crate::ast::exp::{Exp, Expression, IRObj};
use crate::config::config::BType;
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{
    insert_into_dfg_and_list, DataFlowGraph, InstData, InstId, Operand,
};

use std::cell::RefMut;

pub trait Statement {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<DataFlowGraph>);
}

#[derive(Debug, Clone)]
pub enum Stmt {
    RegularStmt { l_val: LVal, exp: Exp },
    RawExp { exp: Option<Exp> },
    Block { block: Box<Block> },
    ReturnStmt { exp: Exp },
}

impl Statement for Stmt {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<DataFlowGraph>) {
        match self {
            Stmt::RegularStmt { l_val, exp } => {
                if GLOBAL_CONST_TABLE.get(l_val.ident.as_str()).is_some() {
                    panic!("Cannot assign to a constant variable");
                } else if GLOBAL_VAR_TABLE.get(l_val.ident.as_str()).is_none() {
                    panic!("Variable {} not declared", l_val.ident);
                }

                let pointer = GLOBAL_VAR_TABLE.get(&l_val.ident).unwrap();
                let result = exp.parse_var_exp(inst_list, dfg);

                insert_into_dfg_and_list(
                    inst_list,
                    dfg,
                    InstData::new(
                        BType::Void, // assuming integer type for simplicity
                        IRObj::None,
                        KoopaOpCode::STORE,
                        vec![
                            match result {
                                IRObj::InstId(id) => Operand::InstId(id),
                                IRObj::Const(value) => Operand::Const(value),
                                IRObj::Pointer {
                                    initialized: _,
                                    pointer_id: _,
                                } => unimplemented!(),
                                IRObj::None => panic!("Cannot store void value"),
                            },

                            match pointer {
                                IRObj::Pointer {
                                    initialized,
                                    pointer_id,
                                } => Operand::Pointer(pointer_id),
                                _ => panic!("Expected a pointer for l_val {}", l_val.ident),
                            },
                        ],
                    ),
                );
                GLOBAL_VAR_TABLE.insert(l_val.ident.clone(), result.clone());
            }

            Stmt::ReturnStmt { exp } => {
                let result = exp.parse_var_exp(inst_list, dfg);

                insert_into_dfg_and_list(
                    inst_list,
                    dfg,
                    InstData::new(
                        BType::Void,
                        IRObj::None,
                        KoopaOpCode::RET,
                        vec![
                            match result {
                                IRObj::InstId(id) => Operand::InstId(id),
                                IRObj::Const(value) => Operand::Const(value),
                                IRObj::Pointer {
                                    initialized: _,
                                    pointer_id: _,
                                } => unimplemented!(),
                                IRObj::None => Operand::None,
                            },
                        ],
                    ),
                );
            }

            // is it necessary?
            Stmt::RawExp { exp } => {
                if let Some(e) = exp {
                    let _ = e.parse_var_exp(inst_list, dfg);
                }
            }

            Stmt::Block { block } => {
                // GLOBAL_CONST_TABLE.enter_scope();
                // GLOBAL_VAR_TABLE.enter_scope();

                // TODO: block.parse(inst_list, dfg);

                //     GLOBAL_VAR_TABLE.exit_scope();
                //     GLOBAL_CONST_TABLE.exit_scope();
                unimplemented!()
            }
        }
    }
}
