//! Backend.

mod canonicalize;
mod compaction;
mod dce;
mod isel;
mod lowering;
mod peephole;
mod regalloc;
pub use canonicalize::*;
pub use compaction::*;
pub use dce::*;
pub use isel::*;
pub use lowering::*;
pub use peephole::*;
pub use regalloc::*;
