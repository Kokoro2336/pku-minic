use crate::ast::exp::{Exp, Expression, IRObj};
use crate::config::config::BType;
use crate::config::config::CONTEXT_STACK;
use crate::koopa_ir::config::{KoopaOpCode, PTR_ID_ALLOCATOR};
use crate::koopa_ir::koopa_ir::{insert_instruction, InstData, Operand};

use std::vec::Vec;

pub trait Declaration {
    fn parse(&self) -> IRObj;
}

#[derive(Debug, Clone)]
pub enum Decl {
    ConstDecl { const_decl: ConstDecl },
    VarDecl { var_decl: VarDecl },
}

impl Decl {
    pub fn parse(&self) {
        match self {
            Decl::ConstDecl { const_decl } => {
                const_decl.parse();
            }
            Decl::VarDecl { var_decl } => {
                var_decl.parse();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub b_type: BType,
    pub const_defs: Vec<ConstDef>,
}

impl ConstDecl {
    fn parse(&self) {
        for const_def in &self.const_defs {
            let result = const_def.parse();
            CONTEXT_STACK.with(|stack| {
                stack
                    .borrow_mut()
                    .insert_const(const_def.ident.clone(), result)
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub ident: String,
    pub const_init_val: ConstInitVal,
}

impl Declaration for ConstDef {
    fn parse(&self) -> IRObj {
        if CONTEXT_STACK
            .with(|stack| stack.borrow().get_current_pointer(self.ident.as_str()))
            .is_some()
        {
            panic!(
                "Cannot declare constant {} with the same name as a variable",
                self.ident
            );
        } else if CONTEXT_STACK
            .with(|stack| stack.borrow().get_current_const(self.ident.as_str()))
            .is_some()
        {
            panic!("Constant {} already declared", self.ident);
        }

        self.const_init_val.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstInitVal {
    pub const_exp: ConstExp,
}

impl Declaration for ConstInitVal {
    fn parse(&self) -> IRObj {
        self.const_exp.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstExp {
    pub exp: Box<Exp>,
}

impl Declaration for ConstExp {
    fn parse(&self) -> IRObj {
        self.exp.parse_const_exp()
    }
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub b_type: BType,
    pub var_defs: Vec<VarDef>,
}

impl VarDecl {
    fn parse(&self) {
        for var_def in &self.var_defs {
            let result = var_def.parse();
            // insert pointer into pointer table for parsing first.
            CONTEXT_STACK.with(|stack| {
                stack
                    .borrow_mut()
                    .insert_pointer(var_def.ident.clone(), result)
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct VarDef {
    pub ident: String,
    pub init_val: Option<InitVal>,
}

impl Declaration for VarDef {
    fn parse(&self) -> IRObj {
        // semantic check
        if CONTEXT_STACK
            .with(|stack| stack.borrow().get_current_const(self.ident.as_str()))
            .is_some()
        {
            panic!(
                "Cannot declare variable {} with the same name as a constant",
                self.ident
            );
        } else if CONTEXT_STACK
            .with(|stack| stack.borrow().get_current_pointer(self.ident.as_str()))
            .is_some()
        {
            panic!("Variable {} already declared", self.ident);
        }

        let pointer_id = PTR_ID_ALLOCATOR.with(|allocator| allocator.borrow_mut().alloc());
        // whatever the init_val is, we need to allocate space for the variable
        insert_instruction(InstData::new(
            BType::Int,
            IRObj::Pointer {
                initialized: self.init_val.is_some(),
                pointer_id, // placeholder, will be replaced
            },
            KoopaOpCode::ALLOC,
            vec![Operand::BType(BType::Int)],
        ));

        if let Some(init_val) = &self.init_val {
            let parse_result = init_val.parse();
            // we don't need to store temp var to var_table here for it'll be removed soon after STORE
            if let IRObj::Const(_) | IRObj::InstId(_) = parse_result {
                insert_instruction(InstData::new(
                    BType::Void,
                    IRObj::None,
                    KoopaOpCode::STORE,
                    vec![
                        match parse_result {
                            IRObj::InstId(id) => Operand::InstId(id),
                            IRObj::Const(c) => Operand::Const(c),
                            _ => unreachable!(),
                        },
                        // the allocated address
                        Operand::Pointer(pointer_id),
                    ],
                ));
            }
        };

        IRObj::Pointer {
            initialized: self.init_val.is_some(),
            pointer_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitVal {
    pub exp: Box<Exp>,
}

impl Declaration for InitVal {
    fn parse(&self) -> IRObj {
        self.exp.parse_var_exp()
    }
}
