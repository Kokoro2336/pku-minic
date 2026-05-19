//! Loop Unrolling.

use crate::analysis::{DomAnalysis, LoopAnalysis, SCEV};

use yachiyo::analysis::{analyze, AddRecInfo, Analysis, LoopId};
use yachiyo::ir::mid::{Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::{match_src, BitSet};

use rustc_hash::FxHashMap;
use std::ops::Range;

const MAX_UNROLL_COUNT: i64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CmpKind {
  Lt,
  Le,
  Gt,
  Ge,
}

impl Default for CmpKind {
  fn default() -> Self {
    Self::Lt
  }
}

impl CmpKind {
  fn flip(self) -> Self {
    match self {
      Self::Lt => Self::Gt,
      Self::Le => Self::Ge,
      Self::Gt => Self::Lt,
      Self::Ge => Self::Le,
    }
  }
}

#[derive(Default)]
pub struct Unrolling<'a> {
  cx: PassContext<'a>,
}

struct Unroller<'cx, 'a> {
  cx: &'cx mut PassContext<'a>,
  trip_count: TripCount,
  scev: &'cx mut SCEV<'a>,
  loop_id: LoopId,
  value_map: FxHashMap<Operand, Operand>,
  bb_map: FxHashMap<Operand, Operand>,
  /// Value that comes from the latch in IV phi.
  iv: Operand,
  /// Continue ops
  continues: Vec<Operand>,
  old_phis: Vec<(Operand, Vec<PhiIncoming>)>,
  header_latch_values: Vec<(Operand, Operand)>,
  /// PhiId -> Incoming BBId -> Old Values
  exit_phi_old_values: FxHashMap<Operand, FxHashMap<Operand, Operand>>,
}

impl<'cx, 'a> Unroller<'cx, 'a> {
  pub fn new(cx: &'cx mut PassContext<'a>, scev: &'cx mut SCEV<'a>, loop_id: LoopId) -> Self {
    Self {
      cx,
      trip_count: TripCount::default(),
      scev,
      iv: Operand::default(),
      loop_id,
      value_map: FxHashMap::default(),
      bb_map: FxHashMap::default(),
      continues: vec![],
      old_phis: vec![],
      header_latch_values: vec![],
      exit_phi_old_values: FxHashMap::default(),
    }
  }

  fn get(&self, operand: Operand) -> Operand {
    match operand {
      Operand::Value(_) => {
        if let Some(&mapped_operand) = self.value_map.get(&operand) {
          mapped_operand
        } else {
          operand
        }
      }
      Operand::BB(_) => {
        if let Some(&mapped_operand) = self.bb_map.get(&operand) {
          mapped_operand
        } else {
          operand
        }
      }
      _ => operand,
    }
  }

  fn clone_inst(&mut self, inst_id: Operand) {
    let loop_data = self.scev.loops[self.loop_id].clone();
    let op = self.cx.get_op(inst_id);
    let (typ, attrs, mut op_data) = (op.typ.clone(), op.attrs.clone(), op.data.clone());

    let remap = |operand: &mut Operand| *operand = self.get(*operand);

    let mut bb_list = vec![];

    if let OpData::Phi { incomings } = op_data.clone() {
      // Fill Phi incomings later.
      op_data = OpData::Phi { incomings: vec![] };
      self.old_phis.push((inst_id, incomings));
    } else {
      // Replace the operand
      match_src! {
        target: &mut op_data,
        bin_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe],
        bin_arm: OpData { lhs, rhs } => {
          remap(lhs);
          remap(rhs);
        },
        un_ops: [Sitofp, Fptosi, Zext, Uitofp],
        un_arm: OpData { value } => {
          remap(value);
        },
        fallback: {
          // In DCE, Load is pure.
          OpData::Load { addr } => {
            remap(addr);
          }
          OpData::GEP { base, indices } => {
            remap(base);
            for index in indices.iter_mut() {
              remap(index);
            }
          }

          OpData::Call { func, args } => {
            remap(func);
            for arg in args.iter_mut() {
              remap(arg);
            }
          }

          OpData::Store { addr, value } => {
            remap(addr);
            remap(value);
          }

          OpData::Br { cond, then_bb, else_bb } => {
            remap(cond);
            remap(then_bb);
            remap(else_bb);
          }

          OpData::Jump { target_bb } => {
            remap(target_bb);
            bb_list.push(*target_bb);
          }

          OpData::Ret { value } => {
            if let Some(value) = value.as_mut() {
              remap(value);
            }
          }

          OpData::Phi {..} => unreachable!(),

          OpData::Alloca(_) => {/*do nothing*/}
          | OpData::GlobalAlloca(_)
          | OpData::Declare { .. } => {
              unreachable!();
          }
        }
      }
    }

    #[cfg(feature = "debug")]
    yachiyo::debug::info!("Cloning inst {:?}", op_data);

    let new_inst_id = self.cx.create(Op::new(typ, attrs, op_data));
    *self.value_map.entry(inst_id).or_default() = new_inst_id;

    // Handle BB
    let old_bb = self.cx.op_bb(inst_id);

    let mut handle_bb = |bb_id: Operand, new_inst_id: Operand, old_bb: Operand| {
      if let Operand::BB(_) = bb_id {
        if bb_id == loop_data.header {
          self.continues.push(new_inst_id);
        } else if loop_data.exit_blocks.contains(bb_id.get_bb_id()) {
          let exit_phis = self.cx.get_all_ops_in_block(bb_id, OpType::Phi);
          for phi_id in exit_phis {
            if let Some(incoming_bb_to_op) = self.exit_phi_old_values.get(&phi_id) {
              if let Some((&incoming_bb, &old_value)) = incoming_bb_to_op
                .iter()
                .find(|(bb_id, _)| **bb_id == old_bb)
              {
                let (mapped_value, mapped_bb) = (self.get(old_value), self.get(incoming_bb));
                self.cx.append_phi_incoming(phi_id, mapped_bb, mapped_value);
              }
            }
          }
        }
      }
    };

    for bb_id in bb_list {
      handle_bb(bb_id, new_inst_id, old_bb);
    }
  }

  fn as_i64_const(&self, op: Operand) -> Option<i64> {
    if let Operand::Int(value) = op {
      Some(value as i64)
    } else {
      None
    }
  }

  fn try_get_addrec(&mut self, op: Operand) -> Option<(Operand, i64, i64)> {
    let scev_id = self.scev.get_op_scev(op);
    let AddRecInfo { start, step, iv } = self.scev.get_add_rec_for_loop(scev_id, self.loop_id)?;
    let start = self.scev.get_const(start)?;
    let step = self.scev.get_const(step)?;

    Some((iv, start, step))
  }

  fn normalize_icmp(&mut self, cond: Operand) -> Option<(Operand, i64, i64, CmpKind, i64)> {
    let cond_data = self.cx.get_op_data(cond).clone();
    let (raw_cmp, lhs, rhs) = match cond_data {
      OpData::SLt { lhs, rhs } => (CmpKind::Lt, lhs, rhs),
      OpData::SLe { lhs, rhs } => (CmpKind::Le, lhs, rhs),
      OpData::SGt { lhs, rhs } => (CmpKind::Gt, lhs, rhs),
      OpData::SGe { lhs, rhs } => (CmpKind::Ge, lhs, rhs),
      _ => return None,
    };

    if let Some((iv, start, step)) = self.try_get_addrec(lhs) {
      // For now only support header-condition loops where the condition compares
      // the header phi itself, not the latch-updated next value.
      if lhs == iv {
        let bound = self.as_i64_const(rhs)?;
        return Some((iv, start, step, raw_cmp, bound));
      }
    }

    if let Some((iv, start, step)) = self.try_get_addrec(rhs) {
      if rhs == iv {
        let bound = self.as_i64_const(lhs)?;
        return Some((iv, start, step, raw_cmp.flip(), bound));
      }
    }

    None
  }

  /// # Return
  /// - `true` if unrollable
  pub fn init(&mut self) -> bool {
    let loop_data = self.scev.loops[self.loop_id].clone();
    let dom_tree = self.scev.dom_tree.clone();
    // Read the header of loop
    let header_id = loop_data.header;
    let pre_header_id = self.cx.get_pre_header_id(header_id, &dom_tree).unwrap();
    let latch_id = self.cx.get_latch_id(header_id, &dom_tree).unwrap();

    let header_phis = self.cx.get_all_ops_in_block(header_id, OpType::Phi);
    for phi_id in header_phis {
      let phi_data = self.cx.get_op_data(phi_id);
      let OpData::Phi { incomings } = phi_data else {
        unreachable!()
      };

      // Map phi value with incoming of pre-header
      for incoming in incomings {
        let PhiIncoming::Data { value, bb } = incoming else {
          unreachable!()
        };
        if *bb == pre_header_id {
          self.value_map.insert(phi_id, *value);
        } else if *bb == latch_id {
          self.header_latch_values.push((phi_id, *value));
        }
      }
    }

    // Read the bound
    let header_term_id = self.cx.get_term(header_id);
    let header_term = self.cx.get_op_data(header_term_id);
    let OpData::Br { cond, .. } = header_term else {
      return false;
    };

    let Some((iv, start, step, cmp, bound)) = self.normalize_icmp(*cond) else {
      return false;
    };

    self.iv = iv;
    self.trip_count = TripCount {
      start,
      step,
      bound,
      cmp,
    };
    let Some(count) = self.trip_count.count() else {
      return false;
    };
    if count == 0 || count > MAX_UNROLL_COUNT {
      return false;
    }

    // Reading exit blocks, record the old value in phis.
    for exit_bb_id in loop_data.exit_blocks.iter() {
      let exit_bb_id = Operand::BB(exit_bb_id);
      let exit_bb_phis = self.cx.get_all_ops_in_block(exit_bb_id, OpType::Phi);

      for phi_id in exit_bb_phis {
        let phi_data = self.cx.get_op_data(phi_id).clone();
        let OpData::Phi { incomings } = phi_data else {
          unreachable!()
        };

        let incoming_bb_to_op = self.exit_phi_old_values.entry(phi_id).or_default();
        for incoming in incomings {
          if let PhiIncoming::Data { value, bb } = incoming {
            if !loop_data.blocks.contains(bb.get_bb_id()) {
              continue;
            }
            incoming_bb_to_op.insert(bb, value);
            // Slay the old incoming
            self.cx.slay_phi_incoming(phi_id, bb);
          }
        }
      }
    }

    // Initialize continues to pre_header's terminator.
    let pre_header_term_id = self.cx.get_term(pre_header_id);
    self.continues.push(pre_header_term_id);

    true
  }

  pub fn run(mut self) {
    let loop_data = self.scev.loops[self.loop_id].clone();

    let Some(range) = self.trip_count.get_range() else {
      return;
    };
    if range.is_empty() || range.end > MAX_UNROLL_COUNT {
      return;
    }

    let dpo = self.cx.get_current_func().cfg.dpo();

    let sort_blocks = |blocks: &BitSet| {
      let mut res = vec![];
      for bb_id in dpo.iter().rev() {
        if blocks.contains(bb_id.get_bb_id()) {
          // Ignore header
          if *bb_id == loop_data.header {
            continue;
          }
          res.push(*bb_id);
        }
      }
      res
    };

    for _ in range {
      // Create and map the blocks, sort the blocks in RPO order.
      for bb_id in loop_data.blocks.iter() {
        // Ignore header
        if bb_id == loop_data.header.get_bb_id() {
          continue;
        }

        let bb_id = Operand::BB(bb_id);
        let new_bb_id = self.cx.create_new_block();
        self.bb_map.insert(bb_id, new_bb_id);
      }

      // Create and map instructions
      let sorted_blocks = sort_blocks(&loop_data.blocks);
      for (idx, bb_id) in sorted_blocks.iter().enumerate() {
        let mapped_bb = self.get(*bb_id);

        // Redirect continues to the first block
        if idx == 0 {
          for continue_op in std::mem::take(&mut self.continues) {
            self
              .cx
              .redirect_bb(continue_op, loop_data.header, mapped_bb);
          }
        }

        self.cx.set_current_block(mapped_bb);
        let cur = self.cx.get_bb(*bb_id).cur.clone();
        for inst_id in cur {
          self.clone_inst(inst_id);
        }

        // Handle phi
        for (phi_id, incomings) in std::mem::take(&mut self.old_phis) {
          for incoming in incomings {
            let PhiIncoming::Data { value, bb } = incoming else {
              unreachable!()
            };
            let (mapped_value, mapped_bb) = (self.get(value), self.get(bb));

            let mapped_phi_id = self.get(phi_id);
            self
              .cx
              .append_phi_incoming(mapped_phi_id, mapped_bb, mapped_value);
          }
        }
      }

      for (phi_id, latch_value) in self.header_latch_values.clone() {
        let mapped_value = self.get(latch_value);
        self.value_map.insert(phi_id, mapped_value);
      }
    }

    // Redirect continues in the last turn to exit_bb.
    let header_id = loop_data.header;
    let (header_exit, _) = *self
      .cx
      .get_bb(header_id)
      .succs
      .iter()
      .find(|(bb_id, _)| loop_data.exit_blocks.contains(bb_id.get_bb_id()))
      .unwrap();

    for continue_op in std::mem::take(&mut self.continues) {
      self
        .cx
        .redirect_bb(continue_op, loop_data.header, header_exit);
    }

    let replacements = self
      .value_map
      .iter()
      .filter_map(|(&old_value, &new_value)| {
        if old_value == new_value || !matches!(old_value, Operand::Value(_)) {
          return None;
        }
        let old_bb = self.cx.op_bb(old_value);
        if loop_data.blocks.contains(old_bb.get_bb_id()) {
          Some((old_value, new_value))
        } else {
          None
        }
      })
      .collect::<Vec<_>>();

    for (old_value, new_value) in replacements {
      let users = self.cx.users(old_value).to_vec();
      for (user, idx) in users {
        let user_bb = self.cx.op_bb(user);
        if !loop_data.blocks.contains(user_bb.get_bb_id()) {
          self.cx.replace_use((user, idx), old_value, new_value);
        }
      }
    }
  }
}

#[derive(Clone, Copy, Debug, Default)]
/// Now supports constant only
struct TripCount {
  start: i64,
  step: i64,
  bound: i64,
  cmp: CmpKind,
}

impl TripCount {
  fn valid_direction(&self) -> bool {
    matches!(
      (self.step.signum(), self.cmp),
      (1, CmpKind::Lt | CmpKind::Le) | (-1, CmpKind::Gt | CmpKind::Ge)
    )
  }

  fn count(&self) -> Option<i64> {
    if self.step == 0 || !self.valid_direction() {
      return None;
    }

    match (self.step > 0, self.cmp) {
      (true, CmpKind::Lt) => {
        if self.start >= self.bound {
          Some(0)
        } else {
          Some(ceil_div_pos(self.bound - self.start, self.step))
        }
      }
      (true, CmpKind::Le) => {
        if self.start > self.bound {
          Some(0)
        } else {
          Some((self.bound - self.start) / self.step + 1)
        }
      }
      (false, CmpKind::Gt) => {
        let step = -self.step;
        if self.start <= self.bound {
          Some(0)
        } else {
          Some(ceil_div_pos(self.start - self.bound, step))
        }
      }
      (false, CmpKind::Ge) => {
        let step = -self.step;
        if self.start < self.bound {
          Some(0)
        } else {
          Some((self.start - self.bound) / step + 1)
        }
      }
      _ => None,
    }
  }

  pub fn get_range(&self) -> Option<Range<i64>> {
    Some(0..self.count()?)
  }
}

fn ceil_div_pos(a: i64, b: i64) -> i64 {
  debug_assert!(a >= 0);
  debug_assert!(b > 0);
  (a + b - 1) / b
}

impl<'a> Unrolling<'a> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn run(&mut self, mut scev: SCEV<'a>) {
    for lp_id in (0..scev.loops.len()).rev() {
      let lp_id: LoopId = lp_id.into();
      if scev
        .loops
        .iter()
        .any(|loop_data| loop_data.parent == Some(lp_id))
      {
        continue;
      }
      let mut unroller = Unroller::new(&mut self.cx, &mut scev, lp_id);
      if unroller.init() {
        unroller.run();
      }
    }
  }
}

impl<'a> Pass<'a> for Unrolling<'a> {
  fn name(&self) -> &str {
    "Unrolling"
  }
  fn mount(&mut self, program: &'a mut IR) {
    self.cx.mount(program);
  }
  fn run(&mut self) {
    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      let (loops, block_to_loop) = analyze::<LoopAnalysis>(self.cx.get_current_func());
      let (dom_tree, _) = analyze::<DomAnalysis>(self.cx.get_current_func());
      let scev = <SCEV as Analysis>::new((&mut self.cx, loops, block_to_loop, dom_tree));
      self.run(scev);
    }
  }
}
