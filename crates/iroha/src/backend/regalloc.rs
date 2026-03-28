//! Register allocation (RegAlloc) via Graph Coloring.
//! Based on Appel and George's paper Iterated Register Coalescing.
//! Reference: https://dl.acm.org/doi/10.1145/229542.229546

use std::ops::BitOr;

use crate::analysis::{LiveAnalysis, LiveOuts};
use yachiyo::analysis::analyze;
use yachiyo::ir::back::{
    BFunction, BOperand, BType, BackIR, Reg, CALLEE_SAVED_FREGS, CALLEE_SAVED_XREGS,
    CALLER_SAVED_FREGS, CALLER_SAVED_XREGS, COLOR_FREGS, COLOR_XREGS,
};
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::match_some;
use yachiyo::utils::set::{array_set, ArraySet, BitSet};
use yachiyo::utils::worklist::{Worklist, WorklistTrait};

use rustc_hash::FxHashSet;

#[derive(PartialEq, Eq, Default)]
enum AllocatorType {
    #[default]
    Int,
    Float,
    Vector, // TODO: For future vectorization extension.
}

#[derive(Default)]
struct Allocator<'a> {
    ir: Option<&'a mut BackIR>,
    current_function: Option<BOperand>,

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
            current_function: None,
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

    fn init(&mut self, func_id: BOperand) {
        self.current_function = Some(func_id);

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

        // Clear the move list.
        self.move_list.clear();
        self.alias.clear();
        self.color.clear();
    }

    // ========= Helper Functions ==========

    #[inline(always)]
    fn is_target(&self, op_id: BOperand) -> bool {
        let func_id = self.current_function.unwrap();
        match_some! {
            target: op_id,
            enu: BOperand,
            minor_arms: {
                BOperand::Reg(Reg::Virt(_)) => {
                    let op = &self.get_func(func_id).dfg[op_id];
                    match &op.typ {
                        BType::I32 | BType::U64 => self.typ == AllocatorType::Int,
                        BType::F32 => self.typ == AllocatorType::Float,
                        BType::Void => false,
                    }
                }
                BOperand::Reg(Reg::F(_)) => self.typ == AllocatorType::Float,
                BOperand::Reg(Reg::X(_)) => self.typ == AllocatorType::Int,
            },
            uni_ops: [IntImm, FloatImm, BB, Inst, Func, Data, RoData, Slot, Undef],
            uni_arm: {
                false
            }
        }
    }

    #[inline(always)]
    fn get_src(&self, op_id: BOperand) -> Vec<BOperand> {
        let func_id = self.current_function;
        self.ir.as_ref().unwrap().get_src(func_id, op_id)
    }

    #[inline(always)]
    fn get_rd(&self, op_id: BOperand) -> Option<BOperand> {
        let func_id = self.current_function;
        self.ir.as_ref().unwrap().get_rd(func_id, op_id)
    }

    #[inline(always)]
    fn get_func<'a>(&'a self, func_id: BOperand) -> &'a BFunction {
        &self.ir.as_ref().unwrap().funcs[func_id]
    }

    #[inline(always)]
    fn get_func_mut<'a>(&'a mut self, func_id: BOperand) -> &'a mut BFunction {
        &mut self.ir.as_mut().unwrap().funcs[func_id]
    }

    #[inline(always)]
    fn get_colors(&self) -> Vec<Reg> {
        match self.typ {
            AllocatorType::Int => CALLEE_SAVED_XREGS
                .to_vec()
                .into_iter()
                .map(|r| Reg::X(r))
                .chain(CALLER_SAVED_XREGS.to_vec().into_iter().map(|r| Reg::X(r)))
                .collect(),
            AllocatorType::Float => CALLEE_SAVED_FREGS
                .to_vec()
                .into_iter()
                .map(|r| Reg::F(r))
                .chain(CALLER_SAVED_FREGS.to_vec().into_iter().map(|r| Reg::F(r)))
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
        let func_id = self.current_function.unwrap();
        let cfg_ids = self.get_func(func_id).cfg.ids();

        for bb_id in cfg_ids {
            let cur = &self.get_func(func_id).cfg[bb_id].cur.clone();
            let mut live = live_outs[bb_id].clone();

            for inst_id in cur.iter().rev() {
                let op = &self.get_func(func_id).dfg[*inst_id];
                let rd = self.get_rd(*inst_id);

                // For move instructions, we need to handle them specially.
                let src = self.get_src(*inst_id);
                if op.data.is_move() {
                    let rd = rd.expect("Move instruction should have rd");
                    // Ignore move that is irrelevant to current allocator.
                    if self.is_target(rd.to_owned()) {
                        // Add the move instruction to src & rd's moveList.
                        for s in src.iter() {
                            // To avoid interference between src and rd, we substract src from live set temporarily.
                            live = live.difference(&array_set![s.to_owned()]);
                            self.move_list[s.get_virt_id()].insert(*inst_id);
                        }
                        self.move_list[rd.get_virt_id()].insert(*inst_id);
                        // Add the move instruction to worklistMoves.
                        self.worklist_moves.push_back(inst_id.to_owned());
                    }
                }

                // Since SysY only produce 1 rd at most,
                // we don't need to add def to current live for building interference graph between current defs.

                if let Some(rd) = rd {
                    // Add interference edges between rd and all live-out nodes.
                    // All of the current live nodes are included, but we'll filter out non-target nodes in add_edge function.
                    for live_var in live.iter() {
                        self.add_edge(rd, live_var.to_owned());
                    }
                }

                // Retrieve src
                for s in src {
                    live.insert(s);
                }
            }
        }
    }

    #[inline(always)]
    fn adjacent(&self, n: BOperand) -> Vec<BOperand> {
        let mut select_stack = ArraySet::new();
        for s in self.select_stack.iter() {
            select_stack.insert(s.to_owned());
        }
        let mut coalesced_nodes = ArraySet::new();
        for n in self.coalesced_nodes.iter() {
            coalesced_nodes.insert(BOperand::Reg(Reg::Virt(n)));
        }
        self.adj_list[n.get_virt_id()]
            .clone()
            .difference(&select_stack)
            .difference(&coalesced_nodes)
            .iter()
            .cloned()
            .collect()
    }

    #[inline(always)]
    fn node_moves(&self, n: BOperand) -> Vec<BOperand> {
        let mut included_moves = ArraySet::new();
        for m in self
            .active_moves
            .bitor(self.worklist_moves.get_in_list())
            .iter()
        {
            included_moves.insert(BOperand::Inst(m));
        }
        self.move_list[n.get_virt_id()]
            .clone()
            .intersection(&included_moves)
            .iter()
            .cloned()
            .collect()
    }

    #[inline(always)]
    fn move_related(&self, n: BOperand) -> bool {
        !self.node_moves(n).is_empty()
    }

    fn make_worklist(&mut self) {
        let vregs_ids = self.get_func(self.current_function.unwrap()).vregs.ids();

        for vreg_id in vregs_ids {
            let vreg_id = BOperand::Reg(Reg::Virt(vreg_id));
            // Nodes that are not target nodes should not be added to worklists.
            if !self.is_target(vreg_id) {
                continue;
            }

            if self.degree[vreg_id.get_virt_id()] >= self.get_colors_num() {
                self.spill_worklist.push_back(vreg_id);
            } else if self.move_related(vreg_id) {
                self.freeze_worklist.push_back(vreg_id);
            } else {
                self.simplify_worklist.push_back(vreg_id);
            }
        }
    }

    fn simplify(&mut self) {
        let n = self.simplify_worklist.pop_back().unwrap();
        if !self.is_target(n) {
            return;
        }
        self.select_stack.push(n);
        for m in self.adjacent(n) {
            self.decrement_degree(m);
        }
    }

    fn decrement_degree(&mut self, n: BOperand) {
        let d = self.degree[n.get_virt_id()];
        self.degree[n.get_virt_id()] = d - 1;
        // If the degree of n drops below the number of colors, we can enable n and its adjacent nodes m.
        if d == self.get_colors_num() {
            // Enable n and its adjacent nodes m.
            let mut nodes = vec![n];
            nodes.extend(self.adjacent(n));
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
        let m = self.worklist_moves.pop_back().unwrap();
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
            self.frozen_moves.insert(m.into());
            self.add_worklist(u);
            self.add_worklist(v);
        } else if (u.is_phys() && self.ok(u, v)) || (u.is_virt() && self.conservative(u, v)) {
            self.coalesced_moves.insert(m.into());
            self.combine(u, v);
            // Since v is combined, we just need to add u to worklist.
            self.add_worklist(u);
        } else {
            // If none of the above conditions hold, we can't coalesce m now. We put it back to active_moves and try it later.
            self.active_moves.insert(m.into());
        }
    }

    fn combine(&mut self, u: BOperand, v: BOperand) {
        if self.freeze_worklist.contains(&v) {
            self.freeze_worklist.remove(&v);
        } else {
            self.spill_worklist.remove(&v);
        }
        self.coalesced_nodes.insert(v.get_virt_id());
        // Set alias
        self.alias[v.get_virt_id()] = u;
        // Combine the nodes' node_moves(NOT original move_list).
        self.move_list[u.get_virt_id()] = self.move_list[u.get_virt_id()]
            .clone()
            .union(&self.move_list[v.get_virt_id()]);
        // Update interference graph of u.
        for t in self.adjacent(v) {
            self.add_edge(t, u);
            // Decrease degree of t since add_edge increase the degree of t.
            self.decrement_degree(t);
        }
        // u can't be in simplify_worklist now.
        if self.degree[u.get_virt_id()] >= self.get_colors_num()
            && self.freeze_worklist.contains(&u)
        {
            self.freeze_worklist.remove(&u);
            self.spill_worklist.push_back(u);
        }
    }

    /// TODO: Briggs' conservative coalescing test.
    fn conservative(&self, u: BOperand, v: BOperand) -> bool {
        let mut adjacent_nodes = self.adjacent(u);
        adjacent_nodes.extend(self.adjacent(v));
        let k = adjacent_nodes
            .iter()
            .filter(|n| self.degree[n.get_virt_id()] >= self.get_colors_num())
            .count();
        k < self.get_colors_num()
    }

    /// TODO: George test.
    fn ok(&self, t: BOperand, r: BOperand) -> bool {
        self.degree[t.get_virt_id()] < self.get_colors_num()
            || t.is_phys()
            || self.adj_set.contains(&(t, r))
    }

    /// Add n to simplify_worklist.
    fn add_worklist(&mut self, n: BOperand) {
        if self.colored_nodes.contains(n.get_virt_id())
            // n might still lie in freeze_worklist but no longer be move-related after coalescing.
            || self.move_related(n)
            || self.degree[n.get_virt_id()] >= self.get_colors_num()
        {
            return;
        }
        self.freeze_worklist.remove(&n);
        self.simplify_worklist.push_back(n);
    }

    fn get_alias(&self, n: BOperand) -> BOperand {
        if self.coalesced_nodes.contains(n.get_virt_id()) {
            self.get_alias(self.alias[n.get_virt_id()])
        } else {
            n
        }
    }

    /// Free the node from freeze_worklist and freeze all of its moves.
    /// It means that all of its related move is given up to coalesce.
    fn freeze(&mut self) {
        let u = self.freeze_worklist.pop_back().unwrap();
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
            if self.degree[v.get_virt_id()] < self.get_colors_num() && self.node_moves(v).is_empty()
            {
                self.freeze_worklist.remove(&v);
                self.simplify_worklist.push_back(v);
            }
        }
    }

    /// Select a node for spilling and add it to simplify_worklist.
    fn select_spill(&mut self) {
        let n = self.spill_worklist.pop_back().unwrap();
        self.simplify_worklist.push_back(n);
        self.freeze_moves(n);
    }

    fn assign_colors(&mut self) {
        while let Some(n) = self.select_stack.pop() {
            let mut ok_colors = self.get_colors();
            for w in self.adj_list[n.get_virt_id()].iter() {
                if let Some(c) = self.color[w.get_virt_id()] {
                    ok_colors.retain(|&color| color != c);
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
            self.color[n] = self.color[self.get_alias(BOperand::Reg(Reg::Virt(n))).get_virt_id()];
        }
    }

    /// TODO: rewrite the program after coloring.
    fn rewrite_program(&mut self) {
        todo!()
    }

    /// Main function of register allocation.
    /// LiveOuts is the result of current function's liveness analysis, which is used for building the interference graph.
    fn allocate(&mut self, live_outs: &LiveOuts) {
        loop {
            // Build the interference graph.
            self.build(live_outs);
            // Make the initial worklist.
            self.make_worklist();

            // Main loop.
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
            // Assign colors to the nodes.
            self.assign_colors();
            // If there is no spill, we are done.
            if self.spilled_nodes.is_empty() {
                break;
            }
            // Rewrite the program to insert spill code, and then rerun the whole process until there is no spill.
            self.rewrite_program();
        }
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
                Allocator::new(AllocatorType::Float),
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
            // Liveness analysis for the whole program.
            let ir = self.ir.as_mut().unwrap();
            let (_, funcs_live_outs) = analyze::<LiveAnalysis>(ir);

            for func_id in self.ir.as_ref().unwrap().funcs.ids() {
                allocator.init(BOperand::Func(func_id));
                allocator.allocate(&funcs_live_outs[func_id]);
            }
        }
    }
}
