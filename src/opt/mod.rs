mod compaction;
mod dce;
mod mem2reg;
mod remove_trivial_phi;
mod sccp;
mod pass;
// mod simplify_cfg;
pub use {compaction::*, dce::*, mem2reg::*, remove_trivial_phi::*, sccp::*, pass::*};
