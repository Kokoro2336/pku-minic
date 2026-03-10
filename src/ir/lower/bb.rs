use crate::utils::arena::*;

pub type LCFG = IndexedArena<LBasicBlock>;

#[derive(Debug, Clone)]
pub struct LBasicBlock {
    pub prev: Vec<usize>,
    pub cur: Vec<usize>,
}
