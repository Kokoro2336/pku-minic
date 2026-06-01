//! Loop Unrolling.

use super::{CanonicalExpr, TripCount};
use crate::analysis::{DomAnalysis, LoopAnalysis, Reachability, SCEV};

use yachiyo::analysis::{analyze, Analysis, LoopData, LoopId, SCEVExpr};
use yachiyo::base::Type;
use yachiyo::ir::mid::{Op, OpData, OpType, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::{match_src, Arena, BitSet};

use rustc_hash::FxHashMap;

const MAX_UNROLL_COUNT: i64 = 64;
const MAX_NESTED_UNROLL_OPS: usize = 256;
const LANE_UNROLL_COUNT: usize = 4;

#[derive(Default)]
pub struct Unrolling<'a> {
  cx: PassContext<'a>,
}

struct Unroller<'cx, 'a> {
  cx: &'cx mut PassContext<'a>,
  unroll_count: usize,
  mode: UnrollMode,
  scev: &'cx mut SCEV<'a>,
  loop_id: LoopId,
  value_map: FxHashMap<Operand, Operand>,
  bb_map: FxHashMap<Operand, Operand>,
  pre_header: Option<Operand>,
  first_unrolled_entry: Option<Operand>,
  unroll_header: Option<Operand>,
  lane_guard: Option<LaneGuard>,
  lane_phis: FxHashMap<Operand, Operand>,
  /// Continue ops
  continues: Vec<(Operand, Operand)>,
  old_phis: Vec<(Operand, Vec<PhiIncoming>)>,
  header_latch_values: Vec<(Operand, Operand)>,
  /// PhiId -> Incoming BBId -> Old Values
  exit_phi_old_values: FxHashMap<Operand, FxHashMap<Operand, Operand>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnrollMode {
  Full,
  Lane,
}

#[derive(Clone, Copy)]
struct LaneGuard {
  iv: Operand,
  bound: Operand,
  step: i64,
  inclusive: bool,
}

impl<'cx, 'a> Unroller<'cx, 'a> {
  fn new(cx: &'cx mut PassContext<'a>, scev: &'cx mut SCEV<'a>, loop_id: LoopId) -> Self {
    Self {
      cx,
      unroll_count: 0,
      mode: UnrollMode::Full,
      scev,
      loop_id,
      value_map: FxHashMap::default(),
      bb_map: FxHashMap::default(),
      pre_header: None,
      first_unrolled_entry: None,
      unroll_header: None,
      lane_guard: None,
      lane_phis: FxHashMap::default(),
      continues: vec![],
      old_phis: vec![],
      header_latch_values: vec![],
      exit_phi_old_values: FxHashMap::default(),
    }
  }

  fn try_init_lane(&mut self, header_id: Operand, loop_data: &LoopData) -> bool {
    let body_succs = self
      .cx
      .get_bb(header_id)
      .succs
      .iter()
      .filter_map(|(bb_id, _)| {
        if loop_data.blocks.contains(bb_id.get_bb_id()) && *bb_id != header_id {
          Some(*bb_id)
        } else {
          None
        }
      })
      .collect::<Vec<_>>();
    if body_succs.len() != 1 {
      return false;
    }

    let body_entry = body_succs[0];
    let Some(lane_guard) = self.try_build_lane_guard(header_id, body_entry, loop_data) else {
      return false;
    };

    self.mode = UnrollMode::Lane;
    self.unroll_count = LANE_UNROLL_COUNT;
    self.lane_guard = Some(lane_guard);

    true
  }

  fn try_build_lane_guard(
    &mut self,
    header_id: Operand,
    body_entry: Operand,
    loop_data: &LoopData,
  ) -> Option<LaneGuard> {
    let header_term = self.cx.get_term(header_id);
    let OpData::Br {
      cond,
      then_bb,
      else_bb,
    } = self.cx.get_op_data(header_term).clone()
    else {
      return None;
    };

    if then_bb != body_entry || !loop_data.exit_blocks.contains(else_bb.get_bb_id()) {
      return None;
    }

    let cmp = CanonicalExpr::from(self.cx.get_op_data(cond));
    let (lhs, rhs, inclusive) = match cmp {
      CanonicalExpr::Lt(lhs, rhs) => (lhs, rhs, false),
      CanonicalExpr::Le(lhs, rhs) => (lhs, rhs, true),
      _ => return None,
    };

    let lhs_scev = self.scev.get_op_scev(lhs);
    let rhs_scev = self.scev.get_op_scev(rhs);
    let add_rec = self.scev.get_add_rec_for_loop(lhs_scev, self.loop_id)?;

    if add_rec.iv != lhs || !self.scev.is_scev_loop_invariant(rhs_scev, self.loop_id) {
      return None;
    }

    let step = self.scev[add_rec.step].as_const()?;
    if step <= 0 {
      return None;
    }

    let _ = i32::try_from(step.saturating_mul((LANE_UNROLL_COUNT - 1) as i64)).ok()?;
    let bound = match self.scev[rhs_scev].clone() {
      SCEVExpr::Const(bound) => Operand::Int(i32::try_from(bound).ok()?),
      SCEVExpr::Unknown(bound) => bound,
      _ => return None,
    };

    Some(LaneGuard {
      iv: lhs,
      bound,
      step,
      inclusive,
    })
  }

  fn mapped_operand(&self, operand: Operand) -> Operand {
    match operand {
      Operand::Value(_) => self.value_map.get(&operand).copied().unwrap_or(operand),
      Operand::BB(_) => self.bb_map.get(&operand).copied().unwrap_or(operand),
      _ => operand,
    }
  }

  fn clone_inst(&mut self, inst_id: Operand) {
    let loop_data = self.scev.loops[self.loop_id].clone();
    let op = self.cx.get_op(inst_id);
    let (typ, attrs, mut op_data) = (op.typ.clone(), op.attrs.clone(), op.data.clone());

    let remap = |operand: &mut Operand| *operand = self.mapped_operand(*operand);

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
            bb_list.push(*then_bb);
            bb_list.push(*else_bb);
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
          self.continues.push((new_inst_id, old_bb));
        } else if loop_data.exit_blocks.contains(bb_id.get_bb_id()) {
          let exit_phis = self.cx.get_all_ops_in_block(bb_id, OpType::Phi);
          for phi_id in exit_phis {
            if let Some(incoming_bb_to_op) = self.exit_phi_old_values.get(&phi_id) {
              if let Some((&incoming_bb, &old_value)) = incoming_bb_to_op
                .iter()
                .find(|(bb_id, _)| **bb_id == old_bb)
              {
                let (mapped_value, mapped_bb) = (
                  self.mapped_operand(old_value),
                  self.mapped_operand(incoming_bb),
                );
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

  /// # Return
  /// - `true` if unrollable
  fn init(&mut self) -> bool {
    let loop_data = self.scev.loops[self.loop_id].clone();
    let dom_tree = self.scev.dom_tree.clone();
    // Read the header of loop
    let header_id = loop_data.header;
    let (Some(pre_header_id), Some(latch_id)) = (
      self.cx.get_pre_header_id(header_id, &dom_tree),
      self.cx.get_latch_id(header_id, &dom_tree),
    ) else {
      return false;
    };
    if self
      .cx
      .get_bb(header_id)
      .preds
      .iter()
      .filter(|(bb_id, _)| loop_data.blocks.contains(bb_id.get_bb_id()))
      .count()
      != 1
    {
      return false;
    }

    if !self
      .cx
      .get_bb(header_id)
      .succs
      .iter()
      .any(|(bb_id, _)| loop_data.exit_blocks.contains(bb_id.get_bb_id()))
    {
      return false;
    }
    for exit_bb_id in loop_data.exit_blocks.iter() {
      for (pred_id, _) in self.cx.get_bb(Operand::BB(exit_bb_id)).preds.iter() {
        if loop_data.blocks.contains(pred_id.get_bb_id()) && *pred_id != header_id {
          return false;
        }
      }
    }
    self.pre_header = Some(pre_header_id);

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
          if *value == Operand::Undefined {
            return false;
          }
          self.value_map.insert(phi_id, *value);
        } else if *bb == latch_id {
          self.header_latch_values.push((phi_id, *value));
        }
      }
    }

    if let Some(trip_count) = TripCount::try_build(self.cx, self.scev, self.loop_id) {
      let count = trip_count.get_trip_count();
      if !(1..=MAX_UNROLL_COUNT).contains(&count) {
        if !self.try_init_lane(header_id, &loop_data) {
          return false;
        }
      } else {
        self.mode = UnrollMode::Full;
        self.unroll_count = count as usize;
      }
    } else if !self.try_init_lane(header_id, &loop_data) {
      return false;
    }

    if loop_data.parent.is_some() {
      let cloned_ops = loop_data
        .blocks
        .iter()
        .filter(|bb_id| *bb_id != header_id.get_bb_id())
        .map(|bb_id| self.cx.get_bb(Operand::BB(bb_id)).cur.len())
        .sum::<usize>();
      if cloned_ops.saturating_mul(self.unroll_count) > MAX_NESTED_UNROLL_OPS {
        return false;
      }
    }

    for bb_id in loop_data.blocks.iter() {
      let bb_id = Operand::BB(bb_id);
      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        for &(user, _) in self.cx.users(inst_id) {
          let user_bb = self.cx.op_bb(user);
          if loop_data.blocks.contains(user_bb.get_bb_id()) || bb_id == header_id {
            continue;
          }
          if loop_data.exit_blocks.contains(user_bb.get_bb_id())
            && matches!(self.cx.get_op_data(user), OpData::Phi { .. })
          {
            continue;
          }
          return false;
        }
      }
    }

    if self.mode == UnrollMode::Full {
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
    }

    if self.mode == UnrollMode::Full {
      // Initialize continues to pre_header's terminator.
      let pre_header_term_id = self.cx.get_term(pre_header_id);
      self.continues.push((pre_header_term_id, pre_header_id));
    }

    true
  }

  fn run(mut self) {
    let loop_data = self.scev.loops[self.loop_id].clone();

    if self.unroll_count == 0 {
      return;
    }

    let dpo = self.cx.get_cfg().dpo();

    if self.mode == UnrollMode::Lane {
      self.prepare_lane_unroll(&loop_data);
    }

    let body_blocks = loop_data
      .blocks
      .iter()
      .filter(|bb_id| *bb_id != loop_data.header.get_bb_id())
      .collect::<Vec<_>>();

    let sort_blocks = |blocks: &[usize]| {
      let mut res = vec![];
      for bb_id in dpo.iter().rev() {
        if blocks.contains(&bb_id.get_bb_id()) {
          res.push(*bb_id);
        }
      }
      res
    };

    for _ in 0..self.unroll_count {
      // Create and map the blocks, sort the blocks in RPO order.
      for bb_id in body_blocks.iter().copied() {
        let bb_id = Operand::BB(bb_id);
        let new_bb_id = self.cx.create_new_block();
        self.bb_map.insert(bb_id, new_bb_id);
      }

      // Create and map instructions
      let sorted_blocks = sort_blocks(&body_blocks);
      for (idx, bb_id) in sorted_blocks.iter().enumerate() {
        let mapped_bb = self.mapped_operand(*bb_id);

        // Redirect continues to the first block
        if idx == 0 {
          if self.first_unrolled_entry.is_none() {
            self.first_unrolled_entry = Some(mapped_bb);
          }
          for (continue_op, _) in std::mem::take(&mut self.continues) {
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
      }

      // Handle phi
      for (phi_id, incomings) in std::mem::take(&mut self.old_phis) {
        for incoming in incomings {
          let PhiIncoming::Data { value, bb } = incoming else {
            unreachable!()
          };
          let (mapped_value, mapped_bb) = (self.mapped_operand(value), self.mapped_operand(bb));

          let mapped_phi_id = self.mapped_operand(phi_id);
          self
            .cx
            .append_phi_incoming(mapped_phi_id, mapped_bb, mapped_value);
        }
      }

      for (phi_id, latch_value) in self.header_latch_values.clone() {
        let mapped_value = self.mapped_operand(latch_value);
        self.value_map.insert(phi_id, mapped_value);
      }
    }

    if self.mode == UnrollMode::Lane {
      self.finish_lane_unroll(&loop_data);
      return;
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

    for (continue_op, old_bb) in std::mem::take(&mut self.continues) {
      let exit_phis = self.cx.get_all_ops_in_block(header_exit, OpType::Phi);
      for phi_id in exit_phis {
        if let Some(incoming_bb_to_op) = self.exit_phi_old_values.get(&phi_id) {
          if let Some(&old_value) = incoming_bb_to_op.get(&loop_data.header) {
            let (mapped_value, mapped_bb) =
              (self.mapped_operand(old_value), self.mapped_operand(old_bb));
            self.cx.append_phi_incoming(phi_id, mapped_bb, mapped_value);
          }
        }
      }
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

  fn prepare_lane_unroll(&mut self, loop_data: &LoopData) {
    let header_id = loop_data.header;
    let pre_header = self
      .pre_header
      .expect("Lane unrolling should have a pre-header");
    let pre_header_term = self.cx.get_term(pre_header);
    let unroll_header = self.cx.create_new_block();
    let header_phis = self.cx.get_all_ops_in_block(header_id, OpType::Phi);

    self.unroll_header = Some(unroll_header);
    self
      .cx
      .redirect_bb(pre_header_term, header_id, unroll_header);
    self.cx.set_current_block(unroll_header);

    for phi_id in header_phis {
      let start_value = self.value_map.get(&phi_id).copied().unwrap_or(phi_id);
      let phi_type = self.cx.get_op_type(phi_id);
      let lane_phi = self.cx.create(Op::new(
        phi_type,
        vec![],
        OpData::Phi {
          incomings: vec![PhiIncoming::Data {
            value: start_value,
            bb: pre_header,
          }],
        },
      ));
      self.lane_phis.insert(phi_id, lane_phi);
      self.value_map.insert(phi_id, lane_phi);
    }
  }

  fn finish_lane_unroll(&mut self, loop_data: &LoopData) {
    let header_id = loop_data.header;
    let pre_header = self
      .pre_header
      .expect("Lane unrolling should have a pre-header");
    let unroll_header = self
      .unroll_header
      .expect("Lane unrolling should have an unroll header");
    let first_unrolled_entry = self
      .first_unrolled_entry
      .expect("Lane unrolling should have a cloned entry");
    let final_continues = std::mem::take(&mut self.continues);
    let final_continue_bbs = final_continues
      .iter()
      .map(|(continue_op, _)| self.cx.op_bb(*continue_op))
      .collect::<Vec<_>>();

    for (continue_op, _) in final_continues {
      self.cx.redirect_bb(continue_op, header_id, unroll_header);
    }

    for (&phi_id, &lane_phi) in self.lane_phis.clone().iter() {
      self.cx.slay_phi_incoming(phi_id, pre_header);
      self.cx.append_phi_incoming(phi_id, unroll_header, lane_phi);

      let mapped_value = self.value_map.get(&phi_id).copied().unwrap_or(lane_phi);

      for incoming_bb in final_continue_bbs.iter().copied() {
        self
          .cx
          .append_phi_incoming(lane_phi, incoming_bb, mapped_value);
      }
    }

    self.cx.set_current_block(unroll_header);
    let cond = self.materialize_lane_guard();
    self.cx.create(Op::new(
      Type::Void,
      vec![],
      OpData::Br {
        cond,
        then_bb: first_unrolled_entry,
        else_bb: header_id,
      },
    ));
  }

  fn materialize_lane_guard(&mut self) -> Operand {
    let lane_guard = self.lane_guard.expect("Lane unrolling should have a guard");
    let iv = self
      .lane_phis
      .get(&lane_guard.iv)
      .copied()
      .unwrap_or(lane_guard.iv);
    let stride = (LANE_UNROLL_COUNT - 1) as i64 * lane_guard.step;
    let guard_lhs = if stride == 0 {
      iv
    } else {
      self.cx.create(Op::new(
        Type::Int,
        vec![],
        OpData::AddI {
          lhs: iv,
          rhs: Operand::Int(i32::try_from(stride).unwrap()),
        },
      ))
    };
    let guard_data = if lane_guard.inclusive {
      OpData::SLe {
        lhs: guard_lhs,
        rhs: lane_guard.bound,
      }
    } else {
      OpData::SLt {
        lhs: guard_lhs,
        rhs: lane_guard.bound,
      }
    };

    self.cx.create(Op::new(Type::Bool, vec![], guard_data))
  }
}

impl<'a> Unrolling<'a> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn remove_unreachable_blocks(&mut self) {
    let func_id = self.cx.get_current_func_id();
    let visited = analyze::<Reachability>(self.cx.get_func(func_id));
    let dead_blocks = self
      .cx
      .get_func(func_id)
      .cfg
      .collect()
      .into_iter()
      .filter(|bb_id| !visited.contains(*bb_id))
      .map(Operand::BB)
      .collect::<Vec<_>>();

    if dead_blocks.is_empty() {
      return;
    }

    let mut dead_set = BitSet::new();
    for bb_id in dead_blocks.iter() {
      dead_set.insert(bb_id.get_bb_id());
    }

    for &bb_id in &dead_blocks {
      let succs = self.cx.get_bb(bb_id).succs.clone();
      for (succ_id, _) in succs {
        if dead_set.contains(succ_id.get_bb_id()) {
          continue;
        }

        let succ_phis = self.cx.get_all_ops_in_block(succ_id, OpType::Phi);
        for phi_id in succ_phis {
          loop {
            let OpData::Phi { incomings } = self.cx.get_op_data(phi_id).clone() else {
              unreachable!()
            };
            if !incomings
              .iter()
              .any(|incoming| matches!(incoming, PhiIncoming::Data { bb, .. } if *bb == bb_id))
            {
              break;
            }
            self.cx.slay_phi_incoming(phi_id, bb_id);
          }
        }
      }
    }

    for &bb_id in &dead_blocks {
      if let Some(&term_id) = self.cx.get_bb(bb_id).cur.last() {
        if self.cx.get_op_data(term_id).is_terminator() {
          self.cx.remove_control_flow(term_id);
        }
      }
    }

    self.cx.clear_uses();

    for &bb_id in &dead_blocks {
      let cur = self.cx.get_bb(bb_id).cur.clone();
      let func = self.cx.get_func_mut(func_id);
      for inst_id in cur.iter().rev() {
        func.op_to_bb[*inst_id] = Operand::default();
        func.dfg.remove(inst_id.get_op_id());
      }
    }

    for &bb_id in &dead_blocks {
      self.cx.get_func_mut(func_id).cfg.remove(bb_id.get_bb_id());
    }

    let blocks = self.cx.get_cfg().collect();
    for bb_id in blocks {
      let cur = self.cx.get_bb(Operand::BB(bb_id)).cur.clone();
      for inst_id in cur {
        self.cx.add_uses(inst_id);
      }
    }
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

    self.remove_unreachable_blocks();
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
      let graph = self.cx.extract_cfg();
      let (dom_tree, _) = analyze::<DomAnalysis>(&graph);
      let (loops, block_to_loop) = analyze::<LoopAnalysis>(&graph);
      let scev = <SCEV as Analysis>::new((&mut self.cx, loops, block_to_loop, dom_tree));
      self.run(scev);
    }
  }
}
