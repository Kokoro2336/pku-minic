//! Debug utilities.

pub mod asm;
pub mod llvm;
pub mod log;
pub use asm::DumpASM;
pub use llvm::DumpLLVM;
#[allow(unused)]
pub use tracing::{debug, error, info, trace, warn};
