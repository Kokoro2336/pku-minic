//! Let-else-like generation.

use crate::core::Pat;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Block, Expr, Ident};

pub struct GenLetElse {
  counter: usize,
}

impl GenLetElse {
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

      Pat::PhiIncoming(value_pat, bb_pat) => {
        let tmp_value = self.mangled_tmp("value");
        let tmp_bb = self.mangled_tmp("bb");

        stmts.push(quote! {
          let PhiIncoming::Data { value: #tmp_value, bb: #tmp_bb } = #value else #else_body;
        });

        self.gen_stmts(cx, quote!(#tmp_value), value_pat, else_body, stmts);
        self.gen_stmts(cx, quote!(#tmp_bb), bb_pat, else_body, stmts);
      }

      Pat::Operand { name, inner } => {
        if let Some(inner) = inner {
          let tmp = self.mangled_tmp("operand");
          stmts.push(quote! {
            let Operand::#name(#tmp) = #value else #else_body;
          });
          self.gen_stmts(cx, quote!(#tmp), inner, else_body, stmts);
        } else {
          stmts.push(quote! {
            let Operand::#name = #value else #else_body;
          });
        }
      }

      Pat::Literal(lit) => stmts.push(quote! {
        let #lit = #value else #else_body;
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

        "Phi" => {
          self.gen_phi(cx, value, name, operands, else_body, stmts);
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
      let &OpData::#name(ref #tmp) = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { value: Some(#value_tmp) } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { addr: #tmp1, value: #tmp2 } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { func: #tmp1, args: ref #tmp2 } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { lhs: #tmp1, rhs: #tmp2 } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { value: #tmp } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { addr: #tmp } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { base: #tmp_base, indices: ref #tmp_indices } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { cond: #tmp_cond, then_block: #tmp_then, else_block: #tmp_else } = #cx.get_op_data(#value) else #else_body;
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
      let &OpData::#name { target: #tmp_target } = #cx.get_op_data(#value) else #else_body;
    });

    self.gen_stmts(cx, quote!(#tmp_target), &operands[0], else_body, stmts);
  }

  pub fn gen_phi(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    else_body: &Block,
    stmts: &mut Vec<TokenStream2>,
  ) {
    assert!(operands.len() == 1);

    let incomings_tmp = self.mangled_tmp("incomings");

    stmts.push(quote! {
      let &OpData::#name { incomings: ref #incomings_tmp } = #cx.get_op_data(#value) else #else_body;
    });

    self.gen_stmts(cx, quote!(#incomings_tmp), &operands[0], else_body, stmts);
  }
}
