//! Sparse Conditional Constant Propagation (SCCP).
//! Based on Wegman and Zadeck's paper Constant Propagation with Conditional Branches.
//! Reference: https://dl.acm.org/doi/10.1145/103135.103136

use yachiyo::base::Type;
use yachiyo::ir::mid::{Builder, Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::Pass;
use yachiyo::utils::arena::{Arena, ArenaItem};
use yachiyo::utils::r#match::match_src;
use yachiyo::utils::set::BitSet;

use rustc_hash::FxHashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Lattice {
    Top,
    Constant(Operand),
    Bottom,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct SCCP<'a> {
    program: Option<&'a mut IR>,
    builder: Builder,

    // HashSet<(from, to)> for edges in the control flow graph that are executable. Only store true.
    executable: FxHashSet<(usize, usize)>,
    // Lattices
    lattices: Vec<Lattice>,
    // Whether the block has been visited. We use BitSet for fast lookup.
    visited: BitSet,
    // Whether the instruction is already in the worklist. We use BitSet for fast lookup.
    in_inst_list: BitSet,

    // Worklists
    // Vec<(from, to)> for CFG edges that need to be processed.
    edge_list: Vec<(Operand, Operand)>,
    // Vec<(OpId, BBId)>
    inst_list: Vec<Operand>,

    // Ancillary infrastructure
    // We need to know the mapping from op_id to bb_id for phi nodes.
    op_to_bb: Vec<Operand>,
    // br_ops excluding ret.
    br_ops: Vec<Operand>,
    in_br_ops: BitSet,
}

impl<'a> SCCP<'a> {
    fn meet(lattices: Vec<Lattice>) -> Lattice {
        if lattices.is_empty() {
            return Lattice::Top;
        }
        let mut result = lattices[0].clone();
        for lattice in lattices.into_iter().skip(1) {
            result = Self::meet_two(result, lattice);
        }
        result
    }

    fn meet_two(old: Lattice, new: Lattice) -> Lattice {
        match (old, new) {
            (Lattice::Top, origin) | (origin, Lattice::Top) => origin,
            (Lattice::Constant(c1), Lattice::Constant(c2)) => {
                if c1 == c2 {
                    Lattice::Constant(c1)
                } else {
                    Lattice::Bottom
                }
            }
            (Lattice::Bottom, _) | (_, Lattice::Bottom) => Lattice::Bottom,
        }
    }

    // If the operand is a value, return its lattice.
    // If it's a constant, return the constant lattice.
    // The functio doesn't support other types of operands and will panic if called with them.
    fn get_lattice(&self, operand: &Operand) -> Lattice {
        match operand {
            Operand::Value(id) => self.lattices[*id].clone(),
            Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => {
                Lattice::Constant(operand.clone())
            }

            Operand::Undefined => Lattice::Top,

            Operand::Global(_) | Operand::Param { .. } => Lattice::Bottom,

            Operand::BB(_) | Operand::Func(_) => panic!(
                "SCCP get_lattice: operand {:?} is not a value or constant",
                operand
            ),
        }
    }

    fn fold(lhs: Lattice, rhs: Lattice, op_typ: OpType) -> Lattice {
        let (lhs, rhs) = match (lhs, rhs) {
            (Lattice::Constant(c1), Lattice::Constant(c2)) => (c1, c2),
            _ => panic!("SCCP fold: both operands must be constants"),
        };
        match (lhs, rhs) {
            (Operand::Int(i1), Operand::Int(i2)) => match &op_typ {
                OpType::AddI => Lattice::Constant(Operand::Int(i1 + i2)),
                OpType::SubI => Lattice::Constant(Operand::Int(i1 - i2)),
                OpType::MulI => Lattice::Constant(Operand::Int(i1 * i2)),
                OpType::DivI => Lattice::Constant(Operand::Int(i1 / i2)),
                OpType::ModI => Lattice::Constant(Operand::Int(i1 % i2)),
                OpType::Xor => Lattice::Constant(Operand::Int(i1 ^ i2)),
                OpType::Shl => Lattice::Constant(Operand::Int(i1 << i2)),
                OpType::Shr => Lattice::Constant(Operand::Int(i1 >> i2)),
                OpType::Sar => Lattice::Constant(Operand::Int(((i1 as i64) >> i2) as i32)),
                OpType::SNe => Lattice::Constant(Operand::Bool(i1 != i2)),
                OpType::SEq => Lattice::Constant(Operand::Bool(i1 == i2)),
                OpType::SGt => Lattice::Constant(Operand::Bool(i1 > i2)),
                OpType::SLt => Lattice::Constant(Operand::Bool(i1 < i2)),
                OpType::SGe => Lattice::Constant(Operand::Bool(i1 >= i2)),
                OpType::SLe => Lattice::Constant(Operand::Bool(i1 <= i2)),
                _ => panic!("{:?}'s operands can't be folded as integers", op_typ),
            },
            (Operand::Float(f1), Operand::Float(f2)) => {
                let (f1, f2) = (f32::from_bits(f1), f32::from_bits(f2));
                match &op_typ {
                    OpType::AddF => Lattice::Constant(Operand::Float((f1 + f2).to_bits())),
                    OpType::SubF => Lattice::Constant(Operand::Float((f1 - f2).to_bits())),
                    OpType::MulF => Lattice::Constant(Operand::Float((f1 * f2).to_bits())),
                    OpType::DivF => Lattice::Constant(Operand::Float((f1 / f2).to_bits())),
                    OpType::ONe => Lattice::Constant(Operand::Bool(f1 != f2)),
                    OpType::OEq => Lattice::Constant(Operand::Bool(f1 == f2)),
                    OpType::OGt => Lattice::Constant(Operand::Bool(f1 > f2)),
                    OpType::OLt => Lattice::Constant(Operand::Bool(f1 < f2)),
                    OpType::OLe => Lattice::Constant(Operand::Bool(f1 <= f2)),
                    OpType::OGe => Lattice::Constant(Operand::Bool(f1 >= f2)),
                    _ => panic!("{:?}'s operands can't be folded as floats", op_typ),
                }
            }
            (Operand::Bool(b1), Operand::Bool(b2)) => match &op_typ {
                OpType::Xor => Lattice::Constant(Operand::Bool(b1 ^ b2)),
                _ => panic!("{:?}'s operands can't be folded as booleans", op_typ),
            },
            _ => panic!("SCCP fold: both operands must be of the same type"),
        }
    }

    fn cast(lattice: Lattice, new_typ: Type) -> Lattice {
        match (&lattice, &new_typ) {
            (Lattice::Constant(Operand::Int(i)), Type::Float) => {
                Lattice::Constant(Operand::Float((*i as f32).to_bits()))
            }
            (Lattice::Constant(Operand::Int(i)), Type::Bool) => {
                Lattice::Constant(Operand::Bool(*i != 0))
            }
            (Lattice::Constant(Operand::Float(f)), Type::Int) => {
                Lattice::Constant(Operand::Int(f32::from_bits(*f) as i32))
            }
            (Lattice::Constant(Operand::Float(f)), Type::Bool) => {
                Lattice::Constant(Operand::Bool(f32::from_bits(*f) != 0.0))
            }
            (Lattice::Constant(Operand::Bool(b)), Type::Int) => {
                Lattice::Constant(Operand::Int(if *b { 1 } else { 0 }))
            }
            (Lattice::Constant(Operand::Bool(b)), Type::Float) => {
                Lattice::Constant(Operand::Float(if *b { 1.0f32 } else { 0.0f32 }.to_bits()))
            }
            /*Don't need to cast*/
            _ => lattice,
        }
    }

    fn init(&mut self, func_id: Operand) {
        self.builder.set_current_func(Some(func_id.clone()));
        let func = &self.program.as_ref().unwrap().funcs[func_id];
        let entry = match func.cfg.entry {
            Some(e) => e,
            None => return, // empty function
        };

        self.lattices.clear();
        self.lattices.resize(func.dfg.storage.len(), Lattice::Top);

        // map OpId to BBId
        self.op_to_bb.clear();
        self.op_to_bb.resize(func.dfg.storage.len(), Operand::BB(0));
        func.cfg
            .storage
            .iter()
            .enumerate()
            .for_each(|(bb_id, item)| {
                if let ArenaItem::Data(bb) = item {
                    for op_id in bb.cur.iter() {
                        self.op_to_bb[op_id.get_op_id()] = Operand::BB(bb_id);
                    }
                }
            });

        self.executable.clear();
        self.visited.clear();
        self.edge_list.clear();

        self.edge_list.extend(
            func.cfg[entry]
                .succs
                .iter()
                .map(|(succ, _)| (Operand::BB(entry), succ.clone()))
                .collect::<Vec<(Operand, Operand)>>(),
        );
        self.visited.insert(entry);

        self.inst_list.clear();
        self.in_inst_list.clear();

        self.br_ops.clear();
        self.in_br_ops.clear();
    }

    fn visit_expr(&mut self, op_id: Operand, bb_id: Operand) {
        let func_id = match self.builder.current_function.clone() {
            Some(id) => id,
            None => panic!("SCCP visit_expr: current_function is None"), // should not happen
        };
        let (op_data, val_typ) = {
            let op = &mut self.program.as_mut().unwrap().funcs[func_id.clone()].dfg[op_id.clone()];
            (op.data.clone(), op.typ.clone())
        };
        let old = Self::get_lattice(self, &op_id);

        match_src! {
            target: op_data.clone(),
            bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
            bin_arm: OpData { lhs, rhs } => {
                // Fold the const first
                let left_lattice = self.get_lattice(&lhs);
                let right_lattice = self.get_lattice(&rhs);
                if matches!(left_lattice, Lattice::Constant(_))
                    && matches!(right_lattice, Lattice::Constant(_))
                {
                    self.lattices[op_id.get_op_id()] =
                        Self::cast(
                            Self::fold(left_lattice, right_lattice, OpType::from(&op_data)),
                            val_typ,
                        );
                } else {
                    // If not foldable, we just meet the lattices of the operands.
                    let lattice_list =
                        vec![
                            // DO NOT invoke get_xx_id() directly. We just ignore non-value operands, never panics.
                            Self::get_lattice(self, &lhs),
                            Self::get_lattice(self, &rhs)
                        ];
                    self.lattices[op_id.get_op_id()] = Self::cast(Self::meet(lattice_list), val_typ);
                }

                if old == self.lattices[op_id.get_op_id()] {
                    return;
                }

                // If the lattice has changed, we need to propagate the change to users.
                for (user, _) in self.program.as_ref().unwrap().funcs[func_id.clone()].dfg[op_id.clone()].users.iter() {
                    if !self.in_inst_list.contains(user.get_op_id()) {
                        self.in_inst_list.insert(user.get_op_id());
                        self.inst_list.push(user.clone());
                    }
                }
            },
            un_ops: [Sitofp, Fptosi, Zext, Uitofp],
            un_arm: OpData { value } => {
                let operand_lattice = self.get_lattice(&value);
                self.lattices[op_id.get_op_id()] = Self::cast(operand_lattice, val_typ);

                if old == self.lattices[op_id.get_op_id()] {
                    return;
                }
                // If the lattice has changed, we need to propagate the change to users.
                for (user, _) in self.program.as_ref().unwrap().funcs[func_id.clone()].dfg[op_id.clone()].users.iter() {
                    if !self.in_inst_list.contains(user.get_op_id()) {
                        self.in_inst_list.insert(user.get_op_id());
                        self.inst_list.push(user.clone());
                    }
                }
            },
            fallback: {
                OpData::GEP { .. } | OpData::Load { .. } | OpData::Call { .. } => {
                    // TODO: We are not able to fold these instructions for now.
                    self.lattices[op_id.get_op_id()] = Lattice::Bottom;

                    if old == self.lattices[op_id.get_op_id()] {
                        return;
                    }
                    // If the lattice has changed, we need to propagate the change to users.
                    for (user, _) in self.program.as_ref().unwrap().funcs[func_id.clone()].dfg[op_id.clone()].users.iter() {
                        if !self.in_inst_list.contains(user.get_op_id()) {
                            self.in_inst_list.insert(user.get_op_id());
                            self.inst_list.push(user.clone());
                        }
                    }
                }

                OpData::Br {
                    cond,
                    then_bb,
                    else_bb,
                } => {
                    let cond_lattice = self.get_lattice(&cond);
                    match cond_lattice {
                        Lattice::Top => {/*do nothing*/}
                        Lattice::Constant(c) => {
                            if let Operand::Bool(b) = c {
                                if b {
                                    self.edge_list.push((bb_id.clone(), then_bb.clone()));
                                } else {
                                    self.edge_list.push((bb_id.clone(), else_bb.clone()));
                                }
                            } else {
                                panic!("SCCP: condition of br must be a boolean constant: {:?}", c);
                            }
                        }
                        // Top requires conservative assumption, and Bottom requires no assumption. So we need to push both branches to the edge list.
                        Lattice::Bottom => {
                            self.edge_list.push((bb_id.clone(), then_bb.clone()));
                            self.edge_list.push((bb_id.clone(), else_bb.clone()));
                        }
                    }
                    // Push the terminator to the worklist for later rewriting.
                    if !self.in_br_ops.contains(op_id.get_op_id()) {
                        self.in_br_ops.insert(op_id.get_op_id());
                        self.br_ops.push(op_id.clone());
                    }
                }

                OpData::Phi { .. } => unreachable!("Phi nodes should be handled in visit_phi()"),

                // Jump is unconditional, and it's been processed outside of visit_expr().
                OpData::Jump { .. }
                | OpData::Ret { .. }
                // SCCP doesn't care about these ops.
                | OpData::Alloca(_)
                | OpData::GlobalAlloca(_)
                | OpData::Store { .. }
                | OpData::Declare { .. } => {}
            }
        }
    }

    fn visit_phi(&mut self, op_id: Operand) {
        let func_id = match self.builder.current_function.clone() {
            Some(id) => id,
            None => panic!("SCCP visit_phi: current_function is None"), // should not happen
        };
        let (op_data, val_typ) = {
            let op = &mut self.program.as_mut().unwrap().funcs[func_id.clone()].dfg[op_id.clone()];
            (op.data.clone(), op.typ.clone())
        };
        let old = Self::get_lattice(self, &op_id);

        if let OpData::Phi { incomings } = op_data {
            let lattice_list = incomings
                .iter()
                .filter_map(|incoming| {
                    if let PhiIncoming::Data {
                        value,
                        bb: Operand::BB(bb_id),
                    } = incoming
                    {
                        // Checking whether the incoming edge is executable. If not, we just ignore this incoming value.
                        // This is the key to handling infeasible paths.
                        if !self.visited.contains(*bb_id) {
                            return None;
                        }
                        Some(self.get_lattice(value))
                    } else {
                        None
                    }
                })
                .collect::<Vec<Lattice>>();
            self.lattices[op_id.get_op_id()] = Self::cast(Self::meet(lattice_list), val_typ);

            if old == self.lattices[op_id.get_op_id()] {
                return;
            }
            // If the lattice has changed, we need to propagate the change to users.
            for (user, _) in self.program.as_ref().unwrap().funcs[func_id.clone()].dfg
                [op_id.clone()]
            .users
            .iter()
            {
                if !self.in_inst_list.contains(user.get_op_id()) {
                    self.in_inst_list.insert(user.get_op_id());
                    self.inst_list.push(user.clone());
                }
            }
        } else {
            panic!("SCCP visit_phi: op is not a phi node");
        }
    }

    fn propagate(&mut self) {
        while !(self.edge_list.is_empty() && self.inst_list.is_empty()) {
            // Handle flow graph edge
            if let Some((from, to)) = self.edge_list.pop() {
                if self
                    .executable
                    .contains(&(from.get_bb_id(), to.get_bb_id()))
                {
                    continue;
                }
                self.executable.insert((from.get_bb_id(), to.get_bb_id()));

                // Visit the successor block. We need to check all phi nodes in the successor block and update their lattices.
                {
                    let phis = self.program.as_deref_mut().unwrap().get_all_ops_in_block(
                        self.builder.current_function.clone(),
                        to.clone(),
                        OpType::Phi,
                    );
                    for phi in phis {
                        self.visit_phi(phi);
                    }
                }

                // If to is visited for the first time, we need to visit all non-phi instructions in the block.
                if !self.visited.contains(to.get_bb_id()) {
                    self.visited.insert(to.get_bb_id());
                    let non_phis = self
                        .program
                        .as_deref_mut()
                        .unwrap()
                        .get_all_non_phi_in_block(
                            self.builder.current_function.clone(),
                            to.clone(),
                        );
                    for non_phi in non_phis {
                        self.visit_expr(non_phi, to.clone());
                    }
                }

                // If to only has only one outgoing edge, push succ to edge_list.
                let cfg = &mut self.program.as_mut().unwrap().funcs
                    [self.builder.current_function.clone().unwrap()]
                .cfg;
                if cfg[to.get_bb_id()].succs.len() == 1 {
                    let (succ, _) = cfg[to.get_bb_id()].succs[0].clone();
                    self.edge_list.push((to.clone(), succ));
                }
            }

            // Handle instruction
            if let Some(op_id) = self.inst_list.pop() {
                // Critical: remove the inst first.
                self.in_inst_list.remove(op_id.get_op_id());

                let dfg = &mut self.program.as_mut().unwrap().funcs
                    [self.builder.current_function.clone().unwrap()]
                .dfg;
                let op_data = dfg[op_id.clone()].data.clone();
                if op_data.is(OpType::Phi) {
                    self.visit_phi(op_id.clone());
                } else {
                    // If any incoming edge is executable, we need to visit the instruction.
                    if self
                        .visited
                        .contains(self.op_to_bb[op_id.get_op_id()].get_bb_id())
                    {
                        self.visit_expr(op_id.clone(), self.op_to_bb[op_id.get_op_id()].clone());
                    }
                }
            }
        }
    }

    // Rewrite the program based on the results of propagation. And then return the existing phi nodes after rewriting.
    fn rewrite(&mut self) {
        // Replace optimizable instructions with constants.
        let removed = self
            .lattices
            .iter()
            .enumerate()
            .filter_map(|(op_id, lattice)| {
                if let Lattice::Constant(c) = lattice {
                    let bb_id = self.op_to_bb[op_id].clone();
                    let op_id = Operand::Value(op_id);
                    self.program.as_deref_mut().unwrap().replace_all_uses(
                        self.builder.current_function.clone(),
                        op_id.clone(),
                        c.clone(),
                    );
                    Some((op_id.clone(), bb_id.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<(Operand, Operand)>>();

        // Replace br with jump if the condition is a constant.
        for br_op in self.br_ops.iter() {
            let dfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .dfg;
            let op = dfg[br_op.clone()].clone();
            if let OpData::Br {
                cond,
                then_bb,
                else_bb,
            } = op.data
            {
                let cond_lattice = self.get_lattice(&cond);
                match cond_lattice {
                    Lattice::Constant(c) => {
                        if let Operand::Bool(b) = c {
                            let target_bb = if b { then_bb } else { else_bb };
                            let bb_id = self.op_to_bb[br_op.get_op_id()].clone();
                            let current_function = self.builder.current_function.clone();
                            self.program.as_deref_mut().unwrap().replace_op(
                                &mut self.builder,
                                current_function,
                                br_op.clone(),
                                bb_id,
                                Op {
                                    typ: Type::Void,
                                    attrs: vec![],
                                    data: OpData::Jump { target_bb },
                                    users: vec![],
                                },
                            );
                        } else {
                            panic!("SCCP: condition of br must be a boolean constant");
                        }
                    }
                    _ => { /*do nothing*/ }
                }
            } else {
                panic!("SCCP rewrite: op is not a br node");
            }
        }

        // Slay the edge of dead block in phi operations.
        let phis = self
            .program
            .as_deref_mut()
            .unwrap()
            .get_all_ops(self.builder.current_function.clone(), OpType::Phi);
        for phi_op in &phis {
            let dfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .dfg;
            let op = dfg[phi_op.clone()].clone();
            if let OpData::Phi { incomings } = op.data {
                for incoming in incomings.iter() {
                    if let PhiIncoming::Data { bb, .. } = incoming {
                        if let Operand::BB(bb_id) = bb {
                            // Check whether the block is dead or the current block is no longer the successor of the incoming block.
                            // If so, we need to slay this incoming edge.
                            let current_bb = self.op_to_bb[phi_op.get_op_id()].clone();
                            let cfg = &mut self.program.as_mut().unwrap().funcs
                                [self.builder.current_function.clone().unwrap()]
                            .cfg;
                            let ans_succ = &cfg[*bb_id]
                                .succs
                                .iter()
                                .map(|(succ, _)| succ.clone())
                                .collect::<Vec<Operand>>();

                            if !self.visited.contains(*bb_id) || !ans_succ.contains(&current_bb) {
                                self.program.as_deref_mut().unwrap().slay_phi_incoming(
                                    self.builder.current_function.clone(),
                                    phi_op.clone(),
                                    bb.clone(),
                                );
                            }
                        } else {
                            panic!("SCCP rewrite: phi incoming bb is not a BB operand");
                        }
                    }
                }
            } else {
                panic!("SCCP rewrite: op is not a phi node");
            }
        }

        // Remove the ops
        removed.into_iter().for_each(|(op_id, bb_id)| {
            self.program.as_deref_mut().unwrap().remove_op(
                self.builder.current_function.clone(),
                op_id,
                Some(bb_id),
            );
        });

        let dead_blocks = self.program.as_ref().unwrap().funcs
            [self.builder.current_function.clone().unwrap()]
        .cfg
        .collect()
        .into_iter()
        .filter(|bb_id| !self.visited.contains(*bb_id))
        .collect::<FxHashSet<usize>>();

        // Phase 1: Isolate the dead blocks, disconnect the edges from live blocks to dead blocks.
        dead_blocks.iter().for_each(|bb_id| {
            let (last, terminator) = {
                let cfg = &mut self.program.as_mut().unwrap().funcs
                    [self.builder.current_function.clone().unwrap()]
                .cfg;
                let bb = &cfg[*bb_id];
                let last = match bb.cur.last() {
                    Some(last) => last.clone(),
                    None => return,
                };
                let data = {
                    let dfg = &mut self.program.as_mut().unwrap().funcs
                        [self.builder.current_function.clone().unwrap()]
                    .dfg;
                    dfg[last.clone()].data.clone()
                };
                (last, data)
            };
            if matches!(terminator, OpData::Br { .. } | OpData::Jump { .. }) {
                // remove the op
                self.program.as_deref_mut().unwrap().remove_op(
                    self.builder.current_function.clone(),
                    last.clone(),
                    Some(Operand::BB(*bb_id)),
                );
            }
        });

        // Phase 2: Check users in dead blocks.
        for bb_id in &dead_blocks {
            let cfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .cfg;
            let cur = cfg[*bb_id].cur.clone();

            // Split users check and removal due to data dependency.
            for inst in cur.iter().rev() {
                let func_id = self.builder.current_function.clone().unwrap();
                let funcs = &mut self.program.as_mut().unwrap().funcs;
                let dfg = &mut funcs[func_id].dfg;

                // inst can be used by the instructions inside the block, but it cannot be used by instructions outside the block.
                let users = dfg[inst.get_op_id()].users.clone();
                for (user, _) in users {
                    let user_bb = self.op_to_bb[user.get_op_id()].clone();
                    // The user can be in the same block, or in another dead block. But it cannot be in a live block.
                    if dead_blocks.contains(&user_bb.get_bb_id()) {
                        // continue. users will be removed later.
                        continue;
                    }
                    panic!(
                        "Builder remove_block: instruction {:#?} has user {:#?} outside the block",
                        dfg[inst.get_op_id()],
                        dfg[user.get_op_id()]
                    );
                }

                // Check whether the instruction uses a value outside dead block. If so, remove the use first.
                let data = dfg[inst.clone()].data.clone();
                let op = inst.clone();
                let is_live_value = |operand: &Operand| match operand {
                    Operand::Value(id) => {
                        let bb = self.op_to_bb[*id].get_bb_id();
                        !dead_blocks.contains(&bb)
                    }
                    _ => false,
                };

                match_src! {
                    target: data,
                    bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
                    bin_arm: OpData { lhs, rhs } => {
                        if is_live_value(&lhs) {
                            dfg.remove_use(lhs.clone(), (op.clone(), 0));
                        }
                        if is_live_value(&rhs) {
                            dfg.remove_use(rhs.clone(), (op.clone(), 1));
                        }
                    },
                    un_ops: [Sitofp, Fptosi, Zext, Uitofp],
                    un_arm: OpData { value } => {
                        if is_live_value(&value) {
                            dfg.remove_use(value.clone(), (op.clone(), 0));
                        }
                    },
                    fallback: {
                        OpData::Load { addr } => {
                            // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                            if is_live_value(&addr) {
                                dfg.remove_use(addr, (op, 0));
                            }
                        }
                        OpData::Store { addr, value } => {
                            // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                            if is_live_value(&addr) {
                                dfg.remove_use(addr, (op.clone(), 0));
                            }
                            if is_live_value(&value) {
                                dfg.remove_use(value, (op, 1));
                            }
                        }
                        OpData::Br { cond, .. } => {
                            if is_live_value(&cond) {
                                dfg.remove_use(cond, (op, 0));
                            }
                        }
                        OpData::Call { args, .. } => {
                            for (i, arg) in args.iter().enumerate() {
                                if is_live_value(arg) {
                                    dfg.remove_use(arg.clone(), (op.clone(), i + 1));
                                }
                            }
                        }
                        OpData::Ret { value } => {
                            if let Some(val) = value {
                                if is_live_value(&val) {
                                    dfg.remove_use(val, (op, 0));
                                }
                            }
                        }
                        OpData::Phi { incomings } => {
                            for (i, phi_incoming) in incomings.iter().enumerate() {
                                if let PhiIncoming::Data { value, .. } = phi_incoming {
                                    if is_live_value(value) {
                                        dfg.remove_use(value.clone(), (op.clone(), i + 1));
                                    }
                                }
                            }
                        }

                        OpData::GEP { base, indices } => {
                            // TODO(SCCP): Re-enable global use-list maintenance after rewrite/dead-block phases avoid stale-use removals.
                            if is_live_value(&base) {
                                dfg.remove_use(base, (op.clone(), 0));
                            }
                            for (i, index) in indices.iter().enumerate() {
                                if is_live_value(index) {
                                    dfg.remove_use(index.clone(), (op.clone(), i + 1));
                                }
                            }
                        }

                        OpData::GlobalAlloca(_)
                        | OpData::Alloca(_)
                        | OpData::Jump { .. }
                        | OpData::Declare { .. } => {}
                    }
                }
            }
        }

        // Phase 3: Remove the instructions in dead blocks directly by dfg.
        for bb_id in &dead_blocks {
            let cfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .cfg;
            let cur = cfg[*bb_id].cur.clone();
            let dfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .dfg;
            for inst in cur.iter().rev() {
                // Remove the uses
                dfg.remove(inst.get_op_id());
            }
        }

        // Phase 4: Remove the blocks directly by cfg.
        for bb_id in dead_blocks {
            // remove the block from cfg
            let cfg = &mut self.program.as_mut().unwrap().funcs
                [self.builder.current_function.clone().unwrap()]
            .cfg;
            cfg.remove(bb_id);
        }
    }
}

impl<'a> Pass<'a> for SCCP<'a> {
    fn name(&self) -> &str {
        "SCCP"
    }
    fn mount(&mut self, program: &'a mut IR) {
        self.program = Some(program);
    }
    fn run(&mut self) {
        let program = self.program.as_mut().unwrap();
        let func_ids = program.funcs.collect_internal();
        for func_id in func_ids {
            self.init(Operand::Func(func_id));
            self.propagate();
            self.rewrite();
        }
    }
}
