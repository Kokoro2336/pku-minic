//! Sparse Conditional Constant Propagation (SCCP).
//! Based on Wegman and Zadeck's paper Constant Propagation with Conditional Branches.
//! Reference: https://dl.acm.org/doi/10.1145/103135.103136

use yachiyo::base::Type;
use yachiyo::ir::mid::{Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
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
  cx: PassContext<'a>,

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
  // br_ops excluding ret.
  br_ops: Vec<Operand>,
  in_br_ops: BitSet,
}

impl SCCP<'_> {
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
      Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => Lattice::Constant(*operand),

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
      (Lattice::Constant(Operand::Int(i)), Type::Bool) => Lattice::Constant(Operand::Bool(*i != 0)),
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
    self.cx.set_current_func(Some(func_id));
    let func = self.cx.get_func(func_id);
    let entry = match func.cfg.entry {
      Some(e) => e,
      None => return, // empty function
    };

    self.lattices.clear();
    self.lattices.resize(func.dfg.storage.len(), Lattice::Top);

    self.executable.clear();
    self.visited.clear();
    self.edge_list.clear();

    self.edge_list.extend(
      func.cfg[entry]
        .succs
        .iter()
        .map(|(succ, _)| (Operand::BB(entry), *succ))
        .collect::<Vec<(Operand, Operand)>>(),
    );
    self.visited.insert(entry);

    self.inst_list.clear();
    self.in_inst_list.clear();

    self.br_ops.clear();
    self.in_br_ops.clear();
  }

  fn visit_expr(&mut self, op_id: Operand, bb_id: Operand) {
    let func_id = self.cx.current_func();
    let (op_data, val_typ) = {
      let op = &mut self.cx.get_func_mut(func_id).dfg[op_id];
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
            for (user, _) in self.cx.get_func(func_id).dfg[op_id].users.iter() {
                if !self.in_inst_list.contains(user.get_op_id()) {
                    self.in_inst_list.insert(user.get_op_id());
                    self.inst_list.push(*user);
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
            for (user, _) in self.cx.get_func(func_id).dfg[op_id].users.iter() {
                if !self.in_inst_list.contains(user.get_op_id()) {
                    self.in_inst_list.insert(user.get_op_id());
                    self.inst_list.push(*user);
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
                for (user, _) in self.cx.get_func(func_id).dfg[op_id].users.iter() {
                    if !self.in_inst_list.contains(user.get_op_id()) {
                        self.in_inst_list.insert(user.get_op_id());
                        self.inst_list.push(*user);
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
                                self.edge_list.push((bb_id, then_bb));
                            } else {
                                self.edge_list.push((bb_id, else_bb));
                            }
                        } else {
                            panic!("SCCP: condition of br must be a boolean constant: {:?}", c);
                        }
                    }
                    // Top requires conservative assumption, and Bottom requires no assumption. So we need to push both branches to the edge list.
                    Lattice::Bottom => {
                        self.edge_list.push((bb_id, then_bb));
                        self.edge_list.push((bb_id, else_bb));
                    }
                }
                // Push the terminator to the worklist for later rewriting.
                if !self.in_br_ops.contains(op_id.get_op_id()) {
                    self.in_br_ops.insert(op_id.get_op_id());
                    self.br_ops.push(op_id);
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
    let func_id = self.cx.current_func();
    let (op_data, val_typ) = {
      let op = &mut self.cx.get_func_mut(func_id).dfg[op_id];
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
      for (user, _) in self.cx.get_func(func_id).dfg[op_id].users.iter() {
        if !self.in_inst_list.contains(user.get_op_id()) {
          self.in_inst_list.insert(user.get_op_id());
          self.inst_list.push(*user);
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
          let phis = self.cx.get_all_ops_in_block(to, OpType::Phi);
          for phi in phis {
            self.visit_phi(phi);
          }
        }

        // If to is visited for the first time, we need to visit all non-phi instructions in the block.
        if !self.visited.contains(to.get_bb_id()) {
          self.visited.insert(to.get_bb_id());
          let non_phis = self.cx.get_all_non_phi_in_block(to);
          for non_phi in non_phis {
            self.visit_expr(non_phi, to);
          }
        }

        // If to only has only one outgoing edge, push succ to edge_list.
        let cfg = &mut self.cx.get_func_mut(self.cx.current_func()).cfg;
        if cfg[to.get_bb_id()].succs.len() == 1 {
          let (succ, _) = cfg[to.get_bb_id()].succs[0];
          self.edge_list.push((to, succ));
        }
      }

      // Handle instruction
      if let Some(op_id) = self.inst_list.pop() {
        // Critical: remove the inst first.
        self.in_inst_list.remove(op_id.get_op_id());

        let dfg = &mut self.cx.get_func_mut(self.cx.current_func()).dfg;
        let op_data = dfg[op_id].data.clone();
        if op_data.is(OpType::Phi) {
          self.visit_phi(op_id);
        } else {
          let bb_id = self.cx.get_func(self.cx.current_func()).op_to_bb[op_id];
          // If any incoming edge is executable, we need to visit the instruction.
          if self.visited.contains(bb_id.get_bb_id()) {
            self.visit_expr(op_id, bb_id);
          }
        }
      }
    }
  }

  // Rewrite the program based on the results of propagation. And then return the existing phi nodes after rewriting.
  fn rewrite(&mut self) {
    let func_id = self.cx.current_func();
    // Replace optimizable instructions with constants.
    self
      .lattices
      .iter()
      .enumerate()
      .for_each(|(op_id, lattice)| {
        if let Lattice::Constant(c) = lattice {
          let op_id = Operand::Value(op_id);
          self.cx.replace_all_uses(op_id, *c);
        }
      });

    // Replace br with jump if the condition is a constant.
    for br_op in std::mem::take(&mut self.br_ops) {
      let dfg = &mut self.cx.get_func_mut(self.cx.current_func()).dfg;
      let op = dfg[br_op].clone();
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
              let (target_bb, other_bb) = if b {
                (then_bb, else_bb)
              } else {
                (else_bb, then_bb)
              };
              let bb_id = self.cx.get_func(self.cx.current_func()).op_to_bb[br_op];
              self.cx.replace_op(
                br_op,
                bb_id,
                Op {
                  typ: Type::Void,
                  attrs: vec![],
                  data: OpData::Jump { target_bb },
                  users: vec![],
                },
              );
              // Slay the phi incoming in other_bb.
              let other_bb_phis = self.cx.get_all_ops_in_block(other_bb, OpType::Phi);
              for phi_id in other_bb_phis {
                let phi = self.cx.get_func(func_id).dfg[phi_id].data.clone();
                if let OpData::Phi { incomings } = phi {
                  for incoming in incomings.iter() {
                    if let PhiIncoming::Data { bb, .. } = incoming {
                      if *bb == bb_id {
                        self.cx.slay_phi_incoming(phi_id, bb_id);
                      }
                    }
                  }
                } else {
                  unreachable!()
                }
              }
            } else {
              unreachable!()
            }
          }
          _ => { /*do nothing*/ }
        }
      } else {
        panic!("SCCP rewrite: op is not a br node");
      }
    }
  }
}

impl<'a> Pass<'a> for SCCP<'a> {
  fn name(&self) -> &str {
    "SCCP"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.cx.mount(program);
  }
  fn run(&mut self) {
    let func_ids = self.cx.ir().funcs.collect_internal();
    for func_id in func_ids {
      self.init(Operand::Func(func_id));
      self.propagate();
      self.rewrite();
    }
  }
}
