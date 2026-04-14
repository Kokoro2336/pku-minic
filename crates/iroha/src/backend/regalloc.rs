//! Register allocation (RegAlloc) via Graph Coloring.
//! Based on Appel and George's paper Iterated Register Coalescing.
//! Reference: https://dl.acm.org/doi/10.1145/229542.229546

use std::ops::{BitAndAssign, BitOr};

use crate::analysis::{LiveAnalysis, LiveOuts};
use yachiyo::analysis::analyze;
use yachiyo::config::{
  CALLEE_SAVED_FREGS, CALLEE_SAVED_XREGS, CALLER_SAVED_FREGS, CALLER_SAVED_XREGS, COLOR_FREGS,
  COLOR_XREGS, RESERVED_REG,
};
use yachiyo::config::{INT_IMM_MAX, INT_IMM_MIN, REGS_NUM};
use yachiyo::ir::back::{
  get_clobbered, BAttr, BBuilder, BFunction, BOp, BOpData, BOperand, BType, BackIR, LOpData,
  MOpData, MemInfo, Reg, Slot, XReg,
};
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::match_some;
use yachiyo::utils::set::{array_set, ArraySet, BitSet};
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

use rustc_hash::FxHashSet;

const RESERVED_REG_BOPRD: BOperand = BOperand::Reg(Reg::X(RESERVED_REG));
const SP_BOPRD: BOperand = BOperand::Reg(Reg::X(XReg::Sp));

#[derive(PartialEq, Eq, Default)]
#[allow(unused)]
enum AllocatorType {
  #[default]
  Int,
  Float,
  Vector, // TODO: For future vectorization extension.
}

enum RemapMode {
  Def(BOperand),
  // (VRegId, index)
  Use(BOperand, usize),
}

#[derive(Default)]
struct Allocator<'a> {
  ir: Option<&'a mut BackIR>,
  builder: BBuilder,

  // Allocator Type
  typ: AllocatorType,

  // ========== Node Structures ==========
  // All of the following structures are indexed by VirtId.

  // Physical registers won't be added to worklists.
  /// Worklist for value that can be simplified.
  simplify_worklist: Worklist<BOperand, BitSet>,
  /// Worklist for value that can be freezed.
  freeze_worklist: Worklist<BOperand, BitSet>,
  /// Worklist for value that needs to be spilled.
  spill_worklist: Worklist<BOperand, BitSet>,

  // Physical registers won't be added to sets.
  /// Nodes Set
  spilled_nodes: BitSet,
  coalesced_nodes: BitSet,
  colored_nodes: BitSet,

  /// Nodes removed from the graph.
  select_stack: Vec<BOperand>,

  // ========== Moves Structures ==========
  // All of the following structures are indexed by InstId.
  /// Move instructions that has been coalesced.
  coalesced_moves: BitSet,
  /// Move instructions that cannot be coalesced. Constrained moves are included in it.
  frozen_moves: BitSet,
  /// Move instructions that is ready to be coalesced.
  worklist_moves: Worklist<BOperand, BitSet>,
  /// Move instructions that is possible to be coalesced.
  active_moves: BitSet,

  // ========== Adjacency Structures ==========
  // All of the following structures are indexed by VirtId, no Physical registers.
  // But physical registers can be in the adj_set and adj_list as neighbors.
  /// Interference edges, (VirtId, VirtId) pairs.
  adj_set: FxHashSet<(BOperand, BOperand)>,
  /// Adjacent Matrix.
  adj_list: Vec<ArraySet<BOperand>>,
  /// Degree of each node.
  degree: Vec<usize>,

  // ========== Coloring Structures ==========
  /// Currently living move instructions associated with each node(not the original moves).
  move_list: Vec<ArraySet<BOperand>>,
  /// Alias of each node.
  alias: Vec<BOperand>,
  /// Color assigned to each node.
  color: Vec<Option<Reg>>,
}

impl Allocator<'_> {
  pub fn new(typ: AllocatorType) -> Self {
    Self {
      ir: None,
      builder: BBuilder::default(),
      typ,
      simplify_worklist: Worklist::new(),
      freeze_worklist: Worklist::new(),
      spill_worklist: Worklist::new(),
      spilled_nodes: BitSet::new(),
      coalesced_nodes: BitSet::new(),
      colored_nodes: BitSet::new(),
      select_stack: Vec::new(),
      coalesced_moves: BitSet::new(),
      frozen_moves: BitSet::new(),
      worklist_moves: Worklist::new(),
      active_moves: BitSet::new(),
      adj_set: FxHashSet::default(),
      adj_list: Vec::new(),
      degree: Vec::new(),
      move_list: Vec::new(),
      alias: Vec::new(),
      color: Vec::new(),
    }
  }

  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.builder.set_current_func(func_id);
  }

  fn reset(&mut self) {
    let func_id = self.builder.current_function.unwrap();

    // Clear the nodes worklist.
    self.simplify_worklist.clear();
    self.freeze_worklist.clear();
    self.spill_worklist.clear();

    // Clear the nodes set.
    self.spilled_nodes.clear();
    self.coalesced_nodes.clear();
    self.colored_nodes.clear();

    // Clear the select stack.
    self.select_stack.clear();

    // Clear the moves set.
    self.coalesced_moves.clear();
    self.frozen_moves.clear();
    self.worklist_moves.clear();
    self.active_moves.clear();

    // Clear the adjacency set.
    self.adj_set.clear();

    self.adj_list.clear();
    self.degree.clear();
    //Resize
    self
      .adj_list
      .resize(self.get_func(func_id).vregs.len(), ArraySet::new());
    self.degree.resize(self.get_func(func_id).vregs.len(), 0);

    // Clear the move list.
    self.move_list.clear();
    self.alias.clear();
    self.color.clear();
    // Resize
    let vregs_len = self.get_func(func_id).vregs.len();
    self.move_list.resize(vregs_len, ArraySet::new());
    self.alias.resize(vregs_len, BOperand::Undef);
    self.color.resize(vregs_len, None);
  }

  // ========= Helper Functions ==========

  #[inline(always)]
  fn is_target(&self, vreg_id: BOperand) -> bool {
    let func_id = self.builder.current_function.unwrap();
    match_some! {
        target: vreg_id,
        enu: BOperand,
        minor_arms: {
            BOperand::Reg(Reg::Virt(_)) => {
                let vreg = &self.get_func(func_id).vregs[vreg_id];
                match &vreg.typ {
                  BType::I32 | BType::U64 | BType::Array { .. } => self.typ == AllocatorType::Int,
                    BType::F32 => self.typ == AllocatorType::Float,
                    BType::Void => false,
                }
            }
            BOperand::Reg(Reg::F(_)) => self.typ == AllocatorType::Float,
            BOperand::Reg(Reg::X(_)) => self.typ == AllocatorType::Int,
        },
        uni_ops: [IntImm, FloatImm, BB, Inst, Func, Data, RoData, Bss, Slot, Undef],
        uni_arm: {
            false
        }
    }
  }

  #[inline(always)]
  fn get_src(&self, op_id: BOperand) -> Vec<&BOperand> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_src(func_id, op_id)
  }

  #[inline(always)]
  fn get_src_tuple(&self, op_id: BOperand) -> Vec<(&BOperand, usize)> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_src_tuple(func_id, op_id)
  }

  #[inline(always)]
  fn get_rd(&self, op_id: BOperand) -> Option<&BOperand> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_rd(func_id, op_id)
  }

  #[inline(always)]
  fn get_func(&self, func_id: BOperand) -> &BFunction {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_func_mut(&mut self, func_id: BOperand) -> &mut BFunction {
    &mut self.ir.as_mut().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_degree(&self, reg: BOperand) -> usize {
    if reg.is_phys() {
      // Physical registers' degree is considered infinite, since they can't be spilled.
      usize::MAX
    } else {
      self.degree[reg.get_virt_id()]
    }
  }

  #[inline(always)]
  fn alloc_slot(&mut self, slot: Slot) -> BOperand {
    let func_id = self.builder.current_function.unwrap();
    let func = self.get_func_mut(func_id);
    let slot_id = func.frame_info.alloc(slot);
    BOperand::Slot(slot_id)
  }

  #[inline(always)]
  fn create(&mut self, op: BOp) -> BOperand {
    let func_id = self.builder.current_function;
    let ir = self.ir.as_mut().unwrap();
    ir.create(&self.builder, func_id, op)
  }

  #[inline(always)]
  fn get_colors<T: FromIterator<Reg>>(&self) -> T {
    match self.typ {
      // Callee-saved registers are preferred.
      AllocatorType::Int => CALLEE_SAVED_XREGS
        .to_vec()
        .into_iter()
        .map(Reg::X)
        .chain(CALLER_SAVED_XREGS.to_vec().into_iter().map(Reg::X))
        .collect::<T>(),
      AllocatorType::Float => CALLEE_SAVED_FREGS
        .to_vec()
        .into_iter()
        .map(Reg::F)
        .chain(CALLER_SAVED_FREGS.to_vec().into_iter().map(Reg::F))
        .collect::<T>(),
      AllocatorType::Vector => unimplemented!(),
    }
  }

  #[inline(always)]
  fn get_colors_num(&self) -> usize {
    match self.typ {
      AllocatorType::Int => COLOR_XREGS,
      AllocatorType::Float => COLOR_FREGS,
      AllocatorType::Vector => unimplemented!(),
    }
  }

  /// Add an undirected edge between u and v in the interference graph.
  fn add_edge(&mut self, u: BOperand, v: BOperand) {
    if u == v
            || self.adj_set.contains(&(u, v))
            || self.adj_set.contains(&(v, u))
            // To avoid interference between non-target nodes, we only add edges between target nodes.
            || !self.is_target(u)
            || !self.is_target(v)
    {
      return;
    }

    // Insert the edges
    self.adj_set.insert((u, v));
    self.adj_set.insert((v, u));

    if matches!(u, BOperand::Reg(Reg::Virt(_))) {
      self.adj_list[u.get_virt_id()].insert(v);
      self.degree[u.get_virt_id()] += 1;
    }
    if matches!(v, BOperand::Reg(Reg::Virt(_))) {
      self.adj_list[v.get_virt_id()].insert(u);
      self.degree[v.get_virt_id()] += 1;
    }
  }

  fn build(&mut self, live_outs: &LiveOuts) {
    let func_id = self.builder.current_function.unwrap();
    let cfg_ids = self.get_func(func_id).cfg.ids();

    for bb_id in cfg_ids {
      let cur = self.get_func(func_id).cfg[bb_id].cur.clone();
      let mut live = live_outs[bb_id].clone();

      for inst_id in cur.iter().rev() {
        let op = &self.get_func(func_id).dfg[*inst_id];
        let rd = self.get_rd(*inst_id).cloned();

        // For move instructions, we need to handle them specially.
        let mut src = self
          .get_src(*inst_id)
          .into_iter()
          .cloned()
          .collect::<Vec<_>>();
        // If the instruction has implicit use operand, we also add the implicit use operand to src.
        op.attrs
          .iter()
          .find(|attr| matches!(attr, BAttr::ImplicitUse(_)))
          .and_then(|implicit_use| {
            if let BAttr::ImplicitUse(implicit_src) = implicit_use {
              src.extend(implicit_src);
              Some(())
            } else {
              None
            }
          });

        if op.data.is_move() {
          let rd = rd.expect("Move instruction should have rd");
          // Ignore move that is irrelevant to current allocator.
          if self.is_target(rd) {
            // Add the move instruction to src & rd's moveList.
            for s in src.iter() {
              if let BOperand::Reg(_) = s {
                // To avoid interference between src and rd, we substract src from live set temporarily.
                live = live.difference(&array_set![*s]);
                // When s is a virtual register, we should alse add the move instruction to s's moveList.
                if let BOperand::Reg(Reg::Virt(id)) = s {
                  self.move_list[*id].insert(*inst_id);
                }
              }
            }
            if let BOperand::Reg(Reg::Virt(id)) = rd {
              self.move_list[id].insert(*inst_id);
            }
            // Add the move instruction to worklistMoves.
            self.worklist_moves.push_back(*inst_id);
          }
        }

        let op = &self.get_func(func_id).dfg[*inst_id];
        if let Some(rd) = rd {
          let mut rds = array_set![rd];
          // Special handling for call instruction: add all clobbered registers to rd, since they are all defined by the call instruction.
          if op.attrs.contains(&BAttr::Clobber) {
            rds = rds.union(
              &get_clobbered::<ArraySet<Reg>>()
                .into_iter()
                .map(BOperand::Reg)
                // In allocator, we only care about target nodes, so we filter out non-target nodes here.
                .filter(|reg| self.is_target(*reg))
                .collect::<ArraySet<BOperand>>(),
            );
          }
          op.attrs
            .iter()
            .find(|attr| matches!(attr, BAttr::ImplicitDef(_)))
            .and_then(|attr| {
              if let BAttr::ImplicitDef(implicit_rd) = attr {
                rds.insert(*implicit_rd);
                Some(())
              } else {
                None
              }
            });

          for rd in rds.iter() {
            // Add interference edges between rd and all live-out nodes.
            // All of the current live nodes are included, but we'll filter out non-target nodes in add_edge function.
            for live_var in live.iter() {
              self.add_edge(*rd, *live_var);
            }
          }
          // Remove def from live set
          live = live.difference(&rds);
        }

        // Retrieve src
        for s in src {
          if let BOperand::Reg(_) = s {
            live.insert(s);
          }
        }
      }
    }
  }

  #[inline(always)]
  fn adjacent(&self, n: BOperand) -> ArraySet<BOperand> {
    let mut select_stack = ArraySet::new();
    for s in self.select_stack.iter() {
      select_stack.insert(*s);
    }
    let mut coalesced_nodes = ArraySet::new();
    for n in self.coalesced_nodes.iter() {
      coalesced_nodes.insert(BOperand::Reg(Reg::Virt(n)));
    }
    if let BOperand::Reg(Reg::Virt(id)) = n {
      self.adj_list[id]
        .difference(&select_stack)
        .difference(&coalesced_nodes)
    } else {
      ArraySet::new()
    }
  }

  #[inline(always)]
  fn node_moves(&self, n: BOperand) -> ArraySet<BOperand> {
    if let BOperand::Reg(Reg::Virt(id)) = n {
      let mut included_moves = ArraySet::new();
      for m in self
        .active_moves
        .bitor(self.worklist_moves.get_in_list())
        .iter()
      {
        included_moves.insert(BOperand::Inst(m));
      }
      self.move_list[id].intersection(&included_moves)
    } else {
      ArraySet::new()
    }
  }

  #[inline(always)]
  fn move_related(&self, n: BOperand) -> bool {
    !self.node_moves(n).is_empty()
  }

  fn make_worklist(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let vregs_ids = self.get_func(func_id).vregs.ids();

    for vreg_id in vregs_ids {
      let vreg_id = BOperand::Reg(Reg::Virt(vreg_id));
      // Nodes that are not target nodes should not be added to worklists.
      if !self.is_target(vreg_id) {
        continue;
      }

      if self.get_degree(vreg_id) >= self.get_colors_num() {
        self.spill_worklist.push_back(vreg_id);
      } else if self.move_related(vreg_id) {
        self.freeze_worklist.push_back(vreg_id);
      } else {
        self.simplify_worklist.push_back(vreg_id);
      }
    }
  }

  fn simplify(&mut self) {
    let n = self.simplify_worklist.pop_front().unwrap();
    if !self.is_target(n) {
      return;
    }
    self.select_stack.push(n);
    for m in self.adjacent(n) {
      self.decrement_degree(m);
    }
  }

  fn decrement_degree(&mut self, n: BOperand) {
    if !n.is_virt() {
      return;
    }
    let d = self.degree[n.get_virt_id()];
    self.degree[n.get_virt_id()] = d - 1;
    // If the degree of n drops below the number of colors, we can enable n and its adjacent nodes m.
    if d == self.get_colors_num() {
      // Enable n and its adjacent nodes m.
      let nodes = array_set![n];
      nodes.union(&self.adjacent(n));
      for m in nodes {
        self.enable_moves(m);
      }
      // Move n from spillWorklist to freezeWorklist or simplifyWorklist.
      self.spill_worklist.remove(&n);
      if self.move_related(n) {
        self.freeze_worklist.push_back(n);
      } else {
        self.simplify_worklist.push_back(n);
      }
    }
  }

  /// Enable n's related moves which are in active_moves.
  fn enable_moves(&mut self, n: BOperand) {
    for m in self.node_moves(n) {
      if self.active_moves.contains(m.get_inst_id()) {
        self.active_moves.remove(m.get_inst_id());
        self.worklist_moves.push_back(m);
      }
    }
  }

  fn coalesce(&mut self) {
    let m = self.worklist_moves.pop_front().unwrap();
    let (x, y) = {
      let rd = self.get_rd(m).unwrap();
      let src = self.get_src(m);
      assert!(src.len() == 1);
      (rd, src[0])
    };
    // Get alias of x and y.
    let x = self.get_alias(*x);
    let y = self.get_alias(*y);
    // if y is precolored, swap x and y.
    let (u, v) = if y.is_phys() { (y, x) } else { (x, y) };

    // Remove the move from worklist_moves.
    self.worklist_moves.remove(&m);
    if u == v {
      self.coalesced_moves.insert(m.into());
      self.add_worklist(u);
    } else if v.is_phys() || self.adj_set.contains(&(u, v)) {
      // In ideal case, u can't interfere with v, since v was removed during the interference graph building of u.
      // If this edge is found, then v must interfere with u in some other place, so we can't coalesce them.
      self.frozen_moves.insert(m.into());
      self.add_worklist(u);
      self.add_worklist(v);
    } else if (u.is_phys() && self.adjacent(u).iter().all(|t| self.ok(*t, u)))
      || (u.is_virt() && self.conservative(self.adjacent(u).union(&self.adjacent(v))))
    {
      self.coalesced_moves.insert(m.into());
      self.combine(u, v);
      // Since v is combined, we just need to add u to worklist.
      self.add_worklist(u);
    } else {
      // If none of the above conditions hold, we can't coalesce m now. We put it back to active_moves and try it later.
      self.active_moves.insert(m.into());
    }
  }

  /// v combined into u
  fn combine(&mut self, u: BOperand, v: BOperand) {
    if let BOperand::Reg(Reg::Virt(v_id)) = v {
      if self.freeze_worklist.contains(&v) {
        self.freeze_worklist.remove(&v);
      } else {
        self.spill_worklist.remove(&v);
      }
      self.coalesced_nodes.insert(v_id);
      // Set alias
      self.alias[v_id] = u;

      if let BOperand::Reg(Reg::Virt(u_id)) = u {
        // Combine the nodes' node_moves(NOT original move_list).
        self.move_list[u_id] = self.move_list[u_id].union(&self.move_list[v_id]);
      }

      // Update interference graph of u.
      for t in self.adjacent(v) {
        self.add_edge(t, u);
        // Decrease degree of t since add_edge increase the degree of t.
        self.decrement_degree(t);
      }
    }
    if let BOperand::Reg(Reg::Virt(u_id)) = u {
      // u can't be in simplify_worklist now.
      if self.degree[u_id] >= self.get_colors_num() && self.freeze_worklist.contains(&u) {
        self.freeze_worklist.remove(&u);
        self.spill_worklist.push_back(u);
      }
    }
  }

  /// TODO: Briggs' conservative coalescing test.
  fn conservative(&self, adjacent_nodes: ArraySet<BOperand>) -> bool {
    let k = adjacent_nodes
      .iter()
      .filter(|n| self.get_degree(**n) >= self.get_colors_num())
      .count();
    k < self.get_colors_num()
  }

  /// TODO: George test.
  fn ok(&self, t: BOperand, r: BOperand) -> bool {
    self.get_degree(t) < self.get_colors_num() || t.is_phys() || self.adj_set.contains(&(t, r))
  }

  /// Add n to simplify_worklist.
  fn add_worklist(&mut self, n: BOperand) {
    if let BOperand::Reg(Reg::Virt(id)) = n {
      if self.get_degree(n) >= self.get_colors_num() || self.colored_nodes.contains(id) {
        return;
      }
    }
    // n might still lie in freeze_worklist but no longer be move-related after coalescing.
    if self.move_related(n) {
      return;
    }
    if let BOperand::Reg(Reg::Virt(_)) = n {
      self.freeze_worklist.remove(&n);
      self.simplify_worklist.push_back(n);
    }
  }

  fn get_alias(&self, n: BOperand) -> BOperand {
    if let BOperand::Reg(Reg::Virt(id)) = n {
      if self.coalesced_nodes.contains(id) {
        self.get_alias(self.alias[id])
      } else {
        n
      }
    } else {
      // Return physical register directly
      n
    }
  }

  /// Free the node from freeze_worklist and freeze all of its moves.
  /// It means that all of its related move is given up to coalesce.
  fn freeze(&mut self) {
    let u = self.freeze_worklist.pop_front().unwrap();
    self.simplify_worklist.push_back(u);
    self.freeze_moves(u);
  }

  fn freeze_moves(&mut self, n: BOperand) {
    for m in self.node_moves(n) {
      let v = {
        let rd = *self.get_rd(m).unwrap();
        let src = self.get_src(m);
        assert!(src.len() == 1);
        if rd == n {
          *src[0]
        } else {
          rd
        }
      };
      // Remove from active_moves/worklist_moves and add to frozen_moves.
      self.active_moves.remove(m.get_inst_id());
      self.worklist_moves.remove(&m);
      self.frozen_moves.insert(m.into());

      // If the other node can be moved to simplify_worklist due to the freezing, we add it to simplify_worklist.
      if v == n {
        continue;
      }
      if self.get_degree(v) < self.get_colors_num() && self.node_moves(v).is_empty() {
        self.freeze_worklist.remove(&v);
        self.simplify_worklist.push_back(v);
      }
    }
  }

  /// Select a node for spilling and add it to simplify_worklist.
  fn select_spill(&mut self) {
    let n = self.spill_worklist.pop_front().unwrap();
    self.simplify_worklist.push_back(n);
    self.freeze_moves(n);
  }

  fn assign_colors(&mut self) {
    while let Some(n) = self.select_stack.pop() {
      let mut ok_colors = self.get_colors::<Vec<Reg>>();
      // Use physical adjacent list rather than adjacent().
      for w in self.adj_list[n.get_virt_id()].iter() {
        if w.is_phys() {
          ok_colors.retain(|&color| {
            color
              != match w {
                BOperand::Reg(Reg::F(r)) => Reg::F(*r),
                BOperand::Reg(Reg::X(r)) => Reg::X(*r),
                _ => unreachable!("Neighbor can't be non-reg"),
              }
          });
        } else if w.is_virt() {
          let get_color = |reg: BOperand| -> Option<Reg> {
            match reg {
              BOperand::Reg(phys @ (Reg::F(_) | Reg::X(_))) => Some(phys),
              BOperand::Reg(Reg::Virt(_)) => {
                let alias = self.get_alias(reg);
                match alias {
                  BOperand::Reg(phys @ (Reg::F(_) | Reg::X(_))) => Some(phys),
                  BOperand::Reg(Reg::Virt(id)) => self.color[id],
                  _ => unreachable!("Unexpected alias: {:?}", alias),
                }
              }
              _ => unreachable!("Unexpected reg: {:?}", reg),
            }
          };

          if let Some(c) = get_color(*w) {
            ok_colors.retain(|&color| color != c);
          }
        } else {
          unreachable!("Node in adjacent list can't be non-reg: {:?}", w);
        }
      }
      // If no color is available, we have to spill the node.
      if ok_colors.is_empty() {
        self.spilled_nodes.insert(n.get_virt_id());
      } else {
        self.colored_nodes.insert(n.get_virt_id());
        self.color[n.get_virt_id()] = Some(ok_colors[0]);
      }
    }
    // For coalesced nodes, we just assign them the color of their alias.
    for n in self.coalesced_nodes.iter() {
      let alias = self.get_alias(BOperand::Reg(Reg::Virt(n)));
      if alias.is_virt() {
        let alias_color = self.color[self.get_alias(BOperand::Reg(Reg::Virt(n))).get_virt_id()];
        self.color[n] = alias_color;
      } else if alias.is_phys() {
        self.color[n] = match alias {
          BOperand::Reg(Reg::F(r)) => Some(Reg::F(r)),
          BOperand::Reg(Reg::X(r)) => Some(Reg::X(r)),
          _ => None,
        };
      } else {
        // This should never happen since physical registers can't be coalesced.
        unreachable!("Alias can't be non-reg");
      }
    }
  }

  fn insert_spills(&mut self) -> ArraySet<BOperand> {
    // Build a map op -> bb
    let op_to_bb = {
      let func_id = self.builder.current_function.unwrap();
      let mut map = vec![BOperand::Undef; self.get_func(func_id).dfg.len()];

      for bb_id in self.get_func(func_id).cfg.ids() {
        let cur = &self.get_func(func_id).cfg[bb_id].cur;
        for op_id in cur.iter() {
          map[op_id.get_inst_id()] = BOperand::BB(bb_id);
        }
      }
      map
    };
    let mut new_temps = array_set![];

    for spilled in std::mem::take(&mut self.spilled_nodes).iter() {
      let vreg_id = BOperand::Reg(Reg::Virt(spilled));
      let (typ, defs, uses) = {
        let func_id = self.builder.current_function.unwrap();
        let vreg = &self.get_func(func_id).vregs[vreg_id];
        (vreg.typ.clone(), vreg.defs.clone(), vreg.uses.clone())
      };
      // Allocate new slot
      let slot_id = self.alloc_slot(Slot::Local {
        typ: typ.clone(),
        offset: 0,
      });

      // Insert store after each definition of the spilled node.
      for def in defs {
        let bb_id = op_to_bb[def.get_inst_id()];
        self.builder.set_current_block(bb_id);
        self.builder.set_after_inst(
          self.ir.as_mut().unwrap(),
          self.builder.current_function,
          Some(def),
        );

        let store_op = BOp::new(
          typ.clone(),
          vec![],
          LOpData::Store {
            addr: slot_id,
            value: *self.get_rd(def).unwrap(),
          }
          .into(),
        );

        self.create(store_op);
      }

      for (r#use, idx) in uses {
        let bb_id = op_to_bb[r#use.get_inst_id()];
        self.builder.set_current_block(bb_id);
        self.builder.set_before_inst(
          self.ir.as_mut().unwrap(),
          self.builder.current_function,
          Some(r#use),
        );

        let load_op = BOp::new(
          typ.clone(),
          vec![],
          LOpData::Load {
            rd: BOperand::Undef,
            addr: slot_id,
          }
          .into(),
        );

        let load_id = self.create(load_op);
        let load_vreg_id = self.get_rd(load_id).cloned().unwrap();
        new_temps.insert(load_vreg_id);

        let src = self.get_src(r#use).into_iter().cloned().collect::<Vec<_>>();
        for operand in src {
          // Replace the following use
          let mut remap_use = |remap_mode: RemapMode| match remap_mode {
            RemapMode::Def(_) => unreachable!(),
            RemapMode::Use(old_operand, idx) => {
              self.replace_src((r#use, idx), old_operand, load_vreg_id);
            }
          };
          remap_use(RemapMode::Use(operand, idx));
        }
      }
    }
    new_temps
  }

  #[inline(always)]
  fn replace_rd(&mut self, inst_id: BOperand, new_operand: BOperand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_rd(func_id, inst_id, new_operand);
  }

  #[inline(always)]
  fn replace_src(
    &mut self,
    use_tuple: (BOperand, usize),
    old_operand: BOperand,
    new_operand: BOperand,
  ) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .replace_src(func_id, use_tuple, old_operand, new_operand);
  }

  fn rewrite(&mut self) {
    let func_id = self.builder.current_function.unwrap();

    for bb_id in self.get_func(func_id).cfg.collect() {
      let bb_id = BOperand::BB(bb_id);
      for inst_id in self.get_func(func_id).cfg[bb_id].cur.clone() {
        let rd = self.get_rd(inst_id).cloned();
        let src_tuples = self
          .get_src_tuple(inst_id)
          .into_iter()
          .map(|(s, idx)| (*s, idx))
          .collect::<Vec<_>>();

        let mut remap_operand = |remap_mode: RemapMode| {
          let (operand, idx) = match remap_mode {
            RemapMode::Def(operand) => (operand, None),
            RemapMode::Use(operand, idx) => (operand, Some(idx)),
          };
          if !operand.is_virt() || !self.is_target(operand) {
            return;
          }
          let alias = self.get_alias(operand);
          if let BOperand::Reg(Reg::Virt(id)) = alias {
            if !self.colored_nodes.contains(id) {
              panic!("rewrite: virtual register v{} is not in colored_nodes", id);
            }
            let color = self.color[id]
              .unwrap_or_else(|| panic!("rewrite: virtual register v{} has no assigned color", id));
            // Remove use first
            if let Some(idx) = idx {
              // use tuple is (InstId, idx)
              self.replace_src((inst_id, idx), operand, BOperand::Reg(color));
            } else {
              self.replace_rd(inst_id, BOperand::Reg(color));
            }
          } else if alias.is_phys() {
            // Remove use first
            if let Some(idx) = idx {
              // use tuple is (InstId, idx)
              self.replace_src((inst_id, idx), operand, alias);
            } else {
              self.replace_rd(inst_id, alias);
            }
          } else {
            unreachable!("Alias can't be non-reg");
          }
        };

        if let Some(rd) = rd {
          remap_operand(RemapMode::Def(rd));
        }
        for (src, idx) in src_tuples {
          remap_operand(RemapMode::Use(src, idx));
        }
      }
    }
  }

  /// Main function of register allocation.
  /// LiveOuts is the result of current function's liveness analysis, which is used for building the interference graph.
  fn run(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    loop {
      // Reset the worklist.
      self.reset();
      // Run live analysis.
      let (_, live_outs) = analyze::<LiveAnalysis>(self.get_func(func_id));
      #[cfg(feature = "debug")]
      yachiyo::debug::info!(
        "Finished liveness analysis for register allocation. funcs_live_outs: {:?}",
        live_outs
      );
      // Build the interference graph.
      self.build(&live_outs);
      #[cfg(feature = "debug")]
      yachiyo::debug::info!(
        "Finished building interference graph for register allocation. adj_set: {:?}, degree: {:?}",
        self.adj_set,
        self.degree
      );
      // Make the initial worklist.
      self.make_worklist();
      #[cfg(feature = "debug")]
      yachiyo::debug::info!(
                "Finished making initial worklist for register allocation. simplify_worklist: {:?}, worklist_moves: {:?}, freeze_worklist: {:?}, spill_worklist: {:?}",
                self.simplify_worklist,
                self.worklist_moves,
                self.freeze_worklist,
                self.spill_worklist,
            );

      // Main loop(The state machine).
      loop {
        if !self.simplify_worklist.is_empty() {
          self.simplify();
        } else if !self.worklist_moves.is_empty() {
          self.coalesce();
        } else if !self.freeze_worklist.is_empty() {
          self.freeze();
        } else if !self.spill_worklist.is_empty() {
          self.select_spill();
        } else {
          break;
        }
      }
      #[cfg(feature = "debug")]
      yachiyo::debug::info!(
                "Finished main loop of register allocation. simplify_worklist: {:?}, worklist_moves: {:?}, freeze_worklist: {:?}, spill_worklist: {:?}, \nselect_stack: {:?}, \ncoalesced_nodes: {:?}",
                self.simplify_worklist,
                self.worklist_moves,
                self.freeze_worklist,
                self.spill_worklist,
                self.select_stack,
                self.coalesced_nodes
            );
      // Assign colors to the nodes.
      self.assign_colors();
      #[cfg(feature = "debug")]
      yachiyo::debug::info!(
                "Finished assigning colors for register allocation. colored_nodes: {:?}, \nspilled_nodes: {:?}",
                self.colored_nodes,
                self.spilled_nodes
            );
      // If there is no spill, we are done.
      if self.spilled_nodes.is_empty() {
        break;
      }
      self.insert_spills();
    }
    #[cfg(feature = "debug")]
    yachiyo::debug::info!(
      "Finished register allocation for function v{}. \ncolored_nodes: {:?}, \nspilled_nodes: {:?}",
      func_id,
      self.colored_nodes,
      self.spilled_nodes
    );
    // Rewrite the program to replace the virtual registers.
    self.rewrite();
  }
}

pub struct RegAlloc<'a> {
  ir: Option<&'a mut BackIR>,
  builder: BBuilder,
  allocators: Vec<Allocator<'a>>,

  // ========= Frame Lowering Structures ==========
  /// ra & Saved Registers -> Slot
  slot_map: Vec<BOperand>,
  /// Used Registers
  used_phys: BitSet,
}

impl RegAlloc<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.builder.set_current_func(func_id);
  }

  fn reset(&mut self) {
    self.slot_map.clear();
    self.used_phys.clear();
    self.slot_map.resize(REGS_NUM, BOperand::Undef);
  }

  #[inline(always)]
  fn create(&mut self, op: BOp) -> BOperand {
    let func_id = self.builder.current_function;
    let ir = self.ir.as_mut().unwrap();
    ir.create(&self.builder, func_id, op)
  }

  #[inline(always)]
  fn get_src(&self, op_id: BOperand) -> Vec<&BOperand> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_src(func_id, op_id)
  }

  #[inline(always)]
  fn get_rd(&self, op_id: BOperand) -> Option<&BOperand> {
    let func_id = self.builder.current_function;
    self.ir.as_ref().unwrap().get_rd(func_id, op_id)
  }

  #[inline(always)]
  fn get_operand_type(&self, operand: BOperand) -> BType {
    let func_id = self.builder.current_function.unwrap();

    match operand {
      BOperand::Inst(id) => {
        let op = &self.get_func(func_id).dfg[id];
        op.typ.clone()
      }
      BOperand::Reg(reg) => match reg {
        Reg::X(_) => BType::I32,
        Reg::F(_) => BType::F32,
        Reg::Virt(_) => self.get_func(func_id).vregs[operand].typ.clone(),
      },
      BOperand::IntImm(_) => BType::I32,
      BOperand::FloatImm(_) => BType::F32,
      BOperand::Undef => BType::Void,

      BOperand::Slot(_) => match &self.get_func(func_id).frame_info[operand] {
        Slot::CalleeSaved { typ, .. }
        | Slot::Local { typ, .. }
        | Slot::Param { typ, .. }
        | Slot::Arg { typ, .. } => typ.clone(),
      },
      BOperand::Data(_) => self.ir.as_ref().unwrap().data_info[operand].typ.clone(),
      BOperand::RoData(_) => self.ir.as_ref().unwrap().rodata_info[operand].typ.clone(),
      BOperand::Bss(_) => self.ir.as_ref().unwrap().bss_info[operand].typ.clone(),

      BOperand::Func(_) | BOperand::BB(_) => unreachable!(),
    }
  }

  #[inline(always)]
  fn get_func(&self, func_id: BOperand) -> &BFunction {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  fn get_func_mut(&mut self, func_id: BOperand) -> &mut BFunction {
    &mut self.ir.as_mut().unwrap().funcs[func_id]
  }

  /// typ: The type of rs.
  fn select_store(&mut self, typ: BType, attrs: Vec<BAttr>, rs: BOperand, base: BOperand, offset: Option<BOperand>) -> BOp {
    if let Some(offset) = offset {
      // if offset is a reg, create individual add instruction.
      if !offset.is_literal() {
        self.create(BOp::new(
            BType::U64,
            vec![],
            MOpData::Add { rd: RESERVED_REG_BOPRD, rs1: base, rs2: offset }.into()
        ));
      }
    };
    match typ {
      BType::I32 | BType::U64 | BType::Array { .. } => if let Some(offset) = offset {
        if offset.is_literal() {
          BOp::new(BType::Void, attrs, MOpData::Sw { rs, base, offset }.into())
        } else {
          BOp::new(BType::Void, attrs, MOpData::Sw { rs, base: RESERVED_REG_BOPRD, offset: BOperand::IntImm(0) }.into())
        }
      } else {
        BOp::new(BType::Void, attrs, MOpData::Sw { rs, base, offset: BOperand::IntImm(0) }.into())
      },
      BType::F32 => if let Some(offset) = offset {
        if offset.is_literal() {
          BOp::new(BType::Void, attrs, MOpData::Fsw { rs, base, offset }.into())
        } else {
          BOp::new(BType::Void, attrs, MOpData::Fsw { rs, base: RESERVED_REG_BOPRD, offset: BOperand::IntImm(0) }.into())
        }
      } else {
        BOp::new(BType::Void, attrs, MOpData::Fsw { rs, base, offset: BOperand::IntImm(0) }.into())
      },
      BType::Void => unreachable!(),
    }
  }
  /// typ: The type of rd.
  fn select_load(&mut self, typ: BType, attrs: Vec<BAttr>, rd: BOperand, base: BOperand, offset: Option<BOperand>) -> BOp {
    if let Some(offset) = offset {
      // if offset is a reg, create individual add instruction.
      if !offset.is_literal() {
        self.create(BOp::new(
            BType::U64,
            vec![],
            MOpData::Add { rd: RESERVED_REG_BOPRD, rs1: base, rs2: offset }.into()
        ));
      }
    };
    match typ {
      BType::I32 | BType::U64 | BType::Array { .. } => if let Some(offset) = offset {
        if offset.is_literal() {
          BOp::new(BType::Void, attrs, MOpData::Lw { rd, base, offset }.into())
        } else {
          // Don't use x0 here, which could introduce extra instruction.
          BOp::new(BType::Void, attrs, MOpData::Lw { rd, base: RESERVED_REG_BOPRD, offset: BOperand::IntImm(0) }.into())
        }
      } else {
        BOp::new(BType::Void, attrs, MOpData::Lw { rd, base, offset: BOperand::IntImm(0) }.into())
      },
      BType::F32 => if let Some(offset) = offset {
        if offset.is_literal() {
          BOp::new(BType::Void, attrs, MOpData::Flw { rd, base, offset }.into())
        } else {
          BOp::new(BType::Void, attrs, MOpData::Flw { rd, base: RESERVED_REG_BOPRD, offset: BOperand::IntImm(0) }.into())
        }
      } else {
        BOp::new(BType::Void, attrs, MOpData::Flw { rd, base, offset: BOperand::IntImm(0) }.into())
      },
      BType::Void => unreachable!(),
    }
  }
  fn select_ptr_add(&mut self, typ: BType, attrs: Vec<BAttr>, rd: BOperand, rs1: BOperand, base: BOperand, offset: Option<BOperand>) -> BOp {
    let final_offset = if let Some(offset) = offset {
      self.create(BOp::new(
        typ.clone(),
        vec![],
        if offset.is_literal() {
          MOpData::Addi { rd: RESERVED_REG_BOPRD, rs1: base, imm: offset }.into()
        } else {
          MOpData::Add { rd: RESERVED_REG_BOPRD, rs1: base, rs2: offset }.into()
        }
      ));
      RESERVED_REG_BOPRD
    } else {
      base
    };
    BOp::new(typ, attrs, MOpData::Add { rd, rs1, rs2: final_offset }.into())
  }
  fn select_ptr_sub(&mut self, typ: BType, attrs: Vec<BAttr>, rd: BOperand, rs1: BOperand, base: BOperand, offset: Option<BOperand>) -> BOp {
    let final_offset = if let Some(offset) = offset {
      self.create(BOp::new(
        typ.clone(),
        vec![],
        // TODO: really Add here?
        if offset.is_literal() {
          MOpData::Addi { rd: RESERVED_REG_BOPRD, rs1: base, imm: offset }.into()
        } else {
          MOpData::Add { rd: RESERVED_REG_BOPRD, rs1: base, rs2: offset }.into()
        }
      ));
      RESERVED_REG_BOPRD
    } else {
      base
    };
    BOp::new(typ, attrs, MOpData::Sub { rd, rs1, rs2: final_offset }.into())
  }

  #[inline(always)]
  fn alloc_and_map_slot(&mut self, reg: Reg, slot: Slot) -> BOperand {
    let func_id = self.builder.current_function.unwrap();
    let func = self.get_func_mut(func_id);
    let slot_id = func.frame_info.alloc(slot);
    self.slot_map[u8::from(reg) as usize] = BOperand::Slot(slot_id);

    #[cfg(feature = "debug")]

    yachiyo::debug::info!(
      "Allocated slot for register {:?} in function v{}. slot_id: {:?}, slot_info: {:?}",
      reg,
      func_id,
      slot_id,
      {
        let func = self.get_func_mut(func_id);
        &func.frame_info[slot_id]
      }
    );

    BOperand::Slot(slot_id)
  }

  #[inline(always)]
  fn get_offset(&self, slot_id: BOperand) -> BOperand {
    let func_id = self.builder.current_function.unwrap();
    BOperand::IntImm(match &self.get_func(func_id).frame_info[slot_id] {
      Slot::Local { offset, .. } => *offset,
      Slot::Param { offset, .. } => *offset,
      Slot::Arg { offset, .. } => *offset,
      Slot::CalleeSaved { offset, .. } => *offset,
    })
  }

  #[inline(always)]
  fn get_callee_saved_bitset() -> BitSet {
    let mut bitset = BitSet::new();
    for reg in CALLEE_SAVED_XREGS.iter() {
      bitset.insert(u8::from(Reg::X(*reg)) as usize);
    }
    for reg in CALLEE_SAVED_FREGS.iter() {
      bitset.insert(u8::from(Reg::F(*reg)) as usize);
    }
    bitset
  }

  #[inline(always)]
  fn replace_op(&mut self, inst_id: BOperand, bb_id: BOperand, new_op: BOp) {
    let func_id = self.builder.current_function;
    self.ir.as_mut().unwrap().replace_op_no_rauw(
      &mut self.builder,
      func_id,
      inst_id,
      bb_id,
      new_op,
    );
  }

  #[inline(always)]
  fn legalize_offset(&mut self, imm: BOperand) -> BOperand {
    match_some! {
        target: imm,
        enu: BOperand,
        minor_arms: {
            BOperand::IntImm(i) => {
                if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&i) {
                    self.create(BOp::new(
                        BType::I32,
                        vec![],
                        MOpData::Li { rd: RESERVED_REG_BOPRD, imm: i }.into(),
                    ));
                    RESERVED_REG_BOPRD
                } else {
                    imm
                }
            }
        },
        uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
        uni_arm: {
            unreachable!("Expected integer immediate, found {:?}", imm);
        }
    }
  }

  /// 1. Move sp
  /// 2. Save callee-saved registers
  /// 3. Save ra if the function is not a leaf.
  fn prologue(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let entry = self.get_func(func_id).cfg.entry;
    if entry.is_none() {
      return;
    }
    let entry = BOperand::BB(entry.unwrap());
    self.builder.set_current_block(entry);
    self
      .builder
      .set_at_head(self.ir.as_mut().unwrap(), self.builder.current_function);

    let sp_offset = -(self.get_func(func_id).frame_info.size() as i32);

    if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&sp_offset) {
      self.create(BOp::new(
        BType::I32,
        vec![],
        MOpData::Li {
          rd: RESERVED_REG_BOPRD,
          imm: sp_offset,
        }
        .into(),
      ));
      self.create(BOp::new(
        BType::U64,
        vec![],
        MOpData::Add {
          rd: SP_BOPRD,
          rs1: SP_BOPRD,
          rs2: RESERVED_REG_BOPRD,
        }
        .into(),
      ));
    } else {
      self.create(BOp::new(
        BType::U64,
        vec![],
        MOpData::Addi {
          rd: SP_BOPRD,
          rs1: SP_BOPRD,
          imm: BOperand::IntImm(sp_offset),
        }
        .into(),
      ));
    }

    for saved in 0..self.slot_map.len() {
      let slot_id = self.slot_map[saved];
      // Ignore those registers that are not used and thus not saved.
      if slot_id == BOperand::Undef {
        continue;
      }
      let reg = Reg::from(saved as u8);
      let offset = self.legalize_offset(self.get_offset(slot_id));
      let value = BOperand::Reg(reg);
      let store_op = match_some! {
          target: offset,
          enu: BOperand,
          minor_arms: {
            BOperand::IntImm(_) | BOperand::Reg(_) => {
                self.select_store(self.get_operand_type(value), vec![], value, SP_BOPRD, Some(offset))
            }
          },
          uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
          uni_arm: {
            unreachable!("Expected integer immediate, found {:?}", offset);
          }
      };
      self.create(store_op);
    }
  }

  /// 1. Restore ra if the function is not a leaf.
  /// 2. Restore callee-saved registers
  /// 3. Move sp back
  fn epilogue(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    for saved in 0..self.slot_map.len() {
      let slot_id = self.slot_map[saved];
      if slot_id == BOperand::Undef {
        continue;
      }
      let reg = Reg::from(saved as u8);
      let offset = self.legalize_offset(self.get_offset(slot_id));
      let rd = BOperand::Reg(reg);
      let load_op = match_some! {
        target: offset,
        enu: BOperand,
        minor_arms: {
            BOperand::IntImm(_) | BOperand::Reg(_) => {
                self.select_load(self.get_operand_type(rd), vec![], rd, SP_BOPRD, Some(offset))
            }
        },
        uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
        uni_arm: {
            unreachable!("Expected integer immediate, found {:?}", offset);
        }
      };
      self.create(load_op);
    }

    let sp_offset = self.get_func(func_id).frame_info.size() as i32;

    if !(INT_IMM_MIN..=INT_IMM_MAX).contains(&sp_offset) {
      self.create(BOp::new(
        BType::I32,
        vec![],
        MOpData::Li {
          rd: RESERVED_REG_BOPRD,
          imm: sp_offset,
        }
        .into(),
      ));
      self.create(BOp::new(
        BType::U64,
        vec![],
        MOpData::Add {
          rd: SP_BOPRD,
          rs1: SP_BOPRD,
          rs2: RESERVED_REG_BOPRD,
        }
        .into(),
      ));
    } else {
      self.create(BOp::new(
        BType::U64,
        vec![],
        MOpData::Addi {
          rd: SP_BOPRD,
          rs1: SP_BOPRD,
          imm: BOperand::IntImm(sp_offset),
        }
        .into(),
      ));
    }
  }

  /// * Check whether the function is a leaf
  /// * Figure out the used registers
  /// * Allocate space for callee-saved registers & ra.
  fn pre_check(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let bb_ids = self.get_func(func_id).cfg.ids();
    let mut is_leaf = true;

    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      let inst_ids = self.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        let data = &self.get_func(func_id).dfg[inst_id].data;
        if data.is_call() {
          is_leaf = false;
        }
        let src = self
          .get_src(inst_id)
          .into_iter()
          .cloned()
          .collect::<Vec<_>>();
        for operand in src {
          if operand.is_phys() {
            self
              .used_phys
              .insert(u8::from(operand.get_phys_reg()) as usize);
          }
        }
        let rd = self.get_rd(inst_id).cloned();
        if let Some(rd) = rd {
          if rd.is_phys() {
            self.used_phys.insert(u8::from(rd.get_phys_reg()) as usize);
          }
        }
      }
    }

    // If the function is not a leaf, we should allocate space for ra.
    if !is_leaf {
      // Emm...though ra is actually caller-saved, we still allocate CalleeSaved for it.
      self.alloc_and_map_slot(
        Reg::X(XReg::Ra),
        Slot::CalleeSaved {
          typ: BType::U64,
          offset: 0,
        },
      );
    }
    // Allocate space for used callee-saved registers.
    let mut used_callee_saved = Self::get_callee_saved_bitset();
    used_callee_saved.bitand_assign(&self.used_phys);
    for reg in used_callee_saved.iter() {
      let reg = Reg::from(reg as u8);
      self.alloc_and_map_slot(
        reg,
        Slot::CalleeSaved {
          typ: BType::U64,
          offset: 0,
        },
      );
    }
  }

  /// * Load/Store Lowering
  /// * Prologue/Epilogue Insertion
  fn frame_lowering(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let bb_ids = self.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      self.builder.set_current_block(bb_id);

      let inst_ids = self.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        let op = &self.get_func(func_id).dfg[inst_id];
        let (op_data, rd_typ, attrs) = (op.data.clone(), op.typ.clone(), op.attrs.clone());

        if let BOpData::L(LOpData::Store { addr, value }) = op_data {
          self.builder.set_before_inst(
            self.ir.as_mut().unwrap(),
            self.builder.current_function,
            Some(inst_id),
          );

          #[cfg(feature = "debug")]
          yachiyo::debug::info!(
            "Lowering store instruction. inst_id: {:?}, addr: {:?}, value: {:?}",
            inst_id,
            addr,
            value
          );

          let store_op = match_some! {
              target: addr,
              enu: BOperand,
              minor_arms: {
                  BOperand::Slot(_) => {
                      let offset = self.legalize_offset(self.get_offset(addr));
                      match_some! {
                          target: offset,
                          enu: BOperand,
                          minor_arms: {
                              BOperand::IntImm(_) | BOperand::Reg(_) => {
                                  self.select_store(self.get_operand_type(value), attrs, value, SP_BOPRD, Some(offset))
                              }
                          },
                          uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
                          uni_arm: {
                              unreachable!("Expected integer immediate, found {:?}", offset);
                          }
                      }
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_) => {
                      // We still keep Data/RoData/Bss Id as the operand. DumpASM will find the global symbol of the id.
                      self.create(BOp::new(
                          BType::U64,
                          vec![],
                          MOpData::La {
                              rd: RESERVED_REG_BOPRD,
                              target: addr,
                          }.into()
                      ));
                      self.select_store(self.get_operand_type(value), attrs, value, RESERVED_REG_BOPRD, None)
                  },
                  BOperand::Reg(_) => {
                      self.select_store(self.get_operand_type(value), attrs, value, addr, None)
                  }
              },
              uni_ops: [IntImm, FloatImm, Func, BB, Inst, Undef],
              uni_arm: {
                  unreachable!("Expected memory enetities, found {:?}", addr);
              }
          };
          self.replace_op(inst_id, bb_id, store_op);
        } else if let BOpData::L(LOpData::Load { rd, addr }) = op_data {
          self.builder.set_before_inst(
            self.ir.as_mut().unwrap(),
            self.builder.current_function,
            Some(inst_id),
          );
          #[cfg(feature = "debug")]
          yachiyo::debug::info!(
            "Lowering load instruction. inst_id: {:?}, rd: {:?}, addr: {:?}",
            inst_id,
            rd,
            addr
          );
          let load_op = match_some! {
              target: addr,
              enu: BOperand,
              minor_arms: {
                  BOperand::Slot(_) => {
                      let offset = self.legalize_offset(self.get_offset(addr));
                      match_some! {
                          target: offset,
                          enu: BOperand,
                          minor_arms: {
                              BOperand::IntImm(_) | BOperand::Reg(_) => {
                                  self.select_load(rd_typ, attrs, rd, SP_BOPRD, Some(offset))
                              }
                          },
                          uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
                          uni_arm: {
                              unreachable!("Expected integer immediate, found {:?}", offset);
                          }
                      }
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_) => {
                      self.create(BOp::new(
                          BType::U64,
                          vec![],
                          MOpData::La {
                              rd: RESERVED_REG_BOPRD,
                              target: addr,
                          }.into()
                      ));
                      self.select_load(rd_typ, attrs, rd, RESERVED_REG_BOPRD, None)
                  },
                  BOperand::Reg(_) => {
                      self.select_load(rd_typ, attrs, rd, addr, None)
                  }
              },
              uni_ops: [IntImm, FloatImm, Func, BB, Inst, Undef],
              uni_arm: {
                  unreachable!("Expected memory enetities, found {:?}", addr);
              }
          };
          self.replace_op(inst_id, bb_id, load_op);

        // Pointer arithmetic lowering
        } else if let BOpData::L(LOpData::AddI { rd, lhs, rhs: addr }) = op_data {
          self.builder.set_before_inst(
            self.ir.as_mut().unwrap(),
            self.builder.current_function,
            Some(inst_id),
          );

          let rd_typ = self.get_operand_type(rd);
          let add_op = match_some! {
              target: addr,
              enu: BOperand,
              minor_arms: {
                  BOperand::Slot(_) => {
                      let offset = self.legalize_offset(self.get_offset(addr));
                      match_some! {
                          target: offset,
                          enu: BOperand,
                          minor_arms: {
                              BOperand::IntImm(_) | BOperand::Reg(_) => {
                                  self.select_ptr_add(rd_typ, attrs, rd, lhs, SP_BOPRD, Some(offset))
                              }
                          },
                          uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
                          uni_arm: {
                              unreachable!("Expected integer immediate, found {:?}", offset);
                          }
                      }
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_) => {
                      self.create(BOp::new(
                          BType::U64,
                          vec![],
                          MOpData::La {
                              rd: RESERVED_REG_BOPRD,
                              target: addr,
                          }.into()
                      ));
                      self.select_ptr_add(rd_typ, attrs, rd, lhs, RESERVED_REG_BOPRD, None)
                  },
                  BOperand::Reg(_) => {
                      self.select_ptr_add(rd_typ, attrs, rd, lhs, addr, None)
                  }
              },
              uni_ops: [IntImm, FloatImm, Func, BB, Inst, Undef],
              uni_arm: {
                  unreachable!("Expected memory enetities, found {:?}", addr);
              }
          };
          self.replace_op(inst_id, bb_id, add_op);
        } else if let BOpData::L(LOpData::SubI { rd, lhs, rhs: addr }) = op_data {
          self.builder.set_before_inst(
            self.ir.as_mut().unwrap(),
            self.builder.current_function,
            Some(inst_id),
          );

          let rd_typ = self.get_operand_type(rd);
          let sub_op = match_some! {
              target: addr,
              enu: BOperand,
              minor_arms: {
                  BOperand::Slot(_) => {
                      let offset = self.legalize_offset(self.get_offset(addr));
                      match_some! {
                          target: offset,
                          enu: BOperand,
                          minor_arms: {
                              BOperand::IntImm(_) | BOperand::Reg(_) => {
                                  self.select_ptr_sub(rd_typ, attrs, rd, lhs, SP_BOPRD, Some(offset))
                              }
                          },
                          uni_ops: [Reg, Func, BB, Inst, Slot, Undef, FloatImm, Data, RoData, Bss],
                          uni_arm: {
                              unreachable!("Expected integer immediate, found {:?}", offset);
                          }
                      }
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_) => {
                      self.create(BOp::new(
                          BType::U64,
                          vec![],
                          MOpData::La {
                              rd: RESERVED_REG_BOPRD,
                              target: addr,
                          }.into()
                      ));
                      self.select_ptr_sub(rd_typ, attrs, rd, lhs, RESERVED_REG_BOPRD, None)
                  },
                  BOperand::Reg(_) => {
                      self.select_ptr_sub(rd_typ, attrs, rd, lhs, addr, None)
                  }
              },
              uni_ops: [IntImm, FloatImm, Func, BB, Inst, Undef],
              uni_arm: {
                  unreachable!("Expected memory enetities, found {:?}", addr);
              }
          };
          self.replace_op(inst_id, bb_id, sub_op);
        } else if let BOpData::M(MOpData::Ret) = op_data {
          self.builder.set_before_inst(
            self.ir.as_mut().unwrap(),
            self.builder.current_function,
            Some(inst_id),
          );
          // Insert epilogue
          self.epilogue();
        }
      }
    }
  }
}

impl Default for RegAlloc<'_> {
  fn default() -> Self {
    Self {
      ir: None,
      builder: BBuilder::default(),
      slot_map: Vec::new(),
      used_phys: BitSet::new(),
      allocators: vec![
        // Run float first.
        Allocator::new(AllocatorType::Float),
        Allocator::new(AllocatorType::Int),
      ],
    }
  }
}

impl<'a> BPass<'a> for RegAlloc<'a> {
  fn name(&self) -> &str {
    "RegAlloc"
  }

  fn mount(&mut self, ir: &'a mut BackIR) {
    self.ir = Some(ir);
  }

  fn run(&mut self) {
    // Mount IR on allocators
    for allocator in self.allocators.iter_mut() {
      let ir_ptr = *self.ir.as_mut().unwrap() as *mut BackIR;
      unsafe {
        allocator.ir = Some(&mut *ir_ptr);
      }
    }

    for func_id in self.ir.as_ref().unwrap().funcs.collect_internal() {
      let func_id = BOperand::Func(func_id);
      self.init(func_id);
      self.reset();

      // ========== RA Phase ==========
      for allocator in self.allocators.iter_mut() {
        allocator.init(func_id);
        allocator.run();
      }

      // ========== Post-RA Phase ==========
      // Pre checking
      self.pre_check();
      // Build stack frame
      let func = self.get_func_mut(func_id);
      func.frame_info.build();
      // Prologue
      self.prologue();
      // Lower the frame
      self.frame_lowering();
    }
  }
}
