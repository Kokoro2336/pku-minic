use crate::ast::decl::Decl;
use crate::ast::stmt::Stmt;
use crate::config::config::BType;
use crate::koopa_ir::koopa_ir::{BasicBlock, Func, Program};

use std::cell::RefCell;
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
    fn parse(&self) -> Rc<RefCell<Func>> {
        // TODO: processing global value

        // get func type and ident
        let func_type = &self.func_type;
        let func_name = self.ident.clone();

        let func = Rc::new(RefCell::new(Func::new(
            func_name,
            func_type.clone(),
            vec![],
        )));

        // processing block
        {
            // as the cycle reference of basic block to func, we need to init it previously rather than in .parse()
            let mut ir_block = BasicBlock::new(&Rc::clone(&func));
            let basic_block = self.block.parse(ir_block);
            let mut func_mut = func.borrow_mut();
            func_mut.push_basic_block(basic_block);
        }

        func
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub block_items: Vec<BlockItem>,
}

impl Block {
    fn parse(&self, mut basic_block: BasicBlock) -> BasicBlock {
        for item in &self.block_items {
            basic_block.parse_item(item);
        }
        basic_block
    }
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
