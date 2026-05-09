//! Backend.

mod canonicalize;
mod compaction;
mod dce;
mod isel;
mod legalize;
mod lowering;
mod peephole;
mod regalloc;
mod strength_reduct;
mod inst_comibine;

pub use canonicalize::*;
pub use compaction::*;
pub use dce::*;
pub use isel::*;
pub use legalize::*;
pub use lowering::*;
pub use peephole::*;
pub use regalloc::*;
pub use strength_reduct::*;
pub use inst_comibine::*;
