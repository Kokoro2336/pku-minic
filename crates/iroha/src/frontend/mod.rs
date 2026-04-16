//! Frontend of the compiler.

mod parse;
mod emit;
mod semantic;
pub use emit::Emit;
pub use parse::Parser;
pub use semantic::Semantic;
