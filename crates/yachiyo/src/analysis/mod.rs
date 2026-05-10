//! Analysis framework for IR.

#[cfg(feature = "debug")]
use crate::debug::info;

mod alias;
mod call_graph;
mod dom;

pub use alias::*;
pub use call_graph::*;
pub use dom::*;

pub trait Analysis {
  type Input;
  type Output;

  fn name(&self) -> &str;
  fn new(input: Self::Input) -> Self;
  fn run(&mut self) -> Self::Output;
}

pub fn analyze<A: Analysis>(input: A::Input) -> A::Output {
  let mut analysis = A::new(input);

  #[cfg(feature = "debug")]

  info!("Running analysis: {}", analysis.name());
  let result = analysis.run();
  #[cfg(feature = "debug")]
  info!("Finished analysis: {}", analysis.name());

  result
}
