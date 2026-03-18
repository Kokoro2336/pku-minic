pub mod analysis;
pub mod backend;
pub mod cli;
pub mod frontend;
pub mod opt;

// Import SysY parser.
#[allow(clippy::all)]
#[allow(clippy::extra_unused_lifetimes)]
#[allow(clippy::needless_lifetimes)]
#[allow(clippy::let_unit_value)]
#[allow(clippy::just_underscores_and_digits)]
pub mod sysy;
