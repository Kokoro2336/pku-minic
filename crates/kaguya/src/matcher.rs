use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
  bracketed, parenthesized,
  parse::{Parse, ParseStream, Result},
  Block, Expr, Ident, Token,
};

pub struct KaguyaHimeInput {
  cx: Expr,
  value: Expr,
  pattern: Pat,
  body: Block,
  else_body: Block,
}

impl Parse for KaguyaHimeInput {
  fn parse(input: ParseStream) -> Result<Self> {
    let cx = input.parse()?;
    input.parse::<Token![,]>()?;
    let value = input.parse()?;
    input.parse::<Token![,]>()?;
    let pattern = input.parse()?;
    input.parse::<syn::token::FatArrow>()?;
    let body = input.parse()?;
    input.parse::<syn::token::Else>()?;
    let else_body = input.parse()?;

    Ok(Self {
      cx,
      value,
      pattern,
      body,
      else_body,
    })
  }
}

pub fn expand(input: KaguyaHimeInput) -> TokenStream {
  let KaguyaHimeInput {
    cx,
    value,
    pattern,
    body,
    else_body,
  } = input;

  let mut gen = Gen::new();
  let mut stmts = Vec::new();

  gen.gen_stmts(&cx, quote!(#value), &pattern, &else_body, &mut stmts);

  quote! {
    #(#stmts)*
    #body
  }
  .into()
}

enum Pat {
  Wildcard,
  DotDot,
  Bind(Ident),
  Op { name: Ident, operands: Vec<Pat> },
  List(Vec<Pat>),
}

impl Parse for Pat {
  fn parse(input: ParseStream) -> Result<Self> {
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

    // Match operator.
    let name: Ident = input.parse()?;

    let content;
    parenthesized!(content in input);

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

struct Gen {
  counter: usize,
}

impl Gen {
  pub fn new() -> Self {
    Self { counter: 0 }
  }

  pub fn mangled_tmp(&mut self, name: &str) -> Ident {
    let ident = format_ident!("__{}_{}", name, self.counter);
    self.counter += 1;
    ident
  }

  pub fn gen_stmts(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    pat: &Pat,
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    match pat {
      Pat::Bind(ident) => stmts.push(quote! {
        let #ident = #value;
      }),

      Pat::Wildcard => stmts.push(quote! {
        let _ = #value;
      }),

      Pat::DotDot => { /* Dot-Dot is handled inside gen_elems() */ }

      Pat::List(elems) => self.gen_elems(cx, value, elems, else_body, stmts),

      Pat::Op { name, operands } => match name.to_string().as_str() {
        "AddI" | "SubI" | "MulI" | "DivI" | "ModI" | "AddF" | "SubF" | "MulF" | "DivF" | "SLe"
        | "SLt" | "SGe" | "SGt" | "SEq" | "SNe" | "OLe" | "OLt" | "OGe" | "OGt" | "OEq" | "ONe"
        | "Xor" | "Shl" | "Shr" | "Sar" => {
          self.gen_binop(cx, value, name, operands, else_body, stmts);
        }

        "Sitofp" | "Fptosi" | "Zext" | "Uitofp" => {
          self.gen_unop(cx, value, name, operands, else_body, stmts);
        }

        "Load" => {
          self.gen_load(cx, value, name, operands, else_body, stmts);
        }

        "GEP" => {
          self.gen_gep(cx, value, name, operands, else_body, stmts);
        }

        "Br" => {
          self.gen_br(cx, value, name, operands, else_body, stmts);
        }

        "Jump" => {
          self.gen_jump(cx, value, name, operands, else_body, stmts);
        }

        "Call" => {
          self.gen_call(cx, value, name, operands, else_body, stmts);
        }

        "Store" => {
          self.gen_store(cx, value, name, operands, else_body, stmts);
        }

        "Alloca" => {
          self.gen_alloca(cx, value, name, operands, else_body, stmts);
        }

        // TODO: For now we only match Ret with an explicit return value.
        "Ret" => {
          self.gen_ret(cx, value, name, operands, else_body, stmts);
        }

        other => {
          unimplemented!("unsupported pattern op: {other}");
        }
      },
    }
  }

  pub fn gen_elems(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    elems: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    let mut has_dot_dot = false;
    let expanded_elems = elems
      .iter()
      .enumerate()
      .map(|(id, pat)| match pat {
        Pat::DotDot => {
          has_dot_dot = true;
          None
        }
        _ => Some(self.mangled_tmp(format!("elem{}", id).as_str())),
      })
      .collect::<Vec<_>>();
    let compacted_expanded_elems = expanded_elems
      .iter()
      .filter_map(|e| e.clone())
      .collect::<Vec<_>>();

    let quote = if has_dot_dot {
      quote! {
        let &[#(#compacted_expanded_elems,)* ..] = #value.as_slice() else #else_body;
      }
    } else {
      quote! {
        let &[#(#compacted_expanded_elems, )*] = #value.as_slice() else #else_body;
      }
    };

    stmts.push(quote);

    for (i, elem) in elems.iter().enumerate() {
      let Some(expanded) = &expanded_elems[i] else {
        continue;
      };
      self.gen_stmts(cx, quote!(#expanded), elem, else_body, stmts);
    }
  }

  pub fn gen_alloca(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("ty");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name(#tmp) = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp), &operands[0], else_body, stmts);
  }

  pub fn gen_ret(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let value_tmp = self.mangled_tmp("value");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { value: Some(#value_tmp) } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#value_tmp), &operands[0], else_body, stmts);
  }

  pub fn gen_store(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("addr");
    let tmp2 = self.mangled_tmp("value");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { addr: #tmp1, value: #tmp2 } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp1), &operands[0], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp2), &operands[1], else_body, stmts);
  }

  pub fn gen_call(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("func");
    let tmp2 = self.mangled_tmp("args");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { func: #tmp1, args: #tmp2 } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp1), &operands[0], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp2), &operands[1], else_body, stmts);
  }

  pub fn gen_binop(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("lhs");
    let tmp2 = self.mangled_tmp("rhs");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { lhs: #tmp1, rhs: #tmp2 } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp1), &operands[0], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp2), &operands[1], else_body, stmts);
  }

  pub fn gen_unop(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("value");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { value: #tmp } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp), &operands[0], else_body, stmts);
  }

  pub fn gen_load(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("addr");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { addr: #tmp } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp), &operands[0], else_body, stmts);
  }

  pub fn gen_gep(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 2);

    let tmp_base = self.mangled_tmp("base");
    let tmp_indices = self.mangled_tmp("indices");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { base: #tmp_base, indices: #tmp_indices } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp_base), &operands[0], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp_indices), &operands[1], else_body, stmts);
  }

  pub fn gen_br(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 3);

    let tmp_cond = self.mangled_tmp("cond");
    let tmp_then = self.mangled_tmp("then_bb");
    let tmp_else = self.mangled_tmp("else_bb");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { cond: #tmp_cond, then_block: #tmp_then, else_block: #tmp_else } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp_cond), &operands[0], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp_then), &operands[1], else_body, stmts);
    self.gen_stmts(cx, quote!(#tmp_else), &operands[2], else_body, stmts);
  }

  pub fn gen_jump(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let tmp_target = self.mangled_tmp("target");

    stmts.push(quote! {
      let op_data = #cx.get_op_data(#value).clone();

      let OpData::#name { target: #tmp_target } = op_data else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp_target), &operands[0], else_body, stmts);
  }
}
