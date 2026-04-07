//! Analysis framework for IR.

use crate::debug::info;

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

    info!("Running analysis: {}", analysis.name());
    let result = analysis.run();
    info!("Finished analysis: {}", analysis.name());

    result
}
