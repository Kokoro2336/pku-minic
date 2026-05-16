//! Analyses.

mod alias;
mod call_graph;
mod dom;
mod live;
mod r#loop;
mod pureness;
mod reachable;
mod scc;
mod scev;

pub use alias::*;
pub use call_graph::*;
pub use dom::*;
pub use live::*;
pub use pureness::*;
pub use r#loop::*;
pub use reachable::*;
pub use scc::*;
pub use scev::*;
