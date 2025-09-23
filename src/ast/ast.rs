use crate::config::ValueType;

/// define the AST structure
#[derive(Debug)]
pub struct CompUnit {
    pub func_def: FuncDef,
}

#[derive(Debug)]
pub struct FuncDef {
    pub func_type: ValueType,
    pub ident: String,
    pub block: Block,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmt: Stmt,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub return_val: i32,
}
