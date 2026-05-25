//! Instruction Combination.

use yachiyo::ir::back::{BOpData, BOperand, BackIR, LOpData, MOpData, Reg, XReg};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::match_some;

#[derive(Default)]
pub struct InstCombine<'a> {
  cx: BPassContext<'a>,
}

impl InstCombine<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  fn run(&mut self) {
    let func_id = self.cx.get_current_func_id();
    let bb_ids = self.cx.get_func(func_id).cfg.dpo();
    for &bb_id in bb_ids.iter().rev() {
      let bb_id = BOperand::BB(bb_id);
      self.cx.set_current_block(bb_id);
      let inst_ids = self.cx.get_bb(bb_id).cur.clone();
      for inst_id in inst_ids {
        self.cx.set_before_inst(Some(inst_id));
        let bop = self.cx.get_op(inst_id);
        let bop_data = bop.data.clone();

        match bop_data {
          BOpData::L(lop_data) => match_some! {
            target: lop_data,
            enu: LOpData,
            minor_arms: {
              LOpData::MulI { lhs, rhs, .. } => {
                if rhs.is_zero() {
                  self.cx.replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
                } else if rhs == BOperand::IntImm(1) {
                  self.cx.replace_all_uses(inst_id, lhs);
                }
              },
              LOpData::DivI { lhs, rhs, .. } => {
                if rhs == BOperand::IntImm(1) {
                  self.cx.replace_all_uses(inst_id, lhs);
                }
              },
              LOpData::ModI { rhs, .. } => {
                if rhs == BOperand::IntImm(1) || rhs == BOperand::IntImm(-1) {
                  self.cx.replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
                }
              },
              LOpData::AddI { lhs, rhs, .. }
              | LOpData::SubI { lhs, rhs, .. }
              | LOpData::Shl { lhs, rhs, .. }
              | LOpData::Shr { lhs, rhs, .. }
              | LOpData::Sar { lhs, rhs, .. } => {
                if rhs.is_zero() {
                  self.cx.replace_all_uses(inst_id, lhs);
                }
              }
            },
            uni_ops: [
              AddF, SubF, MulF, DivF,
              SNe, SEq, SGt, SLt, SGe, SLe,
              Xor, And,
              ONe, OEq, OGt, OLt, OGe, OLe,
              Sitofp, Fptosi, Store, Load, Move,
              LoadIntImm, LoadFloatImm, LoadAddress,
              Call, Br, Jump, Ret
            ],
            uni_arm: {}
          },
          BOpData::M(mop_data) => match mop_data {
            MOpData::Li { imm: 0, .. } => {
              self
                .cx
                .replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
            }
            MOpData::Mulw { rs1, rs2, .. } => {
              if rs1.is_zero() || rs2.is_zero() {
                self
                  .cx
                  .replace_all_uses(inst_id, BOperand::Reg(Reg::X(XReg::Zero)));
              }
            }
            MOpData::Add { rs1, rs2, .. } | MOpData::Addw { rs1, rs2, .. } => {
              if rs2.is_zero() {
                self.cx.replace_all_uses(inst_id, rs1);
              } else if rs1.is_zero() {
                self.cx.replace_all_uses(inst_id, rs2);
              }
            }
            MOpData::Sub { rs1, rs2, .. }
            | MOpData::Subw { rs1, rs2, .. }
            | MOpData::Sllw { rs1, rs2, .. }
            | MOpData::Srlw { rs1, rs2, .. }
            | MOpData::Sraw { rs1, rs2, .. } => {
              if rs2.is_zero() {
                self.cx.replace_all_uses(inst_id, rs1);
              }
            }
            MOpData::Addi { rs1, imm, .. }
            | MOpData::Addiw { rs1, imm, .. }
            | MOpData::Slliw { rs1, imm, .. }
            | MOpData::Srliw { rs1, imm, .. }
            | MOpData::Sraiw { rs1, imm, .. } => {
              if imm.is_zero() {
                self.cx.replace_all_uses(inst_id, rs1);
              }
            }
            _ => {}
          },
        }
      }
    }
  }
}

impl<'a> BPass<'a> for InstCombine<'a> {
  fn name(&self) -> &str {
    "InstCombine"
  }

  fn mount(&mut self, ir: &'a mut BackIR) {
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.init(BOperand::Func(func_id));
      self.run();
    }
  }
}
