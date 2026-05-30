//! Kaguya crate root.

mod core;
mod if_let;
mod let_else;
mod r#match;

use core::{dispatch, KaguyaHimeInput};
use proc_macro::TokenStream;
use syn::parse_macro_input;

#[proc_macro]
pub fn kaguya_hime(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as KaguyaHimeInput);
  dispatch(input)
}
