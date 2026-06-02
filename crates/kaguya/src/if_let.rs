//! If-let-like generation.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Expr, Ident};

use crate::core::Pat;

pub struct GenIfLet {
  counter: usize,
}

impl GenIfLet {
  pub fn new() -> Self {
    Self { counter: 0 }
  }

  fn mangled_tmp(&mut self, name: &str) -> Ident {
    let ident = format_ident!("__{}_{}", name, self.counter);
    self.counter += 1;
    ident
  }

  pub fn gen_nested_ifs(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    pat: &Pat,
    body: TokenStream2,
  ) -> TokenStream2 {
    match pat {
      Pat::Bind(ident, pattern) => {
        let ident_quote = quote! {
          let #ident = #value;
          #body
        };
        if let Some(pattern) = pattern {
          self.gen_nested_ifs(cx, quote!(#value), pattern, quote!(#ident_quote))
        } else {
          ident_quote
        }
      }

      Pat::Or(pats) => {
        let ifs = pats
          .iter()
          .map(|pat| self.gen_nested_ifs(cx, quote!(#value), pat, quote!(#body)));
        // Generate if {} else if {} else if {} ...
        quote! {
          #(#ifs) else *
        }
      }

      Pat::PhiIncoming(value_pat, bb_pat) => {
        let tmp_value = self.mangled_tmp("value");
        let tmp_bb = self.mangled_tmp("bb");

        let tmp_bb_block = self.gen_nested_ifs(cx, quote!(#tmp_bb), bb_pat, quote!(#body));
        let tmp_value_block = self.gen_nested_ifs(cx, quote!(#tmp_value), value_pat, tmp_bb_block);

        quote! {
          if let PhiIncoming::Data { value: #tmp_value, bb: #tmp_bb } = #value {
            #tmp_value_block
          }
        }
      }

      Pat::Operand { name, inner } => {
        if let Some(inner) = inner {
          let tmp = self.mangled_tmp("operand");
          let inner_block = self.gen_nested_ifs(cx, quote!(#tmp), inner, quote!(#body));
          quote! {
            if let Operand::#name(#tmp) = #value {
              #inner_block
            }
          }
        } else {
          quote! {
            if let Operand::#name = #value {
              #body
            }
          }
        }
      }

      Pat::Literal(lit) => quote! {
        if #value == #lit {
          #body
        }
      },

      Pat::Wildcard => quote! {
        let _ = #value;
        #body
      },

      Pat::DotDot => unreachable!("Dot-Dot should be handled in gen_elems()"),

      Pat::List(elems) => self.gen_elems(cx, value, elems, body),

      Pat::Op { name, operands } => {
        let op_quote = match name.to_string().as_str() {
          "AddI" | "SubI" | "MulI" | "DivI" | "ModI" | "AddF" | "SubF" | "MulF" | "DivF"
          | "SLe" | "SLt" | "SGe" | "SGt" | "SEq" | "SNe" | "OLe" | "OLt" | "OGe" | "OGt"
          | "OEq" | "ONe" | "Xor" | "Shl" | "Shr" | "Sar" => {
            self.gen_binop(cx, value.clone(), name, operands, body)
          }

          "Sitofp" | "Fptosi" | "Zext" | "Uitofp" => {
            self.gen_unop(cx, value.clone(), name, operands, body)
          }

          "Load" => self.gen_load(cx, value.clone(), name, operands, body),

          "GEP" => self.gen_gep(cx, value.clone(), name, operands, body),

          "Br" => self.gen_br(cx, value.clone(), name, operands, body),

          "Jump" => self.gen_jump(cx, value.clone(), name, operands, body),

          "Call" => self.gen_call(cx, value.clone(), name, operands, body),

          "Store" => self.gen_store(cx, value.clone(), name, operands, body),

          "Alloca" => self.gen_alloca(cx, value.clone(), name, operands, body),

          "Phi" => self.gen_phi(cx, value.clone(), name, operands, body),

          // TODO: For now we only match Ret with an explicit return value.
          "Ret" => self.gen_ret(cx, value.clone(), name, operands, body),

          other => {
            unimplemented!("unsupported pattern op: {other}");
          }
        };

        quote! {
          if let Operand::Value(_) = #value {
            #op_quote
          }
        }
      }
    }
  }

  pub fn gen_elems(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    elems: &[Pat],
    block: TokenStream2,
  ) -> TokenStream2 {
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

    let quote = elems
      .iter()
      .enumerate()
      .rev()
      .fold(block, |acc, (i, elem)| {
        let Some(expanded) = &expanded_elems[i] else {
          return acc;
        };
        self.gen_nested_ifs(cx, quote!(#expanded), elem, acc)
      });

    let compacted_expanded_elems = expanded_elems
      .iter()
      .filter_map(|e| e.clone())
      .collect::<Vec<_>>();

    if has_dot_dot {
      quote! {
        if let &[#(#compacted_expanded_elems,)* ..] = #value.as_slice() {
          #quote
        }
      }
    } else {
      quote! {
        if let &[#(#compacted_expanded_elems, )*] = #value.as_slice() {
          #quote
        }
      }
    }
  }

  pub fn gen_binop(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("lhs");
    let tmp2 = self.mangled_tmp("rhs");

    let tmp2_block = self.gen_nested_ifs(cx, quote!(#tmp2), &operands[1], quote!(#body));
    let tmp1_block = self.gen_nested_ifs(cx, quote!(#tmp1), &operands[0], tmp2_block);

    quote! {
      if let &OpData::#name { lhs: #tmp1, rhs: #tmp2 } = #cx.get_op_data(#value) {
        #tmp1_block
      }
    }
  }

  pub fn gen_unop(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("value");

    let tmp_block = self.gen_nested_ifs(cx, quote!(#tmp), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name { value: #tmp } = #cx.get_op_data(#value) {
        #tmp_block
      }
    }
  }

  pub fn gen_load(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("addr");

    let tmp_block = self.gen_nested_ifs(cx, quote!(#tmp), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name { addr: #tmp } = #cx.get_op_data(#value) {
        #tmp_block
      }
    }
  }

  pub fn gen_gep(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 2);

    let tmp_ptr = self.mangled_tmp("base");
    let tmp_indices = self.mangled_tmp("indices");

    let indices_block = self.gen_nested_ifs(cx, quote!(#tmp_indices), &operands[1], quote!(#body));
    let base_block = self.gen_nested_ifs(cx, quote!(#tmp_ptr), &operands[0], indices_block);

    quote! {
      if let &OpData::#name { base: #tmp_ptr, indices: ref #tmp_indices } = #cx.get_op_data(#value) {
        #base_block
      }
    }
  }

  pub fn gen_alloca(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let tmp = self.mangled_tmp("ty");

    let tmp_block = self.gen_nested_ifs(cx, quote!(#tmp), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name(ref #tmp) = #cx.get_op_data(#value) {
        #tmp_block
      }
    }
  }

  pub fn gen_ret(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let value_tmp = self.mangled_tmp("value");

    let value_block = self.gen_nested_ifs(cx, quote!(#value_tmp), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name { value: Some(#value_tmp) } = #cx.get_op_data(#value) {
        #value_block
      }
    }
  }

  pub fn gen_store(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("addr");
    let tmp2 = self.mangled_tmp("value");

    let value_block = self.gen_nested_ifs(cx, quote!(#tmp2), &operands[1], quote!(#body));
    let addr_block = self.gen_nested_ifs(cx, quote!(#tmp1), &operands[0], value_block);

    quote! {
      if let &OpData::#name { addr: #tmp1, value: #tmp2 } = #cx.get_op_data(#value) {
        #addr_block
      }
    }
  }

  pub fn gen_call(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 2);

    let tmp1 = self.mangled_tmp("func");
    let tmp2 = self.mangled_tmp("args");

    let args_block = self.gen_nested_ifs(cx, quote!(#tmp2), &operands[1], quote!(#body));
    let func_block = self.gen_nested_ifs(cx, quote!(#tmp1), &operands[0], args_block);

    quote! {
      if let &OpData::#name { func: #tmp1, args: ref #tmp2 } = #cx.get_op_data(#value) {
        #func_block
      }
    }
  }

  pub fn gen_br(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 3);

    let tmp_cond = self.mangled_tmp("cond");
    let tmp_then = self.mangled_tmp("then_bb");
    let tmp_else = self.mangled_tmp("else_bb");

    let else_block = self.gen_nested_ifs(cx, quote!(#tmp_else), &operands[2], quote!(#body));
    let then_block = self.gen_nested_ifs(cx, quote!(#tmp_then), &operands[1], else_block);
    let cond_block = self.gen_nested_ifs(cx, quote!(#tmp_cond), &operands[0], then_block);

    quote! {
      if let &OpData::#name { cond: #tmp_cond, then_bb: #tmp_then, else_bb: #tmp_else } = #cx.get_op_data(#value) {
        #cond_block
      }
    }
  }

  pub fn gen_jump(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let tmp_target = self.mangled_tmp("target");

    let target_block = self.gen_nested_ifs(cx, quote!(#tmp_target), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name { target_bb: #tmp_target } = #cx.get_op_data(#value) {
        #target_block
      }
    }
  }

  pub fn gen_phi(
    &mut self,
    cx: &Expr,
    value: TokenStream2,
    name: &Ident,
    operands: &[Pat],
    body: TokenStream2,
  ) -> TokenStream2 {
    assert!(operands.len() == 1);

    let incomings_tmp = self.mangled_tmp("incomings");

    let incomings_tmp_block =
      self.gen_nested_ifs(cx, quote!(#incomings_tmp), &operands[0], quote!(#body));

    quote! {
      if let &OpData::#name { incomings: ref #incomings_tmp } = #cx.get_op_data(#value) {
        #incomings_tmp_block
      }
    }
  }
}
