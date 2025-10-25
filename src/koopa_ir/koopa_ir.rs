use crate::asm::config::{RVRegCode, RVREG_ALLOCATOR};
use crate::ast::{ast::*, exp::*, stmt::*};
use crate::config::config::BType;
use crate::koopa_ir::config::KoopaOpCode;

use lazy_static::lazy_static;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Mutex;

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

    pub fn get_next_inst_id(&self) -> InstId {
        self.next_inst_id
    }

    pub fn get_inst(&self, inst_id: &InstId) -> Option<&InstData> {
        self.inst_map.get(inst_id)
    }

    pub fn free_reg_used(&mut self, inst_id: InstId) {
        if let Some(inst) = self.inst_map.get_mut(&inst_id) {
            RVREG_ALLOCATOR.free_reg(inst.reg_used.unwrap());

            inst.free_reg_used();
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }

    pub fn set_reg(&mut self, inst_id: &InstId, reg: Option<RVRegCode>) {
        if let Some(inst) = self.inst_map.get_mut(&*inst_id) {
            inst.set_reg(reg.unwrap());
            // concerning that InstData doesn't contain its inst_id, we have to occupy the reg on DFG layer
            RVREG_ALLOCATOR.occupy_reg(reg.unwrap(), *inst_id);
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }

    pub fn add_user(&mut self, inst_id: &InstId, user_inst_id: InstId) {
        if let Some(inst) = self.inst_map.get_mut(&*inst_id) {
            inst.add_user(user_inst_id);
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }

    pub fn remove_user(&mut self, inst_id: &InstId, user_inst_id: InstId) {
        if let Some(inst) = self.inst_map.get_mut(inst_id) {
            inst.remove_user(user_inst_id);

            // if no users, free the register
            if inst.users.is_empty() {
                RVREG_ALLOCATOR.free_reg(inst.reg_used.unwrap());

                inst.free_reg_used();
            }
        } else {
            panic!("Instruction not found for inst_id {:?}", inst_id);
        }
    }
}

#[derive(Clone)]
pub struct KoopaGlobalVal {
    pub name: String,
    pub val_type: BType,
    pub val: i32,
}

impl KoopaGlobalVal {
    pub fn new(name: String, val_type: BType, val: i32) -> Self {
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
    pub func_type: BType,
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
    pub fn new(name: String, func_type: BType, params: Vec<Param>) -> Self {
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
    pub param_type: BType,
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

    pub fn parse_item(&mut self, item: &BlockItem) {
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

#[derive(Debug, Clone)]
pub enum Operand {
    InstId(InstId), // maybe the operand refers to another instruction's result
    Const(i32),     // maybe the operand is a constant value
    BType(BType),   // maybe the operand is a type
    Pointer(u32),
    None,
}

impl Operand {
    pub fn from_parse_result(parse_result: IRObj) -> Self {
        match parse_result {
            IRObj::InstId(id) => Operand::InstId(id),
            IRObj::Const(c) => Operand::Const(c),
            IRObj::Pointer {
                initialized: _,
                pointer_id,
            } => Operand::Pointer(pointer_id),
            // None matches to void return
            IRObj::None => {
                panic!("Cannot convert IRObj::None to Operand")
            }
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Operand::InstId(id) => format!("%{}", id),
            Operand::Const(c) => format!("{}", c),
            Operand::BType(b_type) => format!("{}", b_type),
            Operand::Pointer(pointer) => format!("@{}", pointer),
            Operand::None => "".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct InstData {
    pub typ: BType,
    pub ir_obj: IRObj,
    pub opcode: KoopaOpCode,
    pub operands: Vec<Operand>,
    pub users: Vec<InstId>, // instructions use this instruction's result
    pub reg_used: Option<RVRegCode>, // reg used by this instruction(excluding the source regs)
}

impl InstData {
    // id is either pointer_id or inst_id
    pub fn new(typ: BType, ir_obj: IRObj, opcode: KoopaOpCode, operands: Vec<Operand>) -> Self {
        Self {
            typ,
            ir_obj,
            opcode,
            operands,
            users: vec![],
            reg_used: None,
        }
    }

    pub fn add_user(&mut self, user_inst_id: InstId) {
        self.users.push(user_inst_id);
    }

    pub fn remove_user(&mut self, user_inst_id: InstId) {
        if let Some(pos) = self.users.iter().position(|&id| id == user_inst_id) {
            self.users.swap_remove(pos);
        }
    }

    pub fn free_reg_used(&mut self) {
        self.reg_used = None;
    }

    pub fn set_reg(&mut self, reg: RVRegCode) {
        self.reg_used = Some(reg);
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

pub fn insert_into_dfg_and_list(
    inst_list: &mut Vec<InstId>,
    dfg: &mut RefMut<'_, DataFlowGraph>,
    inst_data: InstData,
) -> IRObj {
    let inst_id = dfg.insert_inst(inst_data.clone());

    // add this inst as a user to all its operand instructions
    for operand in &inst_data.operands {
        if let Operand::InstId(op_id) = operand {
            dfg.add_user(op_id, inst_id);
        }
    }

    inst_list.push(inst_id);
    IRObj::InstId(inst_id)
}
