//! Backend.

mod branch_folding;
mod canonicalize;
mod compaction;
mod dce;
mod inst_comibine;
mod isel;
mod legalize;
mod lowering;
mod peephole;
mod regalloc;
mod strength_reduct;

pub use branch_folding::*;
pub use canonicalize::*;
pub use compaction::*;
pub use dce::*;
pub use inst_comibine::*;
pub use isel::*;
pub use legalize::*;
pub use lowering::*;
pub use peephole::*;
pub use regalloc::*;
pub use strength_reduct::*;
