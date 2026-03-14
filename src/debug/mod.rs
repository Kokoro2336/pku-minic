//! Debug utilities.

pub mod llvm;
pub mod log;
pub use llvm::DumpLLVM;
#[allow(unused)]
pub use tracing::{debug, error, info, trace, warn};
