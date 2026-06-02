//! Optimizer of the compiler.

mod idiom;
mod pre;
mod utils;

mod compaction;
mod dce;
mod gcm;
mod gvn;
mod inlining;
mod lcssa;
mod licm;
mod loop_rotate;
mod loop_simplify;
mod lsr;
mod mem2reg;
mod reassociate;
mod remove_trivial_phi;
mod sccp;
mod simplify_cfg;
mod unrolling;
mod vectorize;

pub use idiom::*;
pub use pre::*;
pub use utils::*;

pub use compaction::*;
pub use dce::*;
pub use gcm::*;
pub use gvn::*;
pub use inlining::*;
pub use lcssa::*;
pub use licm::*;
pub use loop_rotate::*;
pub use loop_simplify::*;
pub use lsr::*;
pub use mem2reg::*;
pub use reassociate::*;
pub use remove_trivial_phi::*;
pub use sccp::*;
pub use simplify_cfg::*;
pub use unrolling::*;
pub use vectorize::*;
