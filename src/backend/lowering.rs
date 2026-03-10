//! IR Lowering from Mid IR to Lower IR.

use crate::ir::lower::*;

use rustc_hash::FxHashMap;

/// TODO: In lowering we need to do:
/// 1. Lower the GEP
/// 2. Lower the function call (handle the argument passing and return value passing according to the calling convention)
/// 3. Phi Elimination (SSA to non-SSA)
/// 4. 
/// And Lowering should not has any ISA-specific transfromation except ABI adaptation.
pub struct Lowering {
	/// Temporary Map between BBId -> LBasicBlock
    block_map: Vec<LBasicBlock>,
	/// IR OpId -> VirtId.
	value_map: Vec<LOperand>,
	/// Move instruction buffer for Phi
	/// Edge(BBId, BBId) -> Move InstId.
	phi_moves: FxHashMap<(usize, usize), Vec<LOperand>>
}
