//! Analysis framework for IR.

#[cfg(feature = "debug")]
use crate::debug::info;

mod dom;
mod call_graph;

pub use dom::*;
pub use call_graph::*;

pub trait Analysis<'a>: Default {
  type Input;
  type Output;

  fn name(&self) -> &str;
  fn mount(&mut self, input: &'a Self::Input);
  fn run(&mut self) -> Self::Output;
}

pub fn analyze<'a, A: Analysis<'a>>(input: &'a A::Input) -> A::Output {
  let mut analysis = A::default();
  analysis.mount(input);

  #[cfg(feature = "debug")]

  info!("Running analysis: {}", analysis.name());
  let result = analysis.run();
  #[cfg(feature = "debug")]
  info!("Finished analysis: {}", analysis.name());

  result
}
