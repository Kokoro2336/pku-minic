//! Assembly module

mod compaction;
mod dce;
mod instcomb;
mod isel;
mod lowering;
mod regalloc;
pub use compaction::*;
pub use dce::*;
pub use instcomb::*;
pub use isel::*;
pub use lowering::*;
pub use regalloc::*;
