//! IR Lowering from Mid IR to Lower IR.

use crate::ir::lower::*;

/// TODO: In lowering we need to do:
/// 1. Lower the GEP
/// 2. Lower the function call (handle the argument passing and return value passing according to the calling convention)
/// 3. Phi Elimination (SSA to non-SSA)
/// 4. 
/// And Lowering should not has any ISA-specific transfromation except ABI adaptation.
pub struct Lowering;
