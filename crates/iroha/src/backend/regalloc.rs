//! Register allocation (RegAlloc) via Graph Coloring.
//! Based on Appel and George's paper Iterated Register Coalescing.
//! Reference: https://dl.acm.org/doi/10.1145/229542.229546

use std::ops::BitOr;

use crate::analysis::{LiveAnalysis, LiveOuts};
use yachiyo::analysis::analyze;
use yachiyo::ir::back::{
    BBuilder, BFunction, BOp, BOpData, BOperand, BType, BackIR, LOpData, MOpData, Reg, Slot,
    CALLEE_SAVED_FREGS, CALLEE_SAVED_XREGS, CALLER_SAVED_FREGS, CALLER_SAVED_XREGS, COLOR_FREGS,
    COLOR_XREGS,
};
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::{match_full_ops, match_some, match_src};
use yachiyo::utils::set::{array_set, ArraySet, BitSet};
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(PartialEq, Eq, Default)]
#[allow(unused)]
enum AllocatorType {
    #[default]
    Int,
    Float,
    Vector, // TODO: For future vectorization extension.
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
        self.adj_list
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
                    // TODO: Store type in vreg.
                    let first_def = vreg.defs[0];
                    let op = &self.get_func(func_id).dfg[first_def];
                    match &op.typ {
                        BType::I32 | BType::U64 => self.typ == AllocatorType::Int,
                        BType::F32 => self.typ == AllocatorType::Float,
                        BType::Void => false,
                    }
                }
                BOperand::Reg(Reg::F(_)) => self.typ == AllocatorType::Float,
                BOperand::Reg(Reg::X(_)) => self.typ == AllocatorType::Int,
            },
            uni_ops: [IntImm, FloatImm, BB, Inst, Func, Data, RoData, Slot, Undef, Extern],
            uni_arm: {
                false
            }
        }
    }

    #[inline(always)]
    fn get_src(&self, op_id: BOperand) -> Vec<BOperand> {
        let func_id = self.builder.current_function;
        self.ir.as_ref().unwrap().get_src(func_id, op_id)
    }

    #[inline(always)]
    fn get_rd(&self, op_id: BOperand) -> Option<BOperand> {
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
    fn get_op_type(&self, op_id: BOperand) -> BType {
        let func_id = self.builder.current_function.unwrap();
        let op = &self.get_func(func_id).dfg[op_id];
        op.typ.clone()
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
    fn get_colors(&self) -> Vec<Reg> {
        match self.typ {
            AllocatorType::Int => CALLEE_SAVED_XREGS
                .to_vec()
                .into_iter()
                .map(Reg::X)
                .chain(CALLER_SAVED_XREGS.to_vec().into_iter().map(Reg::X))
                .collect(),
            AllocatorType::Float => CALLEE_SAVED_FREGS
                .to_vec()
                .into_iter()
                .map(Reg::F)
                .chain(CALLER_SAVED_FREGS.to_vec().into_iter().map(Reg::F))
                .collect(),
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
                let rd = self.get_rd(*inst_id);

                // For move instructions, we need to handle them specially.
                let src = self.get_src(*inst_id);
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

                // Since SysY only produce 1 rd at most,
                // we don't need to add def to current live for building interference graph between current defs.

                if let Some(rd) = rd {
                    // Add interference edges between rd and all live-out nodes.
                    // All of the current live nodes are included, but we'll filter out non-target nodes in add_edge function.
                    for live_var in live.iter() {
                        self.add_edge(rd, *live_var);
                    }
                    // Remove def from live set
                    live = live.difference(&array_set![rd]);
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
        let x = self.get_alias(x);
        let y = self.get_alias(y);
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
                let rd = self.get_rd(m).unwrap();
                let src = self.get_src(m);
                assert!(src.len() == 1);
                if rd == n {
                    src[0]
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
            let mut ok_colors = self.get_colors();
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
                } else if let BOperand::Reg(Reg::Virt(id)) = w {
                    if let Some(c) = self.color[*id] {
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
                let alias_color =
                    self.color[self.get_alias(BOperand::Reg(Reg::Virt(n))).get_virt_id()];
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
        // Initialize a map original VirtId -> (SlotId, Type).
        let mut virt_to_slot: FxHashMap<BOperand, (BOperand, BType)> = FxHashMap::default();
        let mut new_temps = array_set![];

        for spilled in std::mem::take(&mut self.spilled_nodes).iter() {
            let vreg_id = BOperand::Reg(Reg::Virt(spilled));
            let (defs, uses) = {
                let func_id = self.builder.current_function.unwrap();
                let vreg = &self.get_func(func_id).vregs[vreg_id];
                (vreg.defs.clone(), vreg.uses.clone())
            };

            // Insert store after each definition of the spilled node.
            for def in defs {
                let bb_id = op_to_bb[def.get_inst_id()];
                self.builder.set_current_block(bb_id);
                self.builder.set_after_inst(
                    self.ir.as_mut().unwrap(),
                    self.builder.current_function,
                    Some(def),
                );

                // Allocate new slot
                let typ = self.get_op_type(def);
                let slot_id = self.alloc_slot(Slot::Local {
                    size: typ.size(),
                    align: typ.align(),
                    offset: 0,
                });
                virt_to_slot.insert(vreg_id, (slot_id, typ.clone()));

                let store_op = BOp::new(
                    typ,
                    vec![],
                    LOpData::Store {
                        addr: slot_id,
                        value: self.get_rd(def).unwrap(),
                    }
                    .into(),
                );

                self.create(store_op);
            }

            for (r#use, _) in uses {
                let bb_id = op_to_bb[r#use.get_inst_id()];
                self.builder.set_current_block(bb_id);
                self.builder.set_before_inst(
                    self.ir.as_mut().unwrap(),
                    self.builder.current_function,
                    Some(r#use),
                );

                let (slot_id, typ) = virt_to_slot[&vreg_id].clone();
                let load_op = BOp::new(
                    typ,
                    vec![],
                    LOpData::Load {
                        rd: BOperand::Undef,
                        addr: slot_id,
                    }
                    .into(),
                );

                let load_id = self.create(load_op);
                let load_vreg_id = self.get_rd(load_id).unwrap();
                new_temps.insert(load_vreg_id);

                // Replace the following use
                let remap_use = |operand: &mut BOperand| {
                    if *operand == vreg_id {
                        *operand = load_vreg_id;
                    }
                };
                let func_id = self.builder.current_function.unwrap();
                let op_data = &mut self.get_func_mut(func_id).dfg[r#use].data;
                match op_data {
                    BOpData::L(lop_data) => match_src! {
                        target: lop_data,
                        bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
                        bin_arm: LOpData { lhs, rhs } => {
                            remap_use(lhs);
                            remap_use(rhs);
                        },
                        un_ops: [Sitofp, Fptosi],
                        un_arm: LOpData { value } => {
                            remap_use(value);
                        },
                        fallback: {
                            LOpData::Store { addr, value } => {
                                remap_use(addr);
                                remap_use(value);
                            }
                            LOpData::Load { addr, .. } => {
                                remap_use(addr);
                            }
                            LOpData::Move { src, .. } => {
                                remap_use(src);
                            }
                            LOpData::Br { cond, .. } => {
                                remap_use(cond);
                            }
                            LOpData::Call { func } => {
                                remap_use(func);
                            }
                            LOpData::Jump { .. }
                            | LOpData::Ret
                            | LOpData::LoadIntImm { .. }
                            | LOpData::LoadFloatImm { .. } => {}
                        }
                    },
                    BOpData::M(mop_data) => match_src! {
                        target: mop_data,
                        bin_ops: [Addw, Subw, Mulw, Divw, Remw, Sllw, Srlw, Sraw, Slt, Sltu, Xor, FaddS, FsubS, FmulS, FdivS, FeqS, FltS, FleS, FneS, FgtS, FgeS],
                        bin_arm: MOpData { rs1, rs2 } => {
                            remap_use(rs1);
                            remap_use(rs2);
                        },
                        un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
                        un_arm: MOpData { rs } => {
                            remap_use(rs);
                        },
                        fallback: {
                            MOpData::La { target, .. } => {
                                remap_use(target);
                            }
                            MOpData::Slti { rs1, imm, .. }
                            | MOpData::Sltiu { rs1, imm, .. }
                            | MOpData::Addiw { rs1, imm, .. }
                            | MOpData::Subiw { rs1, imm, .. }
                            | MOpData::Muliw { rs1, imm, .. }
                            | MOpData::Diviw { rs1, imm, .. }
                            | MOpData::Remiw { rs1, imm, .. }
                            | MOpData::Slliw { rs1, imm, .. }
                            | MOpData::Srliw { rs1, imm, .. }
                            | MOpData::Sraiw { rs1, imm, .. }
                            | MOpData::Xori { rs1, imm, .. } => {
                                remap_use(rs1);
                                remap_use(imm);
                            }
                            MOpData::Lw { base, offset, .. }
                            | MOpData::Flw { base, offset, .. }
                            | MOpData::Ld { base, offset, .. } => {
                                remap_use(base);
                                remap_use(offset);
                            }
                            MOpData::Sw { rs, base, offset }
                            | MOpData::Fsw { rs, base, offset }
                            | MOpData::Sd { rs, base, offset } => {
                                remap_use(rs);
                                remap_use(base);
                                remap_use(offset);
                            }
                            MOpData::Bnez { rs, .. } => {
                                remap_use(rs);
                            }
                            MOpData::Beq { rs1, rs2, offset }
                            | MOpData::Bne { rs1, rs2, offset }
                            | MOpData::Blt { rs1, rs2, offset }
                            | MOpData::Bge { rs1, rs2, offset }
                            | MOpData::Bltu { rs1, rs2, offset }
                            | MOpData::Bgeu { rs1, rs2, offset } => {
                                remap_use(rs1);
                                remap_use(rs2);
                                remap_use(offset);
                            }
                            MOpData::Li { .. }
                            | MOpData::J { .. }
                            | MOpData::Call { .. }
                            | MOpData::Ret => {}
                        }
                    },
                }
            }
        }
        new_temps
    }

    fn rewrite(&mut self) {
        let func_id = self.builder.current_function.unwrap();

        for bb_id in self.get_func(func_id).cfg.collect() {
            let bb_id = BOperand::BB(bb_id);
            for inst_id in self.get_func(func_id).cfg[bb_id].cur.clone() {
                let mut op_data = self.get_func(func_id).dfg[inst_id].data.clone();
                let remap_operand = |operand: &mut BOperand| {
                    if !operand.is_virt() || !self.is_target(*operand) {
                        return;
                    }
                    let alias = self.get_alias(*operand);
                    if let BOperand::Reg(Reg::Virt(id)) = alias {
                        if !self.colored_nodes.contains(id) {
                            panic!("rewrite: virtual register v{} is not in colored_nodes", id);
                        }
                        let color = self.color[id].unwrap_or_else(|| {
                            panic!("rewrite: virtual register v{} has no assigned color", id)
                        });
                        *operand = BOperand::Reg(color);
                    } else if alias.is_phys() {
                        *operand = alias;
                    } else {
                        unreachable!("Alias can't be non-reg");
                    }
                };

                match &mut op_data {
                    BOpData::L(lop_data) => {
                        match_full_ops! {
                            target: lop_data,
                            bin_ops: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, Xor, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Shl, Shr, Sar],
                            bin_arm: LOpData { rd, lhs, rhs } => {
                                remap_operand(rd);
                                remap_operand(lhs);
                                remap_operand(rhs);
                            },
                            un_ops: [Sitofp, Fptosi],
                            un_arm: LOpData { rd, value } => {
                                remap_operand(rd);
                                remap_operand(value);
                            },
                            fallback: {
                                LOpData::Store { addr, value } => {
                                    remap_operand(addr);
                                    remap_operand(value);
                                }
                                LOpData::Load { rd, addr } => {
                                    remap_operand(rd);
                                    remap_operand(addr);
                                }
                                LOpData::Br { cond, .. } => {
                                    remap_operand(cond);
                                }
                                LOpData::Move { rd, src } => {
                                    remap_operand(rd);
                                    remap_operand(src);
                                }
                                LOpData::Call { func } => {
                                    remap_operand(func);
                                }
                                LOpData::Jump { .. }
                                | LOpData::Ret
                                | LOpData::LoadIntImm { .. }
                                | LOpData::LoadFloatImm { .. } => {}
                            }
                        }
                    }
                    BOpData::M(mop_data) => {
                        match_full_ops! {
                            target: mop_data,
                            bin_ops: [Addw, Subw, Mulw, Divw, Remw, Sllw, Srlw, Sraw, Slt, Sltu, Xor, FaddS, FsubS, FmulS, FdivS, FeqS, FneS, FltS, FgeS, FleS, FgtS],
                            bin_arm: MOpData { rd, rs1, rs2 } => {
                                remap_operand(rd);
                                remap_operand(rs1);
                                remap_operand(rs2);
                            },
                            un_ops: [FcvtWS, FcvtSW, FmvWX, FmvXW, Mv, FmvS],
                            un_arm: MOpData { rd, rs } => {
                                remap_operand(rd);
                                remap_operand(rs);
                            },
                            fallback: {
                                MOpData::Li { rd, .. } => {
                                    remap_operand(rd);
                                }
                                MOpData::La { rd, target } => {
                                    remap_operand(rd);
                                    remap_operand(target);
                                }
                                MOpData::Addiw { rd, rs1, imm }
                                | MOpData::Subiw { rd, rs1, imm }
                                | MOpData::Muliw { rd, rs1, imm }
                                | MOpData::Diviw { rd, rs1, imm }
                                | MOpData::Remiw { rd, rs1, imm }
                                | MOpData::Slliw { rd, rs1, imm }
                                | MOpData::Srliw { rd, rs1, imm }
                                | MOpData::Sraiw { rd, rs1, imm }
                                | MOpData::Slti { rd, rs1, imm }
                                | MOpData::Sltiu { rd, rs1, imm }
                                | MOpData::Xori { rd, rs1, imm } => {
                                    remap_operand(rd);
                                    remap_operand(rs1);
                                    remap_operand(imm);
                                }
                                MOpData::Lw { rd, base, offset }
                                | MOpData::Ld { rd, base, offset }
                                | MOpData::Flw { rd, base, offset } => {
                                    remap_operand(rd);
                                    remap_operand(base);
                                    remap_operand(offset);
                                }
                                MOpData::Sw { rs, base, offset }
                                | MOpData::Sd { rs, base, offset }
                                | MOpData::Fsw { rs, base, offset } => {
                                    remap_operand(rs);
                                    remap_operand(base);
                                    remap_operand(offset);
                                }
                                MOpData::J { target } => {
                                    remap_operand(target);
                                }
                                MOpData::Call { target } => {
                                    remap_operand(target);
                                }
                                MOpData::Bnez { rs, target } => {
                                    remap_operand(rs);
                                    remap_operand(target);
                                }
                                MOpData::Beq { rs1, rs2, offset }
                                | MOpData::Bne { rs1, rs2, offset }
                                | MOpData::Bge { rs1, rs2, offset }
                                | MOpData::Blt { rs1, rs2, offset }
                                | MOpData::Bgeu { rs1, rs2, offset }
                                | MOpData::Bltu { rs1, rs2, offset } => {
                                    remap_operand(rs1);
                                    remap_operand(rs2);
                                    remap_operand(offset);
                                }
                                MOpData::Ret => {}
                            }
                        }
                    }
                }
                // Write back to the slot after rewriting, avoiding borrow checker error.
                self.get_func_mut(func_id).dfg[inst_id].data = op_data;
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
            yachiyo::debug::info!(
                "Finished liveness analysis for register allocation. funcs_live_outs: {:?}",
                live_outs
            );
            // Build the interference graph.
            self.build(&live_outs);
            yachiyo::debug::info!(
                "Finished building interference graph for register allocation. adj_set: {:?}, degree: {:?}",
                self.adj_set,
                self.degree
            );
            // Make the initial worklist.
            self.make_worklist();
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
            yachiyo::debug::info!(
                "Finished main loop of register allocation. simplify_worklist: {:?}, worklist_moves: {:?}, freeze_worklist: {:?}, spill_worklist: {:?}, \nselect_stack: {:?}",
                self.simplify_worklist,
                self.worklist_moves,
                self.freeze_worklist,
                self.spill_worklist,
                self.select_stack,
            );
            // Assign colors to the nodes.
            self.assign_colors();
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
        yachiyo::debug::info!(
            "Finished register allocation for function v{}. \ncolored_nodes: {:?}, \nspilled_nodes: {:?}",
            func_id,
            self.colored_nodes,
            self.spilled_nodes
        );
        // Rewrite the program to replace the virtual registers.
        self.rewrite();
        // TODO: Translate high-level Load/Store.
    }
}

pub struct RegAlloc<'a> {
    ir: Option<&'a mut BackIR>,
    allocators: Vec<Allocator<'a>>,
}

impl Default for RegAlloc<'_> {
    fn default() -> Self {
        Self {
            ir: None,
            allocators: vec![
                // Run float first.
                // Allocator::new(AllocatorType::Float),
                Allocator::new(AllocatorType::Int),
            ],
        }
    }
}

impl<'a> BPass<'a> for RegAlloc<'a> {
    fn name(&self) -> &str {
        "Register Allocation"
    }

    fn mount(&mut self, ir: &'a mut BackIR) {
        self.ir = Some(ir);
    }

    fn run(&mut self) {
        for allocator in self.allocators.iter_mut() {
            // Mount IR on allocator
            let ir_ptr = *self.ir.as_mut().unwrap() as *mut BackIR;
            unsafe {
                allocator.ir = Some(&mut *ir_ptr);
            }
            for func_id in self.ir.as_ref().unwrap().funcs.ids() {
                allocator.init(BOperand::Func(func_id));
                allocator.run();
            }
        }
    }
}
