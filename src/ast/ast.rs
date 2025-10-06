use crate::config::config::BType;
use crate::ast::decl::Decl;
use crate::ast::stmt::Stmt;

/// define the AST structure
#[derive(Debug)]
pub struct CompUnit {
    pub func_def: FuncDef,
}

#[derive(Debug)]
pub struct FuncDef {
    pub func_type: BType,
    pub ident: String,
    pub block: Block,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub block_items: Vec<BlockItem>,
}

#[derive(Debug, Clone)]
pub enum BlockItem {
    Decl { decl: Decl },
    Stmt { stmt: Stmt },
}

#[derive(Debug, Clone)]
pub struct LVal {
    pub ident: String,
}
