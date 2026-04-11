//! Lower IR module defintion, with graph structure variation APIs.

use super::{
  BBuilder, BBuilderGuard, BOp, BOpData, BOperand, BssInfo, DataInfo, LOpData, MOpData, Reg,
  RoDataInfo, VirtReg, BCFG, BCG, BDFG,
};
use crate::utils::arena::ArenaItem;
use crate::utils::r#match::{match_rd, match_some};

#[derive(Debug, Clone)]
pub struct BackIR {
  pub data_info: DataInfo,
  pub rodata_info: RoDataInfo,
  pub bss_info: BssInfo,
  pub funcs: BCG,
}

impl Default for BackIR {
  fn default() -> Self {
    Self::new()
  }
}

impl BackIR {
  pub fn new() -> Self {
    Self {
      data_info: DataInfo::new(),
      rodata_info: RoDataInfo::new(),
      bss_info: BssInfo::new(),
      funcs: BCG::new(),
    }
  }

  pub(crate) fn cfg_mut_or_panic(
    &mut self,
    current_function: Option<BOperand>,
    msg: &str,
  ) -> &mut BCFG {
    let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
    &mut self.funcs[idx].cfg
  }

  fn cfg_dfg_mut_or_panic(
    &mut self,
    current_function: Option<BOperand>,
    msg: &str,
  ) -> (&mut BCFG, &mut BDFG) {
    let idx = current_function.unwrap_or_else(|| panic!("{}", msg));
    let func = &mut self.funcs[idx];
    (&mut func.cfg, &mut func.dfg)
  }

  pub fn add_uses(&mut self, current_function: Option<BOperand>, op: BOperand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<_>>();

    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    for (src, src_idx) in src_tuples {
      vregs.add_use(src, (op, src_idx));
    }
  }

  /// Remove the uses of the operation.
  pub fn remove_uses(&mut self, current_function: Option<BOperand>, op: BOperand) {
    let src_tuples = self
      .get_src_tuple(current_function, op)
      .into_iter()
      .map(|(src, idx)| (*src, idx))
      .collect::<Vec<_>>();
    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    for (src, src_idx) in src_tuples {
      vregs.remove_use(src, (op, src_idx));
    }
  }

  pub fn remove_def(&mut self, current_function: Option<BOperand>, op: BOperand) {
    let rd = match self.get_rd(current_function, op) {
      Some(rd) => *rd,
      None => return,
    };
    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    vregs.remove_def(rd, op);
  }

  /// # Arguments
  /// * `old`: old InstId(Not VirtId)
  /// * `new`: new BOperand
  pub fn replace_rd(
    &mut self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
    new_operand: BOperand,
  ) {
    let old_vreg_id = match inst_id {
      BOperand::Inst(_) => match self.get_rd(current_function, inst_id) {
        Some(rd) => match rd {
          BOperand::Reg(Reg::Virt(_)) => *rd,
          // RAUW only works for SSA form values.
          BOperand::Reg(Reg::X(_))
          | BOperand::Reg(Reg::F(_))
          | BOperand::Data(_)
          | BOperand::Bss(_)
          | BOperand::IntImm(_)
          | BOperand::FloatImm(_)
          | BOperand::Slot(_)
          | BOperand::Undef
          | BOperand::Extern(_)
          | BOperand::RoData(_)
          | BOperand::BB(_)
          | BOperand::Inst(_)
          | BOperand::Func(_) => {
            unreachable!("replace_all_uses: old operand cannot be {:?}", inst_id)
          }
        },
        None => return,
      },
      BOperand::Data(_)
      | BOperand::Reg(_)
      | BOperand::IntImm(_)
      | BOperand::FloatImm(_)
      | BOperand::Slot(_)
      | BOperand::Extern(_)
      | BOperand::Undef
      | BOperand::RoData(_)
      | BOperand::Bss(_)
      | BOperand::BB(_)
      | BOperand::Func(_) => {
        unreachable!("replace_all_uses: new operand cannot be {:?}", inst_id)
      }
    };
    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    let uses = vregs[old_vreg_id].uses.clone();

    // Replace the definition of old_vreg_id with new_operand.
    let rd = self.get_rd_mut(current_function, inst_id).unwrap();
    *rd = new_operand;
    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    vregs.remove_def(old_vreg_id, inst_id);
    vregs.add_def(new_operand, inst_id);

    // Replace the old_vreg_id with new_operand in all uses.
    for use_tuple in uses {
      let src = self.get_src_mut(current_function, use_tuple.0);
      for operand in src {
        if *operand == old_vreg_id {
          *operand = new_operand;
        }
      }

      let vregs = &mut self.funcs[current_function.unwrap()].vregs;
      vregs.remove_use(old_vreg_id, use_tuple);
      vregs.add_use(new_operand, use_tuple);
    }
  }

  /// # Arguments
  /// * `use_tuple`: (InstId, idx), where idx is the operand index in the instruction
  /// * `old_operand`: the operand to be replaced
  /// * `new_operand`: the new operand to replace with
  pub fn replace_src(
    &mut self,
    current_function: Option<BOperand>,
    use_tuple: (BOperand, usize),
    old_operand: BOperand,
    new_operand: BOperand,
  ) {
    let (inst_id, idx) = use_tuple;
    let mut changed = false;
    let mut remap_operand = |operand: &mut BOperand| {
      if *operand == old_operand {
        *operand = new_operand;
        changed = true;
      }
    };

    let src_tuples = self.get_src_tuple_mut(current_function, inst_id);
    for (src, src_idx) in src_tuples {
      if src_idx == idx {
        remap_operand(src);
      }
    }
    if changed {
      let vregs = &mut self.funcs[current_function.unwrap()].vregs;
      vregs.remove_use(old_operand, use_tuple);
      vregs.add_use(new_operand, use_tuple);
    }
  }

  /// # Arguments
  /// * `old`: old InstId(Not VirtId)
  /// * `new`: new InstId
  pub fn replace_all_uses(
    &mut self,
    current_function: Option<BOperand>,
    old: BOperand,
    new: BOperand,
  ) {
    let old_vreg_id = match old {
      BOperand::Inst(_) => match self.get_rd(current_function, old) {
        Some(rd) => match rd {
          BOperand::Reg(Reg::Virt(_)) => *rd,
          // RAUW only works for SSA form values.
          BOperand::Reg(Reg::X(_))
          | BOperand::Reg(Reg::F(_))
          | BOperand::Data(_)
          | BOperand::Bss(_)
          | BOperand::IntImm(_)
          | BOperand::FloatImm(_)
          | BOperand::Slot(_)
          | BOperand::Undef
          | BOperand::Extern(_)
          | BOperand::RoData(_)
          | BOperand::BB(_)
          | BOperand::Inst(_)
          | BOperand::Func(_) => {
            unreachable!("replace_all_uses: old operand cannot be {:?}", old)
          }
        },
        None => return,
      },
      BOperand::Data(_)
      | BOperand::Reg(_)
      | BOperand::IntImm(_)
      | BOperand::FloatImm(_)
      | BOperand::Slot(_)
      | BOperand::Extern(_)
      | BOperand::Undef
      | BOperand::RoData(_)
      | BOperand::Bss(_)
      | BOperand::BB(_)
      | BOperand::Func(_) => {
        unreachable!("replace_all_uses: new operand cannot be {:?}", old)
      }
    };
    let vregs = &mut self.funcs[current_function.unwrap()].vregs;
    let uses = vregs[old_vreg_id].uses.clone();

    let new_vreg_id = match new {
      BOperand::Inst(_) => match self.get_rd(current_function, new) {
        Some(rd) => *rd,
        None => return,
      },
      BOperand::Data(_)
      | BOperand::Reg(_)
      | BOperand::IntImm(_)
      | BOperand::FloatImm(_)
      | BOperand::Slot(_)
      | BOperand::Undef
      | BOperand::Extern(_)
      | BOperand::RoData(_)
      | BOperand::Bss(_) => new,

      BOperand::BB(_) | BOperand::Func(_) => {
        unreachable!("replace_all_uses: new operand cannot be {:?}", new)
      }
    };

    for use_tuple in uses {
      let use_op = use_tuple.0;
      let src = self.get_src_mut(current_function, use_op);
      for operand in src {
        if *operand == old_vreg_id {
          *operand = new_vreg_id;
        }
      }
      let vregs = &mut self.funcs[current_function.unwrap()].vregs;
      vregs.remove_use(old_vreg_id, use_tuple);
      vregs.add_use(new_vreg_id, use_tuple);
    }
  }

  pub fn add_control_flow(
    &mut self,
    current_function: Option<BOperand>,
    op: BOperand,
    bb: BOperand,
  ) {
    let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
      current_function,
      "BackIR add_control_flow: no current function",
    );
    let data = dfg[op.get_inst_id()].data.clone();

    match data {
      BOpData::L(data) => {
        match_some! {
            target: data,
            enu: LOpData,
            minor_arms: {
                LOpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.add_pred(then_bb, (bb, op));
                    cfg.add_succ(bb, (then_bb, op));
                    cfg.add_pred(else_bb, (bb, op));
                    cfg.add_succ(bb, (else_bb, op));
                }
                LOpData::Jump { target_bb } => {
                    cfg.add_pred(target_bb, (bb, op));
                    cfg.add_succ(bb, (target_bb, op));
                }
            },
            uni_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Store, Load, Move, Call, LoadIntImm, LoadFloatImm, Ret],
            uni_arm: {}
        }
      }
      BOpData::M(data) => {
        match_some! {
            target: data,
            enu: MOpData,
            minor_arms: {
                MOpData::J { target } => {
                    cfg.add_pred(target, (bb, op));
                    cfg.add_succ(bb, (target, op));
                }
                MOpData::Bnez { target, .. } => {
                    cfg.add_pred(target, (bb, op));
                    cfg.add_succ(bb, (target, op));
                }
                MOpData::Beq { offset, .. }
                | MOpData::Bne { offset, .. }
                | MOpData::Blt { offset, .. }
                | MOpData::Bge { offset, .. }
                | MOpData::Bltu { offset, .. }
                | MOpData::Bgeu { offset, .. } => {
                    cfg.add_pred(offset, (bb, op));
                    cfg.add_succ(bb, (offset, op));
                }
            },
            uni_ops: [Li, La, Mv, FmvS, Addw, Subw, Mulw, Divw, Remw, Addiw, Subiw, Muliw, Diviw, Remiw, Slliw, Srliw, Sraiw, Sllw, Srlw, Sraw, Slt, Slti, Sltu, Sltiu, Xor, Xori, FaddS, FsubS, FmulS, FdivS, FeqS, FltS, FleS, FneS, FgtS, FgeS, FcvtWS, FcvtSW, FmvWX, FmvXW, Lw, Sw, Flw, Fsw, Ld, Sd, Call, Ret],
            uni_arm: {}
        }
      }
    }
  }

  pub fn remove_control_flow(
    &mut self,
    current_function: Option<BOperand>,
    op: BOperand,
    bb: BOperand,
  ) {
    let (cfg, dfg) = self.cfg_dfg_mut_or_panic(
      current_function,
      "BackIR remove_control_flow: no current function",
    );
    let data = dfg[op.get_inst_id()].data.clone();

    match data {
      BOpData::L(data) => {
        match_some! {
            target: data,
            enu: LOpData,
            minor_arms: {
                LOpData::Br {
                    then_bb, else_bb, ..
                } => {
                    cfg.remove_pred(then_bb, (bb, op));
                    cfg.remove_succ(bb, (then_bb, op));
                    cfg.remove_pred(else_bb, (bb, op));
                    cfg.remove_succ(bb, (else_bb, op));
                }
                LOpData::Jump { target_bb } => {
                    cfg.remove_pred(target_bb, (bb, op));
                    cfg.remove_succ(bb, (target_bb, op));
                }
            },
            uni_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, AddF, SubF, MulF, DivF, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Store, Load, Move, Call, LoadIntImm, LoadFloatImm, Ret],
            uni_arm: {}
        }
      }
      BOpData::M(data) => {
        match_some! {
            target: data,
            enu: MOpData,
            minor_arms: {
                MOpData::J { target } => {
                    cfg.remove_pred(target, (bb, op));
                    cfg.remove_succ(bb, (target, op));
                }
                MOpData::Bnez { target, .. } => {
                    cfg.remove_pred(target, (bb, op));
                    cfg.remove_succ(bb, (target, op));
                }
                MOpData::Beq { offset, .. }
                | MOpData::Bne { offset, .. }
                | MOpData::Blt { offset, .. }
                | MOpData::Bge { offset, .. }
                | MOpData::Bltu { offset, .. }
                | MOpData::Bgeu { offset, .. } => {
                    cfg.remove_pred(offset, (bb, op));
                    cfg.remove_succ(bb, (offset, op));
                }
            },
            uni_ops: [Li, La, Mv, FmvS, Addw, Subw, Mulw, Divw, Remw, Addiw, Subiw, Muliw, Diviw, Remiw, Slliw, Srliw, Sraiw, Sllw, Srlw, Sraw, Slt, Slti, Sltu, Sltiu, Xor, Xori, FaddS, FsubS, FmulS, FdivS, FeqS, FltS, FleS, FneS, FgtS, FgeS, FcvtWS, FcvtSW, FmvWX, FmvXW, Lw, Sw, Flw, Fsw, Ld, Sd, Call, Ret],
            uni_arm: {}
        }
      }
    }
  }

  pub fn create(
    &mut self,
    builder: &BBuilder,
    current_function: Option<BOperand>,
    op: BOp,
  ) -> BOperand {
    crate::debug::info!("Creating op {:?} in function {:?}", op, current_function);

    let (cfg, dfg) =
      self.cfg_dfg_mut_or_panic(current_function, "BackIR create: no current function");

    let new_id = dfg.alloc(op);
    let current_block = if let Some(block) = &builder.current_block {
      block.get_bb_id()
    } else {
      panic!("BackIR create: current_block is None");
    };
    let bb = &mut cfg[current_block];

    let op_id = if let Some(current_inst) = &builder.current_inst {
      let pos = bb
        .cur
        .iter()
        .position(|id| id.get_inst_id() == current_inst.get_inst_id())
        .unwrap_or_else(|| {
          panic!(
            "BackIR create: current_inst {:?} not found in current_block {:?}",
            current_inst, builder.current_block
          )
        });
      let op_id = BOperand::Inst(new_id);
      bb.cur.insert(pos, op_id);
      op_id
    } else {
      let op_id = BOperand::Inst(new_id);
      bb.cur.push(op_id);
      op_id
    };

    crate::debug::info!("Created op {:?} in block {:?}", op_id, current_block);

    self.bind(current_function, op_id);
    self.add_uses(current_function, op_id);
    let current_block = builder
      .current_block
      .unwrap_or_else(|| panic!("BackIR create: current_block is None"));
    self.add_control_flow(current_function, op_id, current_block);
    op_id
  }

  /// Bind the operation with its rd.
  /// If rd is BOperand::Undef, it means we need to create a new virtual register and bind the operation with it.
  /// Else if rd is BOperand::Reg, we do nothing for it.
  /// Else panic and report invalid rd.
  pub fn bind(&mut self, current_function: Option<BOperand>, op_id: BOperand) {
    let func = &mut self.funcs[current_function.unwrap()];
    let op = &mut func.dfg[op_id];
    let (data, typ) = (&mut op.data, op.typ.clone());
    let vregs = &mut func.vregs;

    match data {
      BOpData::L(lop_data) => match_rd! {
          target: lop_data,
          op_with_rds: [AddI, SubI, MulI, DivI, ModI, AddF, SubF, MulF, DivF, SNe, SEq, SGt, SLt, SGe, SLe, Xor, Shl, Shr, Sar, ONe, OEq, OGt, OLt, OGe, OLe, Sitofp, Fptosi, Load, Move, LoadFloatImm, LoadIntImm],
          rd_arm: LOpData(rd) => {
              match rd {
                  BOperand::Reg(_) => {
                      crate::debug::info!("Bind existing vreg {:?} with op {:?} in function {:?}", rd, op_id, current_function);
                      // Bind the operation with the existing virt reg.
                      vregs.add_def(*rd, op_id);
                  }
                  BOperand::Undef => {
                      // Allocate a new virt reg for the operation.
                      let new_vreg = vregs.alloc(VirtReg::new(typ));
                      // Bind the new vreg with the operation.
                      *rd = BOperand::Reg(Reg::Virt(new_vreg));
                      // Bind the operation with the virt reg.
                      vregs.add_def(BOperand::Reg(Reg::Virt(new_vreg)), op_id);
                      crate::debug::info!("Bind new vreg {:?} with op {:?} in function {:?}", rd, op_id, current_function);
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_)
                  | BOperand::BB(_)
                  | BOperand::Slot(_)
                  | BOperand::IntImm(_)
                  | BOperand::Extern(_)
                  | BOperand::FloatImm(_)
                  | BOperand::Func(_)
                  | BOperand::Inst(_) => unreachable!("Invalid rd operand {:?} in LOpData", rd),
              }
          },
          fallback: {
              // Only Move can be binded with vreg, since other LOp with rd field are not created for temp values.
              LOpData::Br {..}
              | LOpData::Jump {..}
              | LOpData::Store {..}
              | LOpData::Call {..}
              | LOpData::Ret => {/*do nothing*/},
          }
      },

      BOpData::M(mop_data) => match_rd! {
          target: mop_data,
          op_with_rds: [
              Li, La, Mv, FmvS,
              Addw, Subw, Mulw, Divw, Remw,
              Slliw, Srliw, Sraiw,
              Sllw, Srlw, Sraw,
              Slt, Slti, Sltu, Sltiu,
              Addiw, Subiw, Muliw, Diviw, Remiw,
              Xor, Xori,
              FaddS, FsubS, FmulS, FdivS,
              FeqS, FltS, FleS, FneS, FgtS, FgeS,
              FcvtWS, FcvtSW, FmvWX, FmvXW,
              Lw, Flw, Ld
          ],
          rd_arm: MOpData(rd) => {
              match rd {
                  BOperand::Reg(_) => {
                      // Bind the operation with the existing virt reg.
                      vregs.add_def(*rd, op_id);
                  }
                  BOperand::Undef => {
                      let new_vreg = vregs.alloc(VirtReg::new(typ));
                      // Bind the new vreg with the operation.
                      *rd = BOperand::Reg(Reg::Virt(new_vreg));
                      // Bind the operation with the virt reg.
                      vregs.add_def(BOperand::Reg(Reg::Virt(new_vreg)), op_id);
                  }
                  BOperand::Data(_)
                  | BOperand::RoData(_)
                  | BOperand::Bss(_)
                  | BOperand::BB(_)
                  | BOperand::Slot(_)
                  | BOperand::IntImm(_)
                  | BOperand::FloatImm(_)
                  | BOperand::Extern(_)
                  | BOperand::Func(_)
                  | BOperand::Inst(_) => unreachable!("Invalid rd operand {:?} in MOpData", rd),
              }
          },
          fallback: {
              // For other MOpData which doesn't have rd field, we return Undef.
              MOpData::Sw { .. }
              | MOpData::Fsw { .. }
              | MOpData::Sd { .. }
              | MOpData::J { .. }
              | MOpData::Bnez { .. }
              | MOpData::Call { .. }
              | MOpData::Ret
              | MOpData::Beq { .. }
              | MOpData::Bne { .. }
              | MOpData::Blt { .. }
              | MOpData::Bge { .. }
              | MOpData::Bltu { .. }
              | MOpData::Bgeu { .. } => {/*do nothing*/},
          }
      },
    };
  }

  pub fn create_at_head(
    &mut self,
    builder: &mut BBuilder,
    current_function: Option<BOperand>,
    op: BOp,
  ) -> BOperand {
    let bb_id = match &builder.current_block {
      Some(block) => block.get_bb_id(),
      None => panic!("BackIR create_at_head: current_block is None"),
    };

    let inst_id = {
      let cfg = self.cfg_mut_or_panic(
        current_function,
        "BackIR create_at_head: no current function",
      );
      let bb = &cfg[bb_id];
      if bb.cur.is_empty() {
        None
      } else {
        Some(bb.cur[0])
      }
    };

    builder.set_before_inst(self, current_function, inst_id);
    self.create(builder, current_function, op)
  }

  pub fn create_new_block(&mut self, current_function: Option<BOperand>) -> BOperand {
    let cfg = self.cfg_mut_or_panic(
      current_function,
      "BackIR create_new_block: no current function",
    );
    let bb_id = cfg.alloc(super::BBasicBlock::default());
    BOperand::BB(bb_id)
  }

  pub fn remove_op(
    &mut self,
    current_function: Option<BOperand>,
    op: BOperand,
    bb: Option<BOperand>,
  ) -> BOp {
    crate::debug::info!(
      "Removing op {:?}: {:?} in function {:?}",
      op,
      self.funcs[current_function.unwrap()].dfg[op].data,
      current_function
    );

    self.remove_def(current_function, op);
    self.remove_uses(current_function, op);
    if let Some(bb_id) = bb {
      self.remove_control_flow(current_function, op, bb_id);
    }

    let (cfg, dfg) =
      self.cfg_dfg_mut_or_panic(current_function, "BackIR remove_op: no current function");

    let op_id = op.get_inst_id();
    let bb_id = bb
      .unwrap_or_else(|| {
        panic!(
          "BackIR remove_op: bb is None when removing instruction {:?}",
          op
        )
      })
      .get_bb_id();
    let bb = &mut cfg[bb_id];

    if let Some(pos) = bb.cur.iter().position(|id| id.get_inst_id() == op_id) {
      bb.cur.remove(pos);
    } else {
      panic!(
        "BackIR remove_op: instruction {:?} not found in block {:?}",
        op, bb_id
      );
    }

    let removed_op = match std::mem::replace(&mut dfg.storage[op_id], ArenaItem::None) {
      ArenaItem::Data(data) => data,
      _ => panic!("BackIR remove_op: dfg slot {} is not data", op_id),
    };

    crate::debug::info!(
      "Removed op {:?}: {:?} in block {:?} of function {:?}",
      op,
      removed_op,
      bb_id,
      current_function
    );

    // We don't check whether the old vreg's uses are all removed, since the vreg might be defined my multiple operations.
    removed_op
  }

  /// This replacement method is for those operations which no longer keep SSA form,
  /// like ABI binding operations and phi moves.
  pub fn replace_op_no_rauw(
    &mut self,
    builder: &mut BBuilder,
    current_function: Option<BOperand>,
    op_id: BOperand,
    bb_id: BOperand,
    new_op: BOp,
  ) -> BOperand {
    crate::debug::info!(
      "Replacing op {:?} with {:?} in function {:?}",
      op_id,
      new_op,
      current_function
    );

    let pos = {
      let cfg = self.cfg_mut_or_panic(
        current_function,
        "BackIR replace_op_no_rauw: no current function",
      );
      let bb = &cfg[bb_id];
      bb.cur
        .iter()
        .position(|id| id.get_inst_id() == op_id.get_inst_id())
        .unwrap_or_else(|| {
          panic!(
            "BackIR replace_op_no_rauw: instruction {:?} not found in block {:?}",
            op_id, bb_id
          )
        })
    };

    let next_inst = {
      let cfg = self.cfg_mut_or_panic(
        current_function,
        "BackIR replace_op_no_rauw: no current function",
      );
      let bb = &cfg[bb_id.get_bb_id()];
      bb.cur.get(pos + 1).cloned()
    };

    {
      let mut guard = BBuilderGuard::new(builder);
      guard.set_current_block(bb_id);
      // We won't bind the new operation with the old vreg. We create a new one directly.
      guard.set_before_inst(self, current_function, next_inst);
      // Remove the old operation.
      self.remove_op(current_function, op_id, Some(bb_id));
      self.create(&guard, current_function, new_op)
    }
  }

  /// This replacement method is for those operations which keep SSA form.
  pub fn replace_op_rauw(
    &mut self,
    builder: &mut BBuilder,
    current_function: Option<BOperand>,
    op_id: BOperand,
    bb_id: BOperand,
    new_op: BOp,
  ) -> BOperand {
    crate::debug::info!(
      "Replacing op {:?} with {:?} in function {:?}",
      op_id,
      new_op,
      current_function
    );

    let pos = {
      let cfg = self.cfg_mut_or_panic(
        current_function,
        "BackIR replace_op_rauw: no current function",
      );
      let bb = &cfg[bb_id];
      bb.cur
        .iter()
        .position(|id| id.get_inst_id() == op_id.get_inst_id())
        .unwrap_or_else(|| {
          panic!(
            "BackIR replace_op_rauw: instruction {:?} not found in block {:?}",
            op_id, bb_id
          )
        })
    };

    let next_inst = {
      let cfg = self.cfg_mut_or_panic(
        current_function,
        "BackIR replace_op_rauw: no current function",
      );
      let bb = &cfg[bb_id.get_bb_id()];
      bb.cur.get(pos + 1).cloned()
    };

    {
      let mut guard = BBuilderGuard::new(builder);
      guard.set_current_block(bb_id);
      // We won't bind the new operation with the old vreg. We create a new one directly.
      guard.set_before_inst(self, current_function, next_inst);
      let new_op_id = self.create(&guard, current_function, new_op);
      // RAUW
      self.replace_all_uses(current_function, op_id, new_op_id);
      // Remove the old operation.
      self.remove_op(current_function, op_id, Some(bb_id));
      new_op_id
    }
  }

  pub fn move_op_to_bb_at(
    &mut self,
    current_function: Option<BOperand>,
    op: BOperand,
    old_bb: BOperand,
    new_bb: BOperand,
    pos: Option<BOperand>,
  ) {
    let cfg = self.cfg_mut_or_panic(
      current_function,
      "BackIR move_op_to_bb_at: no current function",
    );

    let op_id = op.get_inst_id();
    let old_bb_id = old_bb.get_bb_id();

    let old_bb_ref = &mut cfg[old_bb_id];
    if let Some(pos) = old_bb_ref
      .cur
      .iter()
      .position(|id| id.get_inst_id() == op_id)
    {
      old_bb_ref.cur.remove(pos);
    } else {
      panic!(
        "BackIR move_op_to_bb_at: instruction {:?} not found in old_bb {:?}",
        op, old_bb
      );
    }

    let new_bb_id = new_bb.get_bb_id();
    let new_bb_ref = &mut cfg[new_bb_id];
    if let Some(pos) = pos {
      let pos_id = pos.get_inst_id();
      if let Some(pos) = new_bb_ref
        .cur
        .iter()
        .position(|id| id.get_inst_id() == pos_id)
      {
        new_bb_ref.cur.insert(pos, op);
      } else {
        panic!(
          "BackIR move_op_to_bb_at: instruction {:?} not found in new_bb {:?}",
          pos, new_bb
        );
      }
    } else {
      new_bb_ref.cur.push(op);
    }
  }

  pub fn get_rd_tuple(
    &self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Option<(&BOperand, usize)> {
    let current_function = current_function.expect("No current function");
    self.funcs[current_function].get_rd_tuple(inst_id)
  }

  pub fn get_src_tuple(
    &self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Vec<(&BOperand, usize)> {
    let current_function = current_function.expect("No current function");
    self.funcs[current_function].get_src_tuple(inst_id)
  }

  pub fn get_rd_tuple_mut(
    &mut self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Option<(&mut BOperand, usize)> {
    let current_function = current_function.expect("No current function");
    self.funcs[current_function].get_rd_tuple_mut(inst_id)
  }

  pub fn get_src_tuple_mut(
    &mut self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Vec<(&mut BOperand, usize)> {
    let current_function = current_function.expect("No current function");
    self.funcs[current_function].get_src_tuple_mut(inst_id)
  }

  pub fn get_rd(&self, current_function: Option<BOperand>, inst_id: BOperand) -> Option<&BOperand> {
    self
      .get_rd_tuple(current_function, inst_id)
      .map(|(rd, _)| rd)
  }

  pub fn get_src(&self, current_function: Option<BOperand>, inst_id: BOperand) -> Vec<&BOperand> {
    self
      .get_src_tuple(current_function, inst_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }

  pub fn get_rd_mut(
    &mut self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Option<&mut BOperand> {
    self
      .get_rd_tuple_mut(current_function, inst_id)
      .map(|(rd, _)| rd)
  }

  pub fn get_src_mut(
    &mut self,
    current_function: Option<BOperand>,
    inst_id: BOperand,
  ) -> Vec<&mut BOperand> {
    self
      .get_src_tuple_mut(current_function, inst_id)
      .into_iter()
      .map(|(src, _)| src)
      .collect()
  }
}
