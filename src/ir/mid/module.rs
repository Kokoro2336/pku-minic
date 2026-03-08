//! Definition of IR module, including global variables and functions.

use crate::ir::mid::{CG, DFG};

#[derive(Debug, Clone)]
pub struct IR {
    // Including:
    // 1. global variables
    // 2. SysY library functions
    pub globals: DFG,
    // global funcs
    pub funcs: CG,
}

impl IR {
    pub fn new() -> Self {
        Self {
            globals: DFG::new(),
            funcs: CG::new(),
        }
    }
}

impl Default for IR {
    fn default() -> Self {
        Self::new()
    }
}
