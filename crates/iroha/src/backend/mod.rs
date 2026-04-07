//! Assembly module

mod compaction;
mod isel;
mod lowering;
mod regalloc;
mod dce;
mod instcomb;
pub use compaction::*;
pub use isel::*;
pub use lowering::*;
pub use regalloc::*;
pub use dce::*;
pub use instcomb::*;
