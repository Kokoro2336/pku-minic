//! Call Graph Analysis.

use crate::ir::mid::Operand;

pub struct CallGraph {
  /// Callers of each function.
  pub callers: Vec<Vec<Operand>>,
  /// Callees of each function.
  pub callees: Vec<Vec<Operand>>,
}
