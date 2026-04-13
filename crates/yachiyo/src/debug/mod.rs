//! Debug utilities.

pub mod asm;
pub mod llvm;
pub use asm::DumpASM;
pub use llvm::DumpLLVM;

#[cfg(feature = "debug")]
pub mod log;
#[allow(unused)]
#[cfg(feature = "debug")]
pub use tracing::{debug, error, info, trace, warn};
