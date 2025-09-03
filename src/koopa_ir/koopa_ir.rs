use crate::ast::*;
use crate::config::KOOPA_TYPE_MAP;

pub struct Program {
    pub global_vals: Vec<GlobalVal>,
    pub funcs: Vec<Func>,
}

impl Program {
    pub fn new() -> Self {
        Self {
            global_vals: vec![],
            funcs: vec![],
        }
    }

    pub fn push_global_val(&mut self, global_val: GlobalVal) {
        self.global_vals.push(global_val);
    }

    pub fn push_func(&mut self, func: Func) {
        self.funcs.push(func);
    }
}

// customize formatting for Program
impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for func in &self.funcs {
            writeln!(
                f,
                "fun @{}({}): {} {{",
                func.name,
                func.get_params_str(),
                func.func_type
            );
            writeln!(f, "{:#?}", func.basic_block);
            writeln!(f, "}}");
        }
        Ok(())
    }
}

pub struct GlobalVal {
    pub name: String,
    pub val_type: String,
    pub val: i32,
}

impl GlobalVal {
    pub fn new(name: String, val_type: String, val: i32) -> Self {
        Self {
            name,
            val_type,
            val,
        }
    }
}

pub struct Func {
    pub name: String,
    pub func_type: FuncType,
    pub params: Vec<Param>,
    pub basic_block: BasicBlock,
}

impl Func {
    pub fn new(
        name: String,
        func_type: FuncType,
        params: Vec<Param>,
        basic_block: BasicBlock,
    ) -> Self {
        Self {
            name,
            func_type,
            params,
            basic_block,
        }
    }

    pub fn push_basic_block(&mut self, block: BasicBlock) {
        self.basic_block = block;
    }

    pub fn get_params_str(&self) -> String {
        self.params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.param_type))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub struct Param {
    pub name: String,
    pub param_type: String,
}

#[derive(Clone)]
pub struct BasicBlock {
    pub inst_list: Vec<Inst>,
}

impl BasicBlock {
    pub fn new() -> Self {
        Self { inst_list: vec![] }
    }

    pub fn push_stmt(&mut self, stmt: Stmt) {
        // only process return stmt temporarily
        let return_val: i32 = stmt.return_val;
        self.inst_list
            .push(Inst::new("return".to_string(), vec![return_val]));
    }
}

impl std::fmt::Debug for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "%entry: ");
        for inst in &self.inst_list {
            writeln!(f, "    {}", inst);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Inst {
    pub opcode: String,
    pub operands: Vec<i32>,
}

impl Inst {
    pub fn new(opcode: String, operands: Vec<i32>) -> Self {
        Self { opcode, operands }
    }
}

impl std::fmt::Display for Inst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let operands_str = self
            .operands
            .iter()
            .map(|op| op.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{} {}", self.opcode, operands_str)
    }
}

/// func to transform ast to koopa ir.
pub fn ast2koopa_ir(ast: &CompUnit) -> Result<Program, Box<dyn std::error::Error>> {
    // TODO: processing global value

    // get func type and ident
    let func_def = &ast.func_def;
    let func_type = KOOPA_TYPE_MAP
        .get(func_def.func_type.func_type.as_str())
        .unwrap();
    let func_name = func_def.ident.clone();

    // processing block
    let block = &func_def.block;
    let mut basic_block = BasicBlock::new();
    basic_block.push_stmt(block.stmt.clone());

    let mut func = Func::new(func_name, FuncType::new(func_type.clone()), vec![], basic_block.clone());
    func.push_basic_block(basic_block);

    // construct Program and return
    let mut program = Program::new();
    program.push_func(func);
    Ok(program)
}
