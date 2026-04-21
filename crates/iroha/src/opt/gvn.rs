//! Global Value Numbering (GVN) base on Dominator Tree Traversal.

use yachiyo::analysis::analyze;
use yachiyo::ir::mid::{Builder, Function, OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::table::SymbolTable;

use crate::analysis::{DomAnalysis, DomTree};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CanonicalExpr {
  // We don't consider defining separate operations for int and float.
  Add(Operand, Operand),
  Mul(Operand, Operand),
  Sub(Operand, Operand),
  Div(Operand, Operand),
  Mod(Operand, Operand),
  Xor(Operand, Operand),
  Shl(Operand, Operand),
  Shr(Operand, Operand),
  Sar(Operand, Operand),
  Eq(Operand, Operand),
  Ne(Operand, Operand),
  Lt(Operand, Operand),
  Le(Operand, Operand),
  Sitofp(Operand),
  Fptosi(Operand),
  Uitofp(Operand),
  Zext(Operand),
  /// Phi's operands are sorted by the block id.
  Phi(Vec<PhiIncoming>),

  // - Load produce a value, we don't consider it as memory is not constrained by SSA form.
  // - GEP is not to be canonicalized too.
  // TODO: When we can determine whether a function has side effects, we can add Call here.
  /// For other operations that we don't consider, we represent then as None.
  None,
}

enum GVNPhase {
  Start(Operand),
  End,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct GVN<'a> {
  ir: Option<&'a mut IR>,
  builder: Builder,
  symbols: SymbolTable<CanonicalExpr, Operand>,
  stack: Vec<GVNPhase>,
}

impl From<&OpData> for CanonicalExpr {
  fn from(op_data: &OpData) -> Self {
    let swap = |lhs: Operand, rhs: Operand| {
      if lhs < rhs {
        (lhs, rhs)
      } else {
        (rhs, lhs)
      }
    };
    match op_data {
      // Canonicalize commutative operations by sorting their operands.
      OpData::AddI { lhs, rhs } | OpData::AddF { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Add(lhs, rhs)
      }
      OpData::MulI { lhs, rhs } | OpData::MulF { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Mul(lhs, rhs)
      }
      OpData::Xor { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Xor(lhs, rhs)
      }
      OpData::Sar { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Sar(lhs, rhs)
      }
      OpData::SEq { lhs, rhs } | OpData::OEq { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Eq(lhs, rhs)
      }
      OpData::SNe { lhs, rhs } | OpData::ONe { lhs, rhs } => {
        let (lhs, rhs) = swap(*lhs, *rhs);
        CanonicalExpr::Ne(lhs, rhs)
      }

      // These operantions are not commutative, so we keep their operands in order.
      OpData::SubI { lhs, rhs } | OpData::SubF { lhs, rhs } => CanonicalExpr::Sub(*lhs, *rhs),
      OpData::DivI { lhs, rhs } | OpData::DivF { lhs, rhs } => CanonicalExpr::Div(*lhs, *rhs),
      OpData::SLt { lhs, rhs } | OpData::OLt { lhs, rhs } => CanonicalExpr::Lt(*lhs, *rhs),
      OpData::SLe { lhs, rhs } | OpData::OLe { lhs, rhs } => CanonicalExpr::Le(*lhs, *rhs),
      OpData::ModI { lhs, rhs } => CanonicalExpr::Mod(*lhs, *rhs),
      OpData::Shl { lhs, rhs } => CanonicalExpr::Shl(*lhs, *rhs),
      OpData::Shr { lhs, rhs } => CanonicalExpr::Shr(*lhs, *rhs),

      // We can canonicalize `>` and `>=` by swapping their operands and changing them to `<` and `<=`.
      OpData::SGt { lhs, rhs } | OpData::OGt { lhs, rhs } => CanonicalExpr::Lt(*rhs, *lhs),
      OpData::SGe { lhs, rhs } | OpData::OGe { lhs, rhs } => CanonicalExpr::Le(*rhs, *lhs),

      // These operations are unary, so we keep their operand as is.
      OpData::Sitofp { value } => CanonicalExpr::Sitofp(*value),
      OpData::Fptosi { value } => CanonicalExpr::Fptosi(*value),
      OpData::Uitofp { value } => CanonicalExpr::Uitofp(*value),
      OpData::Zext { value } => CanonicalExpr::Zext(*value),

      OpData::Phi { incomings } => {
        let mut sorted_incomings = incomings.clone();
        sorted_incomings.sort_by_key(|incoming| match incoming {
          PhiIncoming::Data { bb, .. } => *bb,
          PhiIncoming::None => unreachable!(),
        });
        CanonicalExpr::Phi(sorted_incomings)
      }

      // In GVN, GEP is not to be canonicalized.
      OpData::GEP { .. }
      | OpData::Alloca(_)
      | OpData::Declare { .. }
      | OpData::Load { .. }
      | OpData::Store { .. }
      | OpData::Call { .. }
      | OpData::Ret { .. }
      | OpData::Br { .. }
      | OpData::Jump { .. }
      | OpData::GlobalAlloca(_) => CanonicalExpr::None,
    }
  }
}

impl GVN<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: Operand) {
    self.builder.set_current_func(Some(func_id));
  }

  #[inline(always)]
  fn get_func(&self, func_id: Operand) -> &Function {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn replace_all_uses(&mut self, old: Operand, new: Operand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_all_uses(func_id, old, new);
  }

  #[inline(always)]
  fn enter_scope(&mut self, bb_id: Operand) {
    self.stack.push(GVNPhase::Start(bb_id));
    self.symbols.enter_scope();
  }

  fn run(&mut self, dom_tree: &DomTree) {
    let func_id = self.builder.current_function.unwrap();
    while let Some(phase) = self.stack.pop() {
      match phase {
        GVNPhase::Start(bb_id) => {
          let insts = self.get_func(func_id).cfg[bb_id.get_bb_id()].cur.clone();

          // Start GVN
          for inst in insts {
            let op_data = &self.get_func(func_id).dfg[inst.get_op_id()].data;
            if let OpData::GEP { base, indices } = op_data {
              if indices.iter().all(|index| matches!(index, Operand::Int(0))) {
                // We can canonicalize GEP with all zero indices to its base pointer.
                self.replace_all_uses(inst, *base);
                continue;
              }
            }

            // Canonicalize the instruction
            let canonical_expr: CanonicalExpr = op_data.into();

            // If it's not the target of GVN, we skip it.
            if canonical_expr == CanonicalExpr::None {
              continue;
            }

            if let Some(value) = self.symbols.get(&canonical_expr) {
              // Replace the instruction with the existing value.
              self.replace_all_uses(inst, *value);
            } else {
              // Insert the canonical expression into the symbol table.
              self.symbols.insert(canonical_expr, inst);
            }
          }

          // Push End phase to the stack
          self.stack.push(GVNPhase::End);

          // Update stack and symbol table
          let idoms = dom_tree[bb_id.get_bb_id()]
            .iter()
            .map(|&child| Operand::BB(child))
            .collect::<Vec<_>>();
          for idom in idoms {
            self.enter_scope(idom);
          }
        }
        GVNPhase::End => {
          // Exit current scope
          self.symbols.exit_scope();
        }
      }
    }
  }
}

impl<'a> Pass<'a> for GVN<'a> {
  fn name(&self) -> &'static str {
    "GVN"
  }

  fn mount(&mut self, program: &'a mut IR) {
    self.ir = Some(program);
  }

  fn run(&mut self) {
    // run dominance analysis to get the dominator tree
    let (dom_trees, _) = analyze::<DomAnalysis>(self.ir.as_ref().unwrap());
    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let dom_tree = &dom_trees[func_id];
      let func_id = Operand::Func(func_id);
      self.init(func_id);

      let entry = self.get_func(func_id).cfg.entry.unwrap();
      // Update stack and symbol table.
      self.enter_scope(Operand::BB(entry));
      self.run(dom_tree);
    }
  }
}
