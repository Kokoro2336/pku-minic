use crate::asm::config::RVRegCode;
use crate::ast::*;
use crate::config::ValueType;
use crate::koopa_ir::config::KoopaOpCode;
use crate::ast::exp::{ParseResult, Expression};
use crate::ast::decl::{Decl, Declaration};
use crate::ast::stmt::{Stmt, Statement};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

#[derive(Clone)]
pub struct Program {
    pub global_vals: Vec<KoopaGlobalVal>,
    pub funcs: Vec<Rc<RefCell<Func>>>,
}

impl Program {
    pub fn new() -> Self {
        Self {
            global_vals: vec![],
            funcs: vec![],
        }
    }

    pub fn push_global_val(&mut self, global_val: KoopaGlobalVal) {
        self.global_vals.push(global_val);
    }

    pub fn push_func(&mut self, func: Rc<RefCell<Func>>) {
        self.funcs.push(func);
    }
}

// customize formatting for Program
impl std::fmt::Display for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for func in &self.funcs {
            writeln!(f, "{}", func.borrow())?;
        }
        Ok(())
    }
}

pub struct DataFlowGraph {
    next_inst_id: InstId,
    pub inst_map: HashMap<InstId, InstData>,
}

impl DataFlowGraph {
    pub fn new() -> Self {
        Self {
            next_inst_id: 0,
            inst_map: HashMap::new(),
        }
    }

    pub fn insert_inst(&mut self, inst: InstData) -> InstId {
        let inst_id = self.next_inst_id;
        self.inst_map.insert(inst_id, inst);
        self.next_inst_id += 1;
        inst_id
    }

    pub fn get_inst(&self, inst_id: &InstId) -> Option<&InstData> {
        self.inst_map.get(inst_id)
    }

    pub fn set_reg_none(&mut self, inst_id: InstId) {
        if let Some(inst) = self.inst_map.get_mut(&inst_id) {
            inst.reg_used = None;
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }

    pub fn set_reg(&mut self, inst_id: &InstId, reg: Option<RVRegCode>) {
        if let Some(inst) = self.inst_map.get_mut(&*inst_id) {
            inst.reg_used = reg;
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }

    fn get_next_inst_id(&self) -> InstId {
        self.next_inst_id
    }
}

#[derive(Clone)]
pub struct KoopaGlobalVal {
    pub name: String,
    pub val_type: ValueType,
    pub val: i32,
}

impl KoopaGlobalVal {
    pub fn new(name: String, val_type: ValueType, val: i32) -> Self {
        Self {
            name,
            val_type,
            val,
        }
    }
}

#[derive(Clone)]
pub struct Func {
    pub name: String,
    pub func_type: ValueType,
    pub params: Vec<Param>,
    pub dfg: Rc<RefCell<DataFlowGraph>>,
    pub basic_blocks: Vec<Rc<BasicBlock>>,
}

impl std::fmt::Display for Func {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "fun @{}({}): {} {{",
            self.name,
            self.get_params_str(),
            self.func_type
        )?;
        for block in &self.basic_blocks {
            writeln!(f, "{}", block)?;
        }
        writeln!(f, "}}")
    }
}

impl Func {
    pub fn new(name: String, func_type: ValueType, params: Vec<Param>) -> Self {
        Self {
            name,
            func_type,
            params,
            dfg: Rc::new(RefCell::new(DataFlowGraph::new())),
            basic_blocks: vec![],
        }
    }

    pub fn push_basic_block(&mut self, block: BasicBlock) {
        self.basic_blocks.push(Rc::new(block));
    }

    pub fn get_params_str(&self) -> String {
        self.params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.param_type))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Clone)]
pub struct Param {
    pub name: String,
    pub param_type: ValueType,
}

#[derive(Clone)]
pub struct BasicBlock {
    pub inst_list: Vec<InstId>,
    func: Weak<RefCell<Func>>,
}

impl BasicBlock {
    pub fn new(func: &Rc<RefCell<Func>>) -> Self {
        Self {
            inst_list: vec![],
            func: Rc::downgrade(func),
        }
    }

    pub fn push_item(&mut self, item: BlockItem) {
        let func_rc = self.func.upgrade().unwrap();
        let func_rc_immut = func_rc.borrow();
        let mut dfg = func_rc_immut.dfg.as_ref().borrow_mut();
        let inst_list = &mut self.inst_list;

        match item {
            BlockItem::Decl { decl } => {
                decl.parse(inst_list, &mut dfg);
            }

            BlockItem::Stmt { stmt } => {
                stmt.parse(inst_list, &mut dfg);
            }
        };
    }
}

impl std::fmt::Display for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "%entry: ")?;

        let func_rc = self.func.upgrade().unwrap();
        let func_rc_immut = func_rc.borrow();
        let dfg = func_rc_immut.dfg.as_ref().borrow();
        for inst in &self.inst_list {
            let inst_data = dfg.get_inst(inst).unwrap();
            match inst_data.opcode {
                KoopaOpCode::RET => {
                    writeln!(f, "  ret {}", inst_data.operands[0].to_string())?;
                    continue;
                }
                _ => {
                    writeln!(f, "  %{} = {}", inst, inst_data)?;
                }
            }
        }
        Ok(())
    }
}

/// instruction id for DFG
pub type InstId = u32;

#[derive(Clone)]
pub enum Operand {
    InstId(InstId), // maybe the operand refers to another instruction's result
    Const(i32),     // maybe the operand is a constant value
}

impl Operand {
    pub fn from_parse_result(parse_result: ParseResult) -> Self {
        match parse_result {
            ParseResult::InstId(id) => Operand::InstId(id),
            ParseResult::Const(c) => Operand::Const(c),
            // ?
            ParseResult::None => None
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Operand::InstId(id) => format!("%{}", id),
            Operand::Const(c) => format!("{}", c),
        }
    }
}

#[derive(Clone)]
pub struct InstData {
    pub opcode: KoopaOpCode,
    pub operands: Vec<Operand>,
    pub inst_used: Vec<InstId>, // instructions used by this instruction in sequence
    pub reg_used: Option<RVRegCode>, // reg used by this instruction(excluding the source regs)
}

impl InstData {
    pub fn new(opcode: KoopaOpCode, operands: Vec<Operand>) -> Self {
        // TODO: find the users of the inst

        Self {
            opcode,
            operands,
            inst_used: vec![],
            reg_used: None,
        }
    }
}

impl std::fmt::Display for InstData {
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
    let func_type = &func_def.func_type;
    let func_name = func_def.ident.clone();

    let func = Rc::new(RefCell::new(Func::new(
        func_name,
        func_type.clone(),
        vec![],
    )));

    // processing block
    let block = &func_def.block;
    let mut basic_block = BasicBlock::new(&Rc::clone(&func));
    for item in &block.block_items {
        basic_block.push_item(block.stmt.clone());
    }

    let mut func_mut = func.borrow_mut();
    func_mut.push_basic_block(basic_block);

    // construct Program and return
    let mut program = Program::new();
    program.push_func(Rc::clone(&func));
    Ok(program)
}
