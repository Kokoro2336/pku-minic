#[derive(Debug, Clone)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone)]
pub enum MulOp {
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone)]
pub enum AddOp {
    Add,
    Sub,
}

#[derive(Debug, Clone)]
pub enum RelOp {
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone)]
pub enum EqOp {
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
pub enum LAndOp {
    And,
}

#[derive(Debug, Clone)]
pub enum LOrOp {
    Or,
}
