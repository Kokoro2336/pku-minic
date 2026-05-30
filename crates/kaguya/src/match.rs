//! Match-like generation. Based on If-let generation, and it doesn't require exhaustive pattern.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Block, Expr};

use crate::core::Pat;
use crate::if_let::GenIfLet;

pub struct GenMatch;

impl GenMatch {
  pub fn new() -> Self {
    Self
  }

  pub fn gen_branches(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    branches: &[(Pat, Block)],
  ) -> Vec<TokenStream2> {
    // Reuse GenIfLet to generate nested if-lets for each branch
    let mut gen_if_let = GenIfLet::new();
    branches
      .iter()
      .map(|(pat, body)| gen_if_let.gen_nested_ifs(cx, value.clone(), pat, quote!(#body)))
      .collect()
  }
}
