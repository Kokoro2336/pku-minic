//! Global Value Numbering (GVN) .

use yachiyo::analysis::{analyze, AliasResult, CallGraph, Pureness, PurenessResult};
use yachiyo::ir::mid::{OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::BitSet;
use yachiyo::utils::SymbolTable;

use crate::analysis::{
  alias, CallGraphAnalysis, DomAnalysis, DomTree, PurenessAnalysis, SCCAnalysis,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CanonicalExpr {
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
  #[allow(clippy::upper_case_acronyms)]
  GEP(Operand, Vec<Operand>),
  Call(Operand, Vec<Operand>),

  // TODO: When we can determine whether a function has side effects, we can add Call here.
  /// For other operations that we don't consider, we represent then as None.
  None,
}

enum GVNPhase {
  /// BasicBlock
  Start(Operand),
  End,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct GVN<'a> {
  cx: PassContext<'a>,
  symbols: SymbolTable<CanonicalExpr, Operand>,
  stack: Vec<GVNPhase>,

  visited: BitSet,
  dfs_post_order: Vec<Operand>,
  /// BBId -> Reveresed Post-Order DFS number
  rdfn: Vec<usize>,
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
      OpData::Sar { lhs, rhs } => CanonicalExpr::Sar(*lhs, *rhs),

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

      OpData::GEP { base, indices } => CanonicalExpr::GEP(*base, indices.clone()),
      OpData::Call { func, args } => CanonicalExpr::Call(*func, args.clone()),

      OpData::Alloca(_)
      | OpData::Declare { .. }
      | OpData::Load { .. }
      | OpData::Store { .. }
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
    self.cx.set_current_func(Some(func_id));

    self.symbols.clear();
    self.stack.clear();
    self.visited.clear();
    self.dfs_post_order.clear();

    self.rdfn.clear();
    self.rdfn.resize(self.cx.get_func(func_id).cfg.len(), 0);
  }

  fn dfs(&mut self, bb_id: Operand) {
    if self.visited.contains(bb_id.get_bb_id()) {
      return;
    }

    self.visited.insert(bb_id.get_bb_id());

    let succs = self.cx.get_bb(bb_id).succs.clone();
    for (succ, _) in succs {
      self.dfs(succ);
    }

    // Post-order traversal.
    self.dfs_post_order.push(bb_id);
  }

  #[inline(always)]
  fn enter_scope(&mut self, bb_id: Operand) {
    self.stack.push(GVNPhase::Start(bb_id));
    self.symbols.enter_scope();
  }

  fn forward(
    &mut self,
    call_graph: &CallGraph,
    mem_entries: &[Operand],
    load_id: Operand,
    pureness: &PurenessResult,
  ) -> Option<Operand> {
    let OpData::Load { addr: origin } = self.cx.get_op_data(load_id).clone() else {
      unreachable!()
    };

    for &mem_entry in mem_entries.iter().rev() {
      match self.cx.get_op_data(mem_entry).clone() {
        OpData::Load { addr } => {
          let res = alias(&mut self.cx, addr, origin, call_graph);
          match res {
            AliasResult::MustAlias => return Some(mem_entry),
            AliasResult::NoAlias => continue,
            AliasResult::MayAlias => return None,
          }
        }
        OpData::Store { addr, value } => {
          let res = alias(&mut self.cx, addr, origin, call_graph);
          match res {
            AliasResult::MustAlias => return Some(value),
            AliasResult::NoAlias => continue,
            AliasResult::MayAlias => return None,
          }
        }
        OpData::Call { func, .. } => {
          if pureness[func] == Pureness::Impure {
            return None;
          } else {
            continue;
          }
        }
        _ => unreachable!(),
      }
    }

    None
  }

  fn run(&mut self, dom_tree: &DomTree, call_graph: &CallGraph, pureness: &PurenessResult) {
    while let Some(phase) = self.stack.pop() {
      match phase {
        GVNPhase::Start(bb_id) => {
          // Local store-load forwaring
          let mut mem_entries = vec![];
          let insts = self.cx.get_bb(bb_id).cur.clone();

          // Start GVN
          for inst in insts {
            let op_data = self.cx.get_op_data(inst);
            match op_data {
              OpData::GEP { base, indices } => {
                if indices.len() == 1
                  && indices.iter().all(|index| matches!(index, Operand::Int(0)))
                {
                  // We can canonicalize GEP with single zero indices to its base pointer.
                  // If indices.len() > 1, we can't eliminate it since replacement would iccur type mismatch.
                  self.cx.replace_all_uses(inst, *base);
                  continue;
                }
              }
              OpData::Store { .. } => {
                mem_entries.push(inst);
                continue;
              }
              OpData::Load { .. } => {
                let forwarded = self.forward(call_graph, &mem_entries, inst, pureness);
                if let Some(value) = forwarded {
                  self.cx.replace_all_uses(inst, value);
                } else {
                  mem_entries.push(inst);
                }
                continue;
              }
              // Only push memory barriers.
              OpData::Call { func, .. } => {
                if pureness[*func] == Pureness::Impure {
                  mem_entries.push(inst);
                  continue;
                }
              }
              _ => {}
            }

            // Canonicalize the instruction
            let canonical_expr: CanonicalExpr = op_data.into();
            // If it's not the target of GVN, we skip it.
            if canonical_expr == CanonicalExpr::None {
              continue;
            }

            if let Some(value) = self.symbols.get(&canonical_expr) {
              // Replace the instruction with the existing value.
              self.cx.replace_all_uses(inst, *value);
            } else {
              // Insert the canonical expression into the symbol table.
              self.symbols.insert(canonical_expr, inst);
            }
          }

          // Push End phase to the stack
          self.stack.push(GVNPhase::End);

          // Update stack and symbol table
          let mut idoms = dom_tree[bb_id.get_bb_id()]
            .iter()
            .map(|&child| Operand::BB(child))
            .collect::<Vec<_>>();
          idoms.sort_by_key(|idom| self.rdfn[idom.get_bb_id()]);
          for idom in idoms.iter().rev() {
            self.enter_scope(*idom);
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
    self.cx.mount(program);
  }

  fn run(&mut self) {
    // Run Call Graph analysis to get the call graph.
    let call_graph = analyze::<CallGraphAnalysis>(self.cx.ir());
    let sccs = analyze::<SCCAnalysis>(&call_graph);
    // Run Pureness analysis to get the pureness information.
    let cx_ptr = &mut self.cx as *mut PassContext<'a>;
    let pureness = analyze::<PurenessAnalysis>((unsafe { &mut *cx_ptr }, &call_graph, &sccs));

    // Run dominance analysis to get the dominator tree.

    // run dominance analysis to get the dominator tree
    for func_id in self.cx.ir().funcs.collect_internal() {
      let func_id = Operand::Func(func_id);
      let func = self.cx.get_func(func_id);
      let (dom_tree, _) = &*self.cx.analyze::<DomAnalysis>(func);

      self.init(func_id);
      let entry = Operand::BB(self.cx.get_func(func_id).cfg.entry.unwrap());
      self.dfs(entry);
      for (rdfn, bb_id) in self.dfs_post_order.iter().rev().enumerate() {
        self.rdfn[bb_id.get_bb_id()] = rdfn;
      }

      // Update stack and symbol table.
      self.enter_scope(entry);
      self.run(dom_tree, &call_graph, &pureness);
    }
  }
}
