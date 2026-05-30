use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use strum_macros::EnumDiscriminants;
use syn::{
  braced, bracketed, parenthesized,
  parse::{Parse, ParseStream, Result},
  Block, Expr, Ident, LitBool, LitInt, Token,
};

use crate::if_let::GenIfLet;
use crate::let_else::GenLetElse;
use crate::r#match::GenMatch;

#[derive(EnumDiscriminants)]
#[strum_discriminants(name(InputType))]
#[strum_discriminants(derive(Hash, Ord, PartialOrd))]
pub enum KaguyaHimeInput {
  /// # Syntax
  /// ```ignore
  /// kaguya_hime!(
  ///   self.cx,
  ///   Add(Mul($a, $b), $c) in some_value else {
  ///     // diverged else body
  ///   }
  /// )
  /// ```
  LetElse {
    cx: Expr,
    value: Expr,
    pattern: Pat,
    else_body: Block,
  },
  /// # Syntax
  /// ```ignore
  /// kaguya_hime!(
  ///   self.cx,
  ///   match some_value {
  ///     Add($a, $b) => { /* branch 1 */ },
  ///     Mul($a, $b) => { /* branch 2 */ },
  ///   }
  /// )
  /// ```
  Match {
    cx: Expr,
    value: Expr,
    branches: Vec<(Pat, Block)>,
  },
  /// # Syntax
  /// ```ignore
  /// kaguya_hime!(
  ///   self.cx,
  ///   if Add($a, $b) in some_value {
  ///     /* then body */
  ///   }
  /// )
  /// ```
  IfLet {
    cx: Expr,
    value: Expr,
    pattern: Pat,
    then_body: Block,
  },
}

impl Parse for KaguyaHimeInput {
  fn parse(input: ParseStream) -> Result<Self> {
    // Parse Context first.
    let cx = input.parse()?;
    input.parse::<Token![,]>()?;

    // Match
    if input.peek(syn::token::Match) {
      input.parse::<syn::token::Match>()?;
      let value = input.call(Expr::parse_without_eager_brace)?;

      let content;
      braced!(content in input);

      let mut branches = Vec::new();
      while !content.is_empty() {
        let pattern = content.parse()?;
        content.parse::<Token![=>]>()?;
        let body = content.parse()?;
        branches.push((pattern, body));
        if content.peek(Token![,]) {
          content.parse::<Token![,]>()?;
        } else {
          break;
        }
      }
      return Ok(Self::Match {
        cx,
        value,
        branches,
      });
    }

    // IfLet
    if input.peek(syn::token::If) {
      input.parse::<syn::token::If>()?;
      let pattern = input.parse()?;
      input.parse::<syn::token::In>()?;
      let value = input.call(Expr::parse_without_eager_brace)?;
      let then_body = input.parse()?;

      return Ok(Self::IfLet {
        cx,
        value,
        pattern,
        then_body,
      });
    }

    // LetElse
    let pattern = input.parse()?;
    input.parse::<syn::token::In>()?;
    let value = input.call(Expr::parse_without_eager_brace)?;
    input.parse::<syn::token::Else>()?;
    let else_body = input.parse()?;

    Ok(Self::LetElse {
      cx,
      value,
      pattern,
      else_body,
    })
  }
}

pub fn dispatch(input: KaguyaHimeInput) -> TokenStream {
  match input {
    KaguyaHimeInput::LetElse {
      cx,
      value,
      pattern,
      else_body,
    } => {
      let mut gen = GenLetElse::new();
      let mut stmts = Vec::new();

      gen.gen_stmts(&cx, quote!(#value), &pattern, &else_body, &mut stmts);

      quote! {
        #(#stmts)*
      }
      .into()
    }
    KaguyaHimeInput::Match {
      cx,
      value,
      branches,
    } => {
      let mut gen = GenMatch::new();
      let ifs = gen.gen_branches(&cx, quote!(#value), &branches);
      quote! {
        #(#ifs)*
      }
      .into()
    }
    KaguyaHimeInput::IfLet {
      cx,
      value,
      pattern,
      then_body,
    } => {
      let mut gen = GenIfLet::new();
      let if_let = gen.gen_nested_ifs(&cx, quote!(#value), &pattern, quote!(#then_body));
      quote! {
        #if_let
      }
      .into()
    }
  }
}

pub enum Pat {
  Wildcard,
  DotDot,
  Bind(Ident),
  Op {
    name: Ident,
    operands: Vec<Pat>,
  },
  List(Vec<Pat>),

  /// Operands
  Operand {
    name: Ident,
    inner: Option<Box<Pat>>,
  },

  Literal(Literal),
}

pub enum Literal {
  Int(LitInt),
  Bool(LitBool),
}

impl ToTokens for Literal {
  fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
    match self {
      Literal::Int(lit) => lit.to_tokens(tokens),
      Literal::Bool(lit) => lit.to_tokens(tokens),
    }
  }
}

impl Parse for Pat {
  fn parse(input: ParseStream) -> Result<Self> {
    if input.peek(LitInt) {
      return Ok(Self::Literal(Literal::Int(input.parse()?)));
    }

    if input.peek(LitBool) {
      return Ok(Self::Literal(Literal::Bool(input.parse()?)));
    }

    // Match _ as a wildcard.
    if input.peek(Token![_]) {
      input.parse::<Token![_]>()?;
      return Ok(Self::Wildcard);
    }

    // Match single binded ident.
    if input.peek(Token![$]) {
      input.parse::<Token![$]>()?;
      let ident: Ident = input.parse()?;
      return Ok(Self::Bind(ident));
    }

    // Match .. as a dot-dot pattern.
    if input.peek(Token![..]) {
      input.parse::<Token![..]>()?;
      return Ok(Self::DotDot);
    }

    // Match [] in GEP/Phi.
    if input.peek(syn::token::Bracket) {
      let content;
      bracketed!(content in input);

      let mut elems = Vec::new();
      while !content.is_empty() {
        elems.push(content.parse()?);
        if content.peek(Token![,]) {
          content.parse::<Token![,]>()?;
        } else {
          break;
        }
      }
      return Ok(Self::List(elems));
    }

    // Match operator or Operand.
    let name: Ident = input.parse()?;

    if is_unit_operand(&name) && !input.peek(syn::token::Paren) {
      return Ok(Self::Operand { name, inner: None });
    }

    let content;
    parenthesized!(content in input);

    if is_operand(&name) {
      let inner = content.parse()?;
      if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
      }
      if !content.is_empty() {
        return Err(content.error("operand patterns take exactly one inner pattern"));
      }
      return Ok(Self::Operand {
        name,
        inner: Some(Box::new(inner)),
      });
    }

    let mut operands = Vec::new();
    while !content.is_empty() {
      operands.push(content.parse()?);
      if content.peek(Token![,]) {
        content.parse::<Token![,]>()?;
      } else {
        break;
      }
    }

    Ok(Self::Op { name, operands })
  }
}

fn is_operand(name: &Ident) -> bool {
  matches!(
    name.to_string().as_str(),
    "Global" | "Func" | "BB" | "Value" | "Param" | "Int" | "Float" | "Bool"
  )
}

fn is_unit_operand(name: &Ident) -> bool {
  name == "Undefined"
}
