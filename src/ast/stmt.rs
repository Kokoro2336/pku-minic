use crate::ast::ast::{Block, LVal};
use crate::ast::exp::{Exp, Expression, IRObj};
use crate::config::config::BType;
use crate::config::config::CONTEXT_STACK;
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{insert_instruction, InstData, Operand};

pub trait Statement {
    fn parse(&self);
}

#[derive(Debug, Clone)]
pub enum Stmt {
    RegularStmt { l_val: LVal, exp: Exp },
    RawExp { exp: Option<Exp> },
    Block { block: Box<Block> },
    ReturnStmt { exp: Exp },
}

impl Statement for Stmt {
    fn parse(&self) {
        match self {
            Stmt::RegularStmt { l_val, exp } => {
                if CONTEXT_STACK
                    .with(|stack| stack.borrow().get_const(l_val.ident.as_str()))
                    .is_some()
                {
                    panic!("Cannot assign to a constant variable");
                } else if CONTEXT_STACK
                    .with(|stack| stack.borrow().get_var(l_val.ident.as_str()))
                    .is_none()
                {
                    panic!("Variable {} not declared", l_val.ident);
                }

                let pointer = CONTEXT_STACK
                    .with(|stack| stack.borrow().get_var(&l_val.ident))
                    .unwrap();
                let result = exp.parse_var_exp();

                insert_instruction(InstData::new(
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
                                initialized: _,
                                pointer_id,
                            } => Operand::Pointer(pointer_id),
                            _ => panic!("Expected a pointer for l_val {}", l_val.ident),
                        },
                    ],
                ));

                CONTEXT_STACK.with(|stack| {
                    stack
                        .borrow_mut()
                        .insert_var(l_val.ident.clone(), result.clone())
                });
            }

            Stmt::ReturnStmt { exp } => {
                let result = exp.parse_var_exp();

                insert_instruction(InstData::new(
                    BType::Void,
                    IRObj::None,
                    KoopaOpCode::RET,
                    vec![match result {
                        IRObj::InstId(id) => Operand::InstId(id),
                        IRObj::Const(value) => Operand::Const(value),
                        IRObj::Pointer {
                            initialized: _,
                            pointer_id: _,
                        } => unimplemented!(),
                        IRObj::None => Operand::None,
                    }],
                ));
            }

            // is it necessary?
            Stmt::RawExp { exp } => {
                if let Some(e) = exp {
                    let _ = e.parse_var_exp();
                }
            }

            Stmt::Block { block } => {
                block.parse();
            }
        }
    }
}
