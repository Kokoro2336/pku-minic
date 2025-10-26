use crate::ast::exp::IRObj;
use crate::koopa_ir::koopa_ir::{DataFlowGraph, Func, IRBlock, InstId};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// type of value
#[derive(Debug, Clone)]
pub enum BType {
    Int,
    Void,
}

impl BType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BType::Int => "int",
            BType::Void => "void",
        }
    }

    pub fn size_in_bytes(&self) -> u32 {
        match self {
            BType::Int => 4,
            BType::Void => 0,
        }
    }
}

impl std::fmt::Display for BType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BType::Int => write!(f, "i32"),
            BType::Void => write!(f, "void"),
        }
    }
}

pub struct Context {
    // current function
    pub func: Rc<Func>,
    // current block
    pub ir_block: Option<Rc<IRBlock>>,
    // current function's symbol tables
    pub global_const_table: HashMap<String, IRObj>,
    // current function's variable symbol tables
    pub global_var_table: HashMap<String, IRObj>,
}

impl Context {
    pub fn new(func: Rc<Func>, ir_block: Option<Rc<IRBlock>>) -> Self {
        Context {
            func: Rc::clone(&func),
            ir_block,
            global_const_table: HashMap::new(),
            global_var_table: HashMap::new(),
        }
    }
}

pub struct ContextStack {
    pub stack: Vec<Context>,
}

impl ContextStack {
    pub fn new() -> Self {
        ContextStack { stack: vec![] }
    }

    pub fn enter_block_scope(&mut self, ir_block: Rc<IRBlock>) {
        let stack = &mut self.stack;
        let last_context = stack.last_mut().unwrap();
        let func = Rc::clone(&last_context.func);

        if last_context.ir_block.is_none() {
            last_context.ir_block = Some(ir_block);
        } else {
            stack.push(Context::new(func, Some(ir_block)));
        }
    }

    pub fn enter_func_scope(&mut self, func: Rc<Func>) {
        let stack = &mut self.stack;
        stack.push(Context::new(func, None));
    }

    pub fn exit_scope(&mut self) {
        let stack = &mut self.stack;
        stack.pop();
    }

    pub fn insert_var(&mut self, name: String, value: IRObj) {
        if let Some(current_context) = self.stack.last_mut() {
            current_context.global_var_table.insert(name, value);
        } else {
            panic!("No context available to insert variable");
        }
    }

    pub fn insert_const(&mut self, name: String, value: IRObj) {
        if let Some(current_context) = self.stack.last_mut() {
            current_context.global_const_table.insert(name, value);
        } else {
            panic!("No context available to insert constant");
        }
    }

    pub fn get_var(&self, name: &str) -> Option<IRObj> {
        for context in self.stack.iter().rev() {
            if let Some(value) = context.global_var_table.get(name) {
                return Some(value.clone());
            }
        }
        None
    }

    pub fn get_const(&self, name: &str) -> Option<IRObj> {
        for context in self.stack.iter().rev() {
            if let Some(value) = context.global_const_table.get(name) {
                return Some(value.clone());
            }
        }
        None
    }

    pub fn get_current_dfg(&self) -> Rc<RefCell<DataFlowGraph>> {
        let stack = &self.stack;
        if let Some(current_context) = stack.last() {
            Rc::clone(&current_context.func.dfg)
        } else {
            panic!("No context available to get DFG");
        }
    }

    pub fn get_current_inst_list(&self) -> Rc<RefCell<Vec<InstId>>> {
        let stack = &self.stack;
        if let Some(current_context) = stack.last() {
            if let Some(ir_block) = &current_context.ir_block {
                Rc::clone(&ir_block.inst_list)
            } else {
                panic!("You couldn't call this func when inst_list is None");
            }
        } else {
            panic!("No context available to get instruction list");
        }
    }
}

#[derive(Debug)]
pub struct SymbolTable {
    table: Vec<HashMap<String, IRObj>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable { table: vec![] }
    }

    pub fn insert(&mut self, name: String, value: IRObj) {
        if let Some(scope) = self.table.last_mut() {
            scope.insert(name, value);
        } else {
            panic!("No scope available to insert symbol");
        }
    }

    pub fn get(&self, name: &str) -> Option<IRObj> {
        for scope in self.table.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.clone());
            }
        }
        None
    }
}

thread_local! {
    pub static CONTEXT_STACK: RefCell<ContextStack> = RefCell::new(ContextStack::new());
}
