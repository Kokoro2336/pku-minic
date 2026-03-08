use crate::ir::mid::{CG, DFG};

#[derive(Debug, Clone)]
pub struct MidIR {
    // Including:
    // 1. global variables
    // 2. SysY library functions
    pub globals: DFG,
    // global funcs
    pub funcs: CG,
}

impl MidIR {
    pub fn new() -> Self {
        Self {
            globals: DFG::new(),
            funcs: CG::new(),
        }
    }
}

impl Default for MidIR {
    fn default() -> Self {
        Self::new()
    }
}
