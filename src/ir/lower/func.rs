//! LFunction definition.

use crate::ir::machine::MType;
use crate::ir::lower::{LCFG, LDFG, VirtReg};
use crate::utils::arena::*;

pub type VRegs = IndexedArena<VirtReg>;
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub typ: MType,
    pub cfg: LCFG,
    pub dfg: LDFG,
    /// Virtual registers used in this function.
    /// Distinct from MidIR, the virtual register is represented as a separate entity
    /// rather than the instruction id like MidIR.
    pub vregs: VRegs,
}
