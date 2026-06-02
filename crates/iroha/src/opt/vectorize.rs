//! Vectorization via SLP.

use yachiyo::analysis::{analyze, AliasResult, Analysis, CallGraph, MemLoc, LoopId};
use yachiyo::ir::mid::{Attr, OpData, Operand, PhiIncoming, IR};
use yachiyo::pass::{Pass, PassContext};
use yachiyo::utils::IndexedArena;

use crate::analysis::{alias, CallGraphAnalysis, DomAnalysis, LoopAnalysis, SCEV};

use kaguya::kaguya_hime;

use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackId(usize);

#[derive(Default)]
pub struct Vectorize<'a> {
  cx: PassContext<'a>,
  call_graph: CallGraph,
  groups: Vec<[Operand; 4]>,
  pack_keys: FxHashMap<[Operand; 4], PackId>,
  packs: IndexedArena<Pack>,
  candidate: Vec<PackId>,
}

/// SLP Tree Packing.
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum Pack {
  Store { addr: Operand, value: PackId },
  Load { addr: Operand },
  Add { lhs: PackId, rhs: PackId },
  Sub { lhs: PackId, rhs: PackId },
  Mul { lhs: PackId, rhs: PackId },
  Phi { incomings: Vec<(PackId, Operand)> },
  Build { lanes: [Operand; 4] },
}

impl Vectorize<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
    self.groups.clear();
    self
      .groups
      .resize(self.cx.get_dfg().len(), [Operand::Undefined; 4]);
  }

  fn add_to_group(&mut self, group_id: Operand, lane: usize, inst_id: Operand) {
    self.groups[group_id.get_op_id()][lane] = inst_id;
  }

  fn collect_groups(&mut self) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in self.cx.bbs(func_id) {
      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        let Some(&Attr::Lane { group_id, lane }) = self
          .cx
          .get_op(inst_id)
          .attrs
          .iter()
          .find(|attr| matches!(attr, Attr::Lane { .. }))
        else {
          continue;
        };

        self.add_to_group(group_id, lane, inst_id);
      }
    }
  }

  fn no_alias(&mut self, addrs: [Operand; 4]) -> bool {
    for i in 0..4 {
      for j in i + 1..4 {
        let a = addrs[i];
        let b = addrs[j];
        if a == Operand::Undefined || b == Operand::Undefined {
          continue;
        }
        if alias(&mut self.cx, a, b, &self.call_graph) != AliasResult::NoAlias {
          return false;
        }
      }
    }
    true
  }

  fn match_mem(&mut self, scev: &mut SCEV<'_>, addrs: [Operand; 4], size: i64) -> bool {
    let (
      MemLoc {
        base: base0,
        offset: offset0,
        ..
      },
      MemLoc {
        base: base1,
        offset: offset1,
        ..
      },
      MemLoc {
        base: base2,
        offset: offset2,
        ..
      },
      MemLoc {
        base: base3,
        offset: offset3,
        ..
      },
    ) = (
      self.cx.compute_mem_loc(addrs[0]),
      self.cx.compute_mem_loc(addrs[1]),
      self.cx.compute_mem_loc(addrs[2]),
      self.cx.compute_mem_loc(addrs[3]),
    );

    if base0 != base1 || base0 != base2 || base0 != base3 {
      return false;
    }

    let (scev_id0, scev_id1, scev_id2, scev_id3) = (
      scev.get_affine_scev(&offset0),
      scev.get_affine_scev(&offset1),
      scev.get_affine_scev(&offset2),
      scev.get_affine_scev(&offset3),
    );

    let (Some((iv0, c0)), Some((iv1, c1)), Some((iv2, c2)), Some((iv3, c3))) = (
      scev.get_iv_plus_const(scev_id0),
      scev.get_iv_plus_const(scev_id1),
      scev.get_iv_plus_const(scev_id2),
      scev.get_iv_plus_const(scev_id3),
    ) else {
      return false;
    };

    iv0 == iv1 && iv1 == iv2 && iv2 == iv3 && c1 - c0 == size && c2 - c1 == size && c3 - c2 == size
  }

  fn check_ty(&mut self, ops: [Operand; 4]) -> bool {
    let ty = self.cx.get_op_type(ops[0]);
    for &op in ops.iter().skip(1) {
      if self.cx.get_op_type(op) != ty {
        return false;
      }
    }
    true
  }

  /// Try to pack parallel instructions into vector instructions.
  fn try_pack(&mut self, scev: &mut SCEV<'_>, pack: [Operand; 4]) -> Option<PackId> {
    if let Some(id) = self.pack_keys.get(&pack) {
      return Some(*id);
    }

    let typ = self.cx.get_op_type(pack[0]);

    match (
      self.cx.get_op_data(pack[0]).clone(),
      self.cx.get_op_data(pack[1]).clone(),
      self.cx.get_op_data(pack[2]).clone(),
      self.cx.get_op_data(pack[3]).clone(),
    ) {
      (
        OpData::Load { addr: addr0, .. },
        OpData::Load { addr: addr1, .. },
        OpData::Load { addr: addr2, .. },
        OpData::Load { addr: addr3, .. },
      ) => {
        let addrs_lanes = [addr0, addr1, addr2, addr3];
        if self.match_mem(scev, addrs_lanes, typ.size() as i64) {
          self.try_pack(scev, addrs_lanes)?;
          let pack = Pack::Load { addr: addr0 };
          let pack_id = self.packs.alloc(pack);
          self.pack_keys.insert(addrs_lanes, PackId(pack_id));
          return Some(PackId(pack_id));
        }
        None
      }
      (
        OpData::Store {
          addr: addr0,
          value: value0,
        },
        OpData::Store {
          addr: addr1,
          value: value1,
        },
        OpData::Store {
          addr: addr2,
          value: value2,
        },
        OpData::Store {
          addr: addr3,
          value: value3,
        },
      ) => {
        let addrs_lanes = [addr0, addr1, addr2, addr3];
        if self.match_mem(scev, addrs_lanes, typ.size() as i64) {
          self.try_pack(scev, addrs_lanes)?;
          let value_lanes = [value0, value1, value2, value3];
          if !self.check_ty(value_lanes) {
            return None;
          }
          let value_pack_id = self.try_pack(scev, value_lanes)?;

          let pack = Pack::Store {
            addr: addr0,
            value: value_pack_id,
          };
          let pack_id = self.packs.alloc(pack);
          self.pack_keys.insert(addrs_lanes, PackId(pack_id));
          return Some(PackId(pack_id));
        }

        None
      }
      (
        OpData::AddI {
          lhs: lhs0,
          rhs: rhs0,
        },
        OpData::AddI {
          lhs: lhs1,
          rhs: rhs1,
        },
        OpData::AddI {
          lhs: lhs2,
          rhs: rhs2,
        },
        OpData::AddI {
          lhs: lhs3,
          rhs: rhs3,
        },
      )
      | (
        OpData::MulI {
          lhs: lhs0,
          rhs: rhs0,
        },
        OpData::MulI {
          lhs: lhs1,
          rhs: rhs1,
        },
        OpData::MulI {
          lhs: lhs2,
          rhs: rhs2,
        },
        OpData::MulI {
          lhs: lhs3,
          rhs: rhs3,
        },
      ) => {
        let lhs_lanes = [lhs0, lhs1, lhs2, lhs3];
        let rhs_lanes = [rhs0, rhs1, rhs2, rhs3];
        if !self.check_ty(lhs_lanes) || !self.check_ty(rhs_lanes) {
          return None;
        }

        let lhs_pack_id = self.try_pack(scev, lhs_lanes)?;
        let rhs_pack_id = self.try_pack(scev, rhs_lanes)?;

        let pack_data = if matches!(self.cx.get_op_data(pack[0]), OpData::AddI { .. }) {
          Pack::Add {
            lhs: lhs_pack_id,
            rhs: rhs_pack_id,
          }
        } else {
          Pack::Mul {
            lhs: lhs_pack_id,
            rhs: rhs_pack_id,
          }
        };
        let pack_id = self.packs.alloc(pack_data);
        self.pack_keys.insert(pack, PackId(pack_id));
        Some(PackId(pack_id))
      }

      (OpData::Phi { .. }, OpData::Phi { .. }, OpData::Phi { .. }, OpData::Phi { .. }) => {
        let (
          Some(pre_header_id0),
          Some(pre_header_id1),
          Some(pre_header_id2),
          Some(pre_header_id3),
        ) = (
          self.cx.get_pre_header_id(pack[0], &scev.dom_tree),
          self.cx.get_pre_header_id(pack[1], &scev.dom_tree),
          self.cx.get_pre_header_id(pack[2], &scev.dom_tree),
          self.cx.get_pre_header_id(pack[3], &scev.dom_tree),
        )
        else {
          return None;
        };
        if !(pre_header_id0 == pre_header_id1
          && pre_header_id1 == pre_header_id2
          && pre_header_id2 == pre_header_id3)
        {
          return None;
        }

        let (Some(latch_id0), Some(latch_id1), Some(latch_id2), Some(latch_id3)) = (
          self.cx.get_latch_id(pack[0], &scev.dom_tree),
          self.cx.get_latch_id(pack[1], &scev.dom_tree),
          self.cx.get_latch_id(pack[2], &scev.dom_tree),
          self.cx.get_latch_id(pack[3], &scev.dom_tree),
        ) else {
          return None;
        };
        if !(latch_id0 == latch_id1 && latch_id1 == latch_id2 && latch_id2 == latch_id3) {
          return None;
        }

        let (
          Some(preheader_value0),
          Some(preheader_value1),
          Some(preheader_value2),
          Some(preheader_value3),
        ) = (
          self.cx.get_pre_header_value(pack[0], &scev.dom_tree),
          self.cx.get_pre_header_value(pack[1], &scev.dom_tree),
          self.cx.get_pre_header_value(pack[2], &scev.dom_tree),
          self.cx.get_pre_header_value(pack[3], &scev.dom_tree),
        )
        else {
          return None;
        };

        let (Some(latch_value0), Some(latch_value1), Some(latch_value2), Some(latch_value3)) = (
          self.cx.get_latch_value(pack[0], &scev.dom_tree),
          self.cx.get_latch_value(pack[1], &scev.dom_tree),
          self.cx.get_latch_value(pack[2], &scev.dom_tree),
          self.cx.get_latch_value(pack[3], &scev.dom_tree),
        ) else {
          return None;
        };

        let preheader_values = [
          preheader_value0,
          preheader_value1,
          preheader_value2,
          preheader_value3,
        ];
        let latch_values = [latch_value0, latch_value1, latch_value2, latch_value3];
        if !self.check_ty(preheader_values) || !self.check_ty(latch_values) {
          return None;
        }

        let preheader_pack_id = self.try_pack(scev, preheader_values)?;
        let latch_pack_id = self.try_pack(scev, latch_values)?;

        let phi_pack = Pack::Phi {
          incomings: vec![
            (preheader_pack_id, pre_header_id0),
            (latch_pack_id, latch_id0),
          ],
        };
        let phi_pack_id = self.packs.alloc(phi_pack);
        self.pack_keys.insert(pack, PackId(phi_pack_id));
        Some(PackId(phi_pack_id))
      }

      other => unreachable!("Unexpected pack: {:?}", other),
    }
  }

  fn run(&mut self, scev: &mut SCEV<'_>) {
    for lp_id in (0..scev.loops.len()).rev() {
      let lp_id: LoopId = lp_id.into();

      for bb_id in scev.loops[lp_id].owned_blocks.clone().iter() {
        let bb_id = Operand::BB(bb_id);

        for inst_id in self.cx.get_bb(bb_id).cur.clone() {
          let Some(&Attr::Lane { group_id, .. }) = self
            .cx
            .get_op(inst_id)
            .attrs
            .iter()
            .find(|attr| matches!(attr, Attr::Lane { .. }))
          else {
            continue;
          };

          kaguya_hime!(
            self.cx,
            match inst_id {
              // sum = sum + a[i]
              AddI(Phi(_), Load(GEP(_, _))) | AddI(Load(GEP(_, [_, _] | [_])), Phi(_)) => {
                let add_lanes = self.groups[group_id.get_op_id()];
                let Some(pack_id) = self.try_pack(scev, add_lanes) else {
                  continue;
                };
                self.candidate.push(pack_id);
              },
              // sum = sum + a[i] + b[i]
              AddI(
                Phi([PhiIncoming(_, _), ..]),
                AddI(Load(GEP(_, [Int(0), _])), Load(GEP(_, [Int(0), _]))),
              ) => {
                let add_lanes = self.groups[group_id.get_op_id()];
                let Some(pack_id) = self.try_pack(scev, add_lanes) else {
                  continue;
                };
                self.candidate.push(pack_id);
              },
              // c[i] = a[i] + b[i]
              Store(
                GEP(_, [Int(0), _]),
                AddI(Load(GEP(_, [Int(0), _])), Load(GEP(_, [Int(0), _]))),
              ) => {
                let store_lanes = self.groups[group_id.get_op_id()];
                let Some(pack_id) = self.try_pack(scev, store_lanes) else {
                  continue;
                };
                self.candidate.push(pack_id);
              },
              // c[i][j] = a[i][k] + b[k][j]
              Store(
                GEP(_, [Int(0), _, _]),
                AddI(
                  Phi([PhiIncoming(_, _), ..]),
                  MulI(Load(GEP(_, [Int(0), _, _])), Load(GEP(_, [Int(0), _, _]))),
                ),
              ) => {
                let store_lanes = self.groups[group_id.get_op_id()];
                let Some(pack_id) = self.try_pack(scev, store_lanes) else {
                  continue;
                };
                self.candidate.push(pack_id);
              }
            }
          );
        }
      }
    }
  }

  fn rewrite(&mut self) {
    
  }
}

impl<'a> Pass<'a> for Vectorize<'a> {
  fn name(&self) -> &str {
    "Vectorize"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    self.call_graph = analyze::<CallGraphAnalysis>(self.cx.ir());

    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      self.collect_groups();
      let graph = self.cx.extract_cfg();
      let (dom_tree, _) = analyze::<DomAnalysis>(&graph);
      let (loops, block_to_loop) = analyze::<LoopAnalysis>(&graph);
      let mut scev = SCEV::new((&mut self.cx, loops, block_to_loop, dom_tree));
      self.run(&mut scev);
    }
  }
}
