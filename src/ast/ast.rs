use crate::ast::decl::Decl;
use crate::ast::stmt::{Statement, Stmt};
use crate::config::config::{BType, CONTEXT_STACK};
use crate::koopa_ir::koopa_ir::{Func, IRBlock, Program};

use std::rc::Rc;

/// define the AST structure
#[derive(Debug)]
pub struct CompUnit {
    pub global_decls: Vec<Decl>,
    pub func_defs: Vec<FuncDef>,
}

impl CompUnit {
    pub fn parse(&self) -> Result<Program, Box<dyn std::error::Error>> {
        // construct Program and return
        let mut program = Program::new();
        for func in &self.func_defs {
            program.push_func(Rc::clone(&func.parse()));
        }
        Ok(program)
    }
}

#[derive(Debug)]
pub struct FuncDef {
    pub func_type: BType,
    pub ident: String,
    pub block: Block,
}

impl FuncDef {
    fn parse(&self) -> Rc<Func> {
        // TODO: processing global value

        // get func type and ident
        let func_type = &self.func_type;
        let func_name = self.ident.clone();

        let mut func = Rc::new(Func::new(func_name, func_type.clone(), vec![]));

        CONTEXT_STACK.with(|stack| stack.borrow_mut().enter_func_scope(Rc::clone(&func)));

        {
            let ir_block = Rc::new(IRBlock::new());
            self.block.parse(Rc::clone(&ir_block));
            let func_mut = Rc::get_mut(&mut func).unwrap();
            func_mut.push_ir_block(ir_block);
        }

        CONTEXT_STACK.with(|stack| stack.borrow_mut().exit_scope());

        func
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub block_items: Vec<BlockItem>,
}

impl Block {
    pub fn parse(&self, ir_block: Rc<IRBlock>) {
        CONTEXT_STACK.with(|stack| stack.borrow_mut().enter_block_scope(ir_block));

        for item in &self.block_items {
            item.parse();
        }

        CONTEXT_STACK.with(|stack| stack.borrow_mut().exit_scope());
    }
}

#[derive(Debug, Clone)]
pub enum BlockItem {
    Decl { decl: Decl },
    Stmt { stmt: Stmt },
}

impl BlockItem {
    pub fn parse(&self) {
        match self {
            BlockItem::Decl { decl } => {
                decl.parse();
            }
            BlockItem::Stmt { stmt } => {
                stmt.parse();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LVal {
    pub ident: String,
}
