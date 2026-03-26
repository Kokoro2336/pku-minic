//! Register allocation (RegAlloc) via Graph Coloring.
//! Based on Appel and George's paper Iterated Register Coalescing.
//! Reference: https://dl.acm.org/doi/10.1145/229542.229546

use std::ops::BitOr;

use crate::analysis::{LiveAnalysis, LiveOuts};
use yachiyo::analysis::analyze;
use yachiyo::ir::back::{BFunction, BOperand, BackIR, Reg};
use yachiyo::pass::BPass;
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
    select_stack: BitSet,

    // ========== Moves Structures ==========
    // All of the following structures are indexed by InstId.
    /// Move instructions that has been coalesced.
    coalesced_moves: BitSet,
    /// Move instructions that cannot be coalesced. Constrained moves are included in it.
    frozen_moves: BitSet,
    /// Move instructions that is ready to be coalesced.
    worklist_moves: BitSet,
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
    /// Move instructions associated with each node.
    move_list: Vec<ArraySet<BOperand>>,
    /// Alias of each node.
    alias: Vec<BOperand>,
    /// Color assigned to each node.
    color: Vec<Option<BOperand>>,
}

impl Allocator<'_> {
    pub fn new(typ: AllocatorType) -> Self {
        let mut allocator = Self::default();
        allocator.typ = typ;
        allocator
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

    /// Add an undirected edge between u and v in the interference graph.
    fn add_edge(&mut self, u: BOperand, v: BOperand) {
        if u == v || self.adj_set.contains(&(u, v)) || self.adj_set.contains(&(v, u)) {
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
                    let rd = self
                        .get_rd(*inst_id)
                        .expect("Move instruction should have rd");
                    // Add the move instruction to src & rd's moveList.
                    for s in src.iter() {
                        // To avoid interference between src and rd, we substract src from live set temporarily.
                        live = live.difference(&array_set![s.to_owned()]);
                        self.move_list[s.get_virt_id()].insert(*inst_id);
                    }
                    self.move_list[rd.get_virt_id()].insert(*inst_id);
                    // Add the move instruction to worklistMoves.
                    self.worklist_moves.insert(inst_id.get_inst_id());
                }

                // Since SysY only produce 1 rd at most,
                // we don't need to add def to current

                if let Some(rd) = rd {
                    // Add interference edges between rd and all live-out nodes.
                    for live_var in live.iter() {
                        self.add_edge(rd, live_var.to_owned());
                    }
                }

                // Retrive src
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
            select_stack.insert(BOperand::Reg(Reg::Virt(s)));
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
        let mut excluded_moves = ArraySet::new();
        for m in self.active_moves.bitor(&self.worklist_moves).iter() {
            excluded_moves.insert(BOperand::Inst(m));
        }
        self.move_list[n.get_virt_id()]
            .clone()
            .intersection(&excluded_moves)
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
            if self.degree[vreg_id.get_virt_id()] >= yachiyo::config::PARAM_REG_MAX_NUM as usize {
                self.spill_worklist.push_back(vreg_id);
            } else if self.move_related(vreg_id) {
                self.freeze_worklist.push_back(vreg_id);
            } else {
                self.simplify_worklist.push_back(vreg_id);
            }
        }
    }
}

pub struct RegAlloc<'a> {
    ir: Option<&'a mut BackIR>,
}

impl<'a> BPass<'a> for RegAlloc<'a> {
    fn name(&self) -> &str {
        "Register Allocation"
    }

    fn mount(&mut self, ir: &'a mut BackIR) {
        self.ir = Some(ir);
    }

    fn run(&mut self) {
        let ir = self.ir.as_mut().unwrap();
        let (funcs_live_ins, funcs_live_outs) = analyze::<LiveAnalysis>(ir);
    }
}
