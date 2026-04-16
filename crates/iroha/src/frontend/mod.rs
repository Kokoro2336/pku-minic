//! Frontend of the compiler.

mod emit;
mod parse;
mod semantic;
pub use emit::Emit;
pub use parse::Parser;
pub use semantic::Semantic;
