//! Analyses.

mod alias;
mod call_graph;
mod dom;
mod live;
mod r#loop;
mod reachable;
mod scc;
mod pureness;

pub use alias::*;
pub use call_graph::*;
pub use dom::*;
pub use live::*;
pub use r#loop::*;
pub use reachable::*;
pub use scc::*;
pub use pureness::*;
