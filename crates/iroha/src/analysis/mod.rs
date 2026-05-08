//! Analyses.

mod call_graph;
mod dom;
mod live;
mod r#loop;
mod reachable;

pub use call_graph::*;
pub use dom::*;
pub use live::*;
pub use r#loop::*;
pub use reachable::*;
