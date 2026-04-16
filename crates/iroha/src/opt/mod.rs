//! Optimizer of the compiler.

mod compaction;
mod dce;
mod mem2reg;
mod remove_trivial_phi;
mod sccp;
// mod simplify_cfg;

pub use compaction::*;
pub use dce::*;
pub use mem2reg::*;
pub use remove_trivial_phi::*;
pub use sccp::*;
// pub use simplify_cfg::*;
