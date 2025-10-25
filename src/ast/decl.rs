use crate::ast::exp::{Exp, Expression, IRObj};
use crate::config::config::BType;
use crate::koopa_ir::config::{KoopaOpCode, PTR_ID_ALLOCATOR};
use crate::koopa_ir::koopa_ir::{
    insert_into_dfg_and_list, DataFlowGraph, InstData, InstId, Operand,
};

use lazy_static::lazy_static;
use std::cell::RefMut;
use std::collections::HashMap;
use std::sync::Mutex;
use std::vec::Vec;

#[derive(Debug)]
pub struct SymbolTable {
    table: Mutex<Vec<HashMap<String, IRObj>>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            table: Mutex::new(vec![HashMap::new()]),
        }
    }

    pub fn enter_scope(&self) {
        let mut table = self.table.lock().unwrap();
        table.push(HashMap::new());
    }

    pub fn exit_scope(&self) {
        let mut table = self.table.lock().unwrap();
        table.pop();
    }

    pub fn insert(&self, name: String, value: IRObj) {
        let mut table = self.table.lock().unwrap();
        if let Some(scope) = table.last_mut() {
            scope.insert(name, value);
        } else {
            panic!("No scope available to insert symbol");
        }
    }

    pub fn get(&self, name: &str) -> Option<IRObj> {
        let table = self.table.lock().unwrap();
        for scope in table.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.clone());
            }
        }
        None
    }
}

lazy_static! {
    pub static ref GLOBAL_CONST_TABLE: SymbolTable = SymbolTable::new();
    pub static ref GLOBAL_VAR_TABLE: SymbolTable = SymbolTable::new();
}

#[derive(Debug, Clone)]
pub enum Decl {
    ConstDecl { const_decl: ConstDecl },
    VarDecl { var_decl: VarDecl },
}

impl Decl {
    pub fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>) {
        match self {
            Decl::ConstDecl { const_decl } => {
                const_decl.parse();
            }
            Decl::VarDecl { var_decl } => {
                var_decl.parse(inst_list, dfg);
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
            GLOBAL_CONST_TABLE.insert(const_def.ident.clone(), result);
        }
    }
}

pub trait ConstDeclaration {
    fn parse(&self) -> IRObj;
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub ident: String,
    pub const_init_val: ConstInitVal,
}

impl ConstDeclaration for ConstDef {
    fn parse(&self) -> IRObj {
        self.const_init_val.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstInitVal {
    pub const_exp: ConstExp,
}

impl ConstDeclaration for ConstInitVal {
    fn parse(&self) -> IRObj {
        self.const_exp.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstExp {
    pub exp: Box<Exp>,
}

impl ConstDeclaration for ConstExp {
    fn parse(&self) -> IRObj {
        self.exp.parse_const_exp()
    }
}

pub trait VarDeclaration {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>) -> IRObj;
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub b_type: BType,
    pub var_defs: Vec<VarDef>,
}

impl VarDecl {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>) {
        for var_def in &self.var_defs {
            let result = var_def.parse(inst_list, dfg);
            // insert pointer into symbol table for parsing first.
            GLOBAL_VAR_TABLE.insert(var_def.ident.clone(), result);
        }
    }
}

#[derive(Debug, Clone)]
pub struct VarDef {
    pub ident: String,
    pub init_val: Option<InitVal>,
}

impl VarDeclaration for VarDef {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>) -> IRObj {
        let pointer_id = PTR_ID_ALLOCATOR.alloc();
        // whatever the init_val is, we need to allocate space for the variable
        insert_into_dfg_and_list(
            inst_list,
            dfg,
            InstData::new(
                BType::Int,
                IRObj::Pointer {
                    initialized: self.init_val.is_some(),
                    pointer_id, // placeholder, will be replaced
                },
                KoopaOpCode::ALLOC,
                vec![Operand::BType(BType::Int)],
            ),
        );

        match &self.init_val {
            Some(init_val) => {
                let parse_result = init_val.parse(inst_list, dfg);
                match parse_result {
                    IRObj::Const(_) | IRObj::InstId(_) => {
                        insert_into_dfg_and_list(
                            inst_list,
                            dfg,
                            InstData::new(
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
                            ),
                        );
                    }

                    _ => {}
                }
            }

            // if no init value, just return None(no pointer)
            None => {}
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

impl VarDeclaration for InitVal {
    fn parse(&self, inst_list: &mut Vec<InstId>, dfg: &mut RefMut<'_, DataFlowGraph>) -> IRObj {
        self.exp.parse_var_exp(inst_list, dfg)
    }
}
