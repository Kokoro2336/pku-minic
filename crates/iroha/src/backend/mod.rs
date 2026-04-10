//! Assembly module

mod compaction;
mod dce;
mod peephole;
mod isel;
mod lowering;
mod regalloc;
pub use compaction::*;
pub use dce::*;
pub use peephole::*;
pub use isel::*;
pub use lowering::*;
pub use regalloc::*;
