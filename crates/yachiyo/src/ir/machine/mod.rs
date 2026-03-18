//! Machine IR (MIR) definition.
mod module;
mod reg;
mod op;
mod r#type;
mod mem;
pub use module::*;
pub use op::*;
pub use reg::*;
pub use r#type::*;
pub use mem::*;
