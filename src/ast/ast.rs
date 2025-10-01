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
    pub exp: Option<Exp>,
}

#[derive(Debug, Clone)]
pub enum Exp {
    LOrExp { lor_exp: Box<LOrExp> },
}

#[derive(Debug, Clone)]
pub enum LOrExp {
    LAndExp {
        land_exp: Box<LAndExp>,
    },
    LOrExp {
        lor_exp: Box<LOrExp>,
        lor_op: LOrOp,
        land_exp: Box<LAndExp>,
    },
}

#[derive(Debug, Clone)]
pub enum LAndExp {
    EqExp {
        eq_exp: Box<EqExp>,
    },
    LAndExp {
        land_exp: Box<LAndExp>,
        land_op: LAndOp,
        eq_exp: Box<EqExp>,
    },
}

#[derive(Debug, Clone)]
pub enum EqExp {
    RelExp {
        rel_exp: Box<RelExp>,
    },
    EqExp {
        eq_exp: Box<EqExp>,
        eq_op: EqOp,
        rel_exp: Box<RelExp>,
    },
}

#[derive(Debug, Clone)]
pub enum RelExp {
    AddExp {
        add_exp: Box<AddExp>,
    },
    RelExp {
        rel_exp: Box<RelExp>,
        rel_op: RelOp,
        add_exp: Box<AddExp>,
    },
}

#[derive(Debug, Clone)]
pub enum UnaryExp {
    PrimaryExp {
        exp: Box<PrimaryExp>,
    },
    UnaryExp {
        unary_op: UnaryOp,
        unary_exp: Box<UnaryExp>,
    },
}

#[derive(Debug, Clone)]
pub enum MulExp {
    UnaryExp {
        unary_exp: Box<UnaryExp>,
    },
    MulExp {
        mul_exp: Box<MulExp>,
        mul_op: MulOp,
        unary_exp: Box<UnaryExp>,
    },
}

#[derive(Debug, Clone)]
pub enum AddExp {
    MulExp {
        mul_exp: Box<MulExp>,
    },
    AddExp {
        add_exp: Box<AddExp>,
        add_op: AddOp,
        mul_exp: Box<MulExp>,
    },
}

#[derive(Debug, Clone)]
pub enum PrimaryExp {
    Number { value: i32 },
    Exp { exp: Box<Exp> },
}

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
