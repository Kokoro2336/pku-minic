//! Optimizer of the compiler.

mod compaction;
mod dce;
mod gvn;
mod mem2reg;
mod remove_trivial_phi;
mod sccp;
mod simplify_cfg;
mod loop_simplify;
mod licm;

pub use compaction::*;
pub use dce::*;
pub use gvn::*;
pub use mem2reg::*;
pub use remove_trivial_phi::*;
pub use sccp::*;
pub use simplify_cfg::*;
pub use loop_simplify::*;
pub use licm::*;
