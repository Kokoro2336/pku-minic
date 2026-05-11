//! Alias Analysis.

use yachiyo::analysis::{AliasResult, Analysis, CallGraph, MemLoc, RangeRelation};
use yachiyo::ir::mid::Operand;
use yachiyo::pass::PassContext;

pub struct AliasAnalysis<'cx, 'ir, 'cg> {
  cx: &'cx mut PassContext<'ir>,
  a: Operand,
  b: Operand,
  call_graph: &'cg CallGraph,
}

impl<'cx, 'ir, 'cg> Analysis for AliasAnalysis<'cx, 'ir, 'cg> {
  /// (memory_location_a, memory_location_b)
  type Input = (&'cx mut PassContext<'ir>, Operand, Operand, &'cg CallGraph);
  type Output = AliasResult;

  fn name() -> &'static str {
    "Alias Analysis"
  }

  fn new(input: Self::Input) -> Self {
    let (cx, a, b, call_graph) = input;
    Self {
      cx,
      a,
      b,
      call_graph,
    }
  }

  fn run(&mut self) -> Self::Output {
    let (mut a_mem_loc, mut b_mem_loc) = (
      self.cx.compute_mem_loc(self.a),
      self.cx.compute_mem_loc(self.b),
    );
    Self::alias_rec(self.cx, self.call_graph, &mut a_mem_loc, &mut b_mem_loc)
  }
}

impl AliasAnalysis<'_, '_, '_> {
  fn alias_rec(
    cx: &mut PassContext<'_>,
    call_graph: &CallGraph,
    a_mem_loc: &mut MemLoc,
    b_mem_loc: &mut MemLoc,
  ) -> AliasResult {
    let (a_base, b_base) = (a_mem_loc.base, b_mem_loc.base);

    // This check should go earlier than eq, or the analysis may treat unknown offset as eq.
    if matches!(a_base, Operand::Undefined) || matches!(b_base, Operand::Undefined) {
      return AliasResult::MayAlias;
    }

    if a_mem_loc == b_mem_loc {
      return AliasResult::MustAlias;
    }

    if a_base == b_base {
      return match RangeRelation::check(a_mem_loc, b_mem_loc) {
        RangeRelation::Disjoint => AliasResult::NoAlias,
        RangeRelation::Overlap | RangeRelation::Unknown => AliasResult::MayAlias,
      };
    }

    match (a_base, b_base) {
      (Operand::Global(_), Operand::Global(_)) => AliasResult::NoAlias,

      (Operand::Param(a_idx), Operand::Param(b_idx)) => {
        let callee_id = cx.current_func();
        let call_sites_info = call_graph.get_call_sites_by_callee(callee_id);
        let mut saw_no = false;
        let mut saw_must = false;

        for call_site_info in call_sites_info {
          let (caller_id, args) = (call_site_info.caller, &call_site_info.args);

          // Compute args' MemLoc, and then concat them with the offset of the original MemLoc to get the MemLoc of the arg.
          let (a_arg, b_arg) = (args[a_idx], args[b_idx]);
          let (mut a_arg_mem_loc, mut b_arg_mem_loc) = cx.with_current_func(caller_id, |cx| {
            (cx.compute_mem_loc(a_arg), cx.compute_mem_loc(b_arg))
          });
          a_arg_mem_loc.offset += &a_mem_loc.offset;
          b_arg_mem_loc.offset += &b_mem_loc.offset;

          let aa_res = cx.with_current_func(caller_id, |cx| {
            // Fuck borrow checker!
            let cx_ptr = cx as *mut PassContext<'_>;
            Self::alias_rec(
              unsafe { &mut *cx_ptr },
              call_graph,
              &mut a_arg_mem_loc,
              &mut b_arg_mem_loc,
            )
          });

          match aa_res {
            AliasResult::NoAlias => saw_no = true,
            AliasResult::MustAlias => saw_must = true,
            AliasResult::MayAlias => return AliasResult::MayAlias,
          }
          if saw_no && saw_must {
            return AliasResult::MayAlias;
          }
        }

        if saw_must {
          AliasResult::MustAlias
        } else if saw_no {
          AliasResult::NoAlias
        } else {
          AliasResult::MayAlias
        }
      }

      (Operand::Param(idx), Operand::Global(_)) | (Operand::Global(_), Operand::Param(idx)) => {
        let callee_id = cx.current_func();
        let call_sites_info = call_graph.get_call_sites_by_callee(callee_id);
        let mut saw_no = false;
        let mut saw_must = false;

        for call_site_info in call_sites_info {
          let (caller_id, args) = (call_site_info.caller, &call_site_info.args);
          let arg = args[idx];
          let mut arg_mem_loc = cx.with_current_func(caller_id, |cx| cx.compute_mem_loc(arg));

          arg_mem_loc.offset += if matches!(a_base, Operand::Param(_)) {
            &a_mem_loc.offset
          } else {
            &b_mem_loc.offset
          };

          let mut global_mem_loc = if matches!(a_base, Operand::Global(_)) {
            a_mem_loc.clone()
          } else {
            b_mem_loc.clone()
          };

          let aa_res = cx.with_current_func(caller_id, |cx| {
            let cx_ptr = cx as *mut PassContext<'_>;
            Self::alias_rec(
              unsafe { &mut *cx_ptr },
              call_graph,
              &mut arg_mem_loc,
              &mut global_mem_loc,
            )
          });

          match aa_res {
            AliasResult::NoAlias => saw_no = true,
            AliasResult::MustAlias => saw_must = true,
            AliasResult::MayAlias => return AliasResult::MayAlias,
          }

          if saw_no && saw_must {
            return AliasResult::MayAlias;
          }
        }

        if saw_must {
          AliasResult::MustAlias
        } else if saw_no {
          AliasResult::NoAlias
        } else {
          AliasResult::MayAlias
        }
      }

      // TODO: Value after computing memory location can only be Alloca
      (Operand::Param(_), Operand::Value(_)) | (Operand::Value(_), Operand::Param(_)) => {
        AliasResult::NoAlias
      }

      (Operand::Value(_), _) | (_, Operand::Value(_)) => AliasResult::MayAlias,

      _ => AliasResult::MayAlias,
    }
  }
}

pub fn alias(
  cx: &mut PassContext<'_>,
  a: Operand,
  b: Operand,
  call_graph: &CallGraph,
) -> AliasResult {
  AliasAnalysis::new((cx, a, b, call_graph)).run()
}
