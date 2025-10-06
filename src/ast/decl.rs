use crate::ast::exp::{Exp, Expression, ParseResult};
use crate::config::config::BType;
use crate::koopa_ir::koopa_ir::{DataFlowGraph, InstId};

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use std::cell::RefMut;

lazy_static! {
    pub static ref GLOBAL_CONST_TABLE: Mutex<HashMap<String, ParseResult>> =
        Mutex::new(HashMap::new());
    pub static ref GLOBAL_VAR_TABLE: Mutex<HashMap<String, ParseResult>> =
        Mutex::new(HashMap::new());
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
            GLOBAL_CONST_TABLE
                .lock()
                .unwrap()
                .insert(const_def.ident.clone(), result);
        }
    }
}

pub trait ConstDeclaration {
    fn parse(&self) -> ParseResult;
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub ident: String,
    pub const_init_val: ConstInitVal,
}

impl ConstDeclaration for ConstDef {
    fn parse(&self) -> ParseResult {
        self.const_init_val.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstInitVal {
    pub const_exp: ConstExp,
}

impl ConstDeclaration for ConstInitVal {
    fn parse(&self) -> ParseResult {
        self.const_exp.parse()
    }
}

#[derive(Debug, Clone)]
pub struct ConstExp {
    pub exp: Box<Exp>,
}

impl ConstDeclaration for ConstExp {
    fn parse(&self) -> ParseResult {
        self.exp.parse_const_exp()
    }
}

pub trait VarDeclaration {
    fn parse(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult;
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub b_type: BType,
    pub var_defs: Vec<VarDef>,
}

impl VarDecl {
    fn parse(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) {
        for var_def in &self.var_defs {
            let result = var_def.parse(inst_list, dfg);
            GLOBAL_VAR_TABLE
                .lock()
                .unwrap()
                .insert(var_def.ident.clone(), result);
        }
    }
}

#[derive(Debug, Clone)]
pub struct VarDef {
    pub ident: String,
    pub init_val: Option<InitVal>,
}

impl VarDeclaration for VarDef {
    fn parse(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        match &self.init_val {
            Some(init_val) => init_val.parse(inst_list, dfg),
            None => ParseResult::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitVal {
    pub exp: Box<Exp>,
}

impl VarDeclaration for InitVal {
    fn parse(
        &self,
        inst_list: &mut Vec<InstId>,
        dfg: &mut RefMut<'_, DataFlowGraph>,
    ) -> ParseResult {
        self.exp.parse_var_exp(inst_list, dfg)
    }
}
