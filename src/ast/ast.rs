/// define the AST structure
#[derive(Debug)]
pub struct CompUnit {
    pub func_def: FuncDef,
}

#[derive(Debug)]
pub struct FuncDef {
    pub func_type: FuncType,
    pub ident: String,
    pub block: Block,
}

#[derive(Debug)]
pub struct FuncType {
    pub func_type: String,
}

impl FuncType {
    pub fn new(func_type: String) -> Self {
        Self { func_type }
    }
}

impl std::fmt::Display for FuncType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.func_type)
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmt: Stmt,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub return_val: i32,
}
