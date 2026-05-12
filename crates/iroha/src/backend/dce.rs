//! Backend Dead Code Elimation (BDCE).
//! P.S.: This is just a trivial DCE that has nothing to do with ADCE!!!

use yachiyo::ir::back::{BOpData, BOperand, BackIR, LOpData, MOpData, Reg};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::r#match::{match_some, match_src};
use yachiyo::utils::set::BitSet;
use yachiyo::utils::worklist::Worklist;

#[allow(clippy::upper_case_acronyms)]
#[derive(Default)]
pub struct BDCE<'a> {
  cx: BPassContext<'a>,
  // Worklist of inst
  worklist: Worklist<BOperand, BitSet>,
}

impl BDCE<'_> {
  pub fn is_dead(&self, operand: BOperand) -> bool {
    let current_func = self.cx.get_func(self.cx.current_func());
    let vregs = &current_func.vregs;

    match_some! {
        target: operand,
        enu: BOperand,
        minor_arms: {
            BOperand::Inst(_) => {
                let Some(rd) = self.cx.get_rd(operand) else {
                    return false;
                };
                match_some! {
                    target: rd,
                    enu: BOperand,
                    minor_arms: {
                        BOperand::Reg(Reg::Virt(_)) => vregs[*rd].uses.is_empty(),
                        BOperand::Reg(Reg::X(_) | Reg::F(_)) => false,
                    },
                    uni_ops: [Undef, BB, IntImm, FloatImm, Inst, Func, Data, RoData, Bss, Slot],
                    uni_arm: false
                }
            }
            BOperand::Reg(Reg::Virt(_)) => vregs[operand].uses.is_empty(),
            BOperand::Reg(Reg::X(_) | Reg::F(_)) => false,
        },
        uni_ops: [Undef, BB, IntImm, FloatImm, Inst, Func, Data, RoData, Bss, Slot],
        uni_arm: false
    }
  }

  pub fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
    self.worklist.clear();

    // Initialize the worklist
    let block_ids = self.cx.get_func(func_id).cfg.collect();
    for block_id in block_ids {
      let block = self.cx.get_bb(BOperand::BB(block_id));
      for inst_id in block.cur.iter() {
        let is_impure = self.cx.get_op_data(*inst_id).is_impure();
        if self.is_dead(*inst_id) && !is_impure {
          self.worklist.push_back(*inst_id);
        }
      }
    }
  }
}

impl<'a> BPass<'a> for BDCE<'a> {
  fn name(&self) -> &str {
    "BDCE"
  }
  fn mount(&mut self, program: &'a mut BackIR) {
    self.cx.mount(program);
  }

  fn run(&mut self) {
    fn check(this: &mut BDCE<'_>, operand: BOperand) {
      if !this.is_dead(operand) {
        return;
      }

      match_some! {
          target: operand,
          enu: BOperand,
          minor_arms: {
              BOperand::Inst(op_id) => {
                let op = BOperand::Inst(op_id);

                let should_push = !this.cx.get_op_data(op).is_impure();

                if should_push {
                    this.worklist.push_back(op);
                }
              }
              BOperand::Reg(Reg::Virt(_)) => {
                let defs = {
                    let func = this.cx.get_func(this.cx.current_func());
                    func.vregs[operand].defs.clone()
                };

                for def in defs {
                    let should_push = !this.cx.get_op_data(def).is_impure();

                    if should_push {
                        this.worklist.push_back(def);
                    }
                }
              }
          },
          uni_ops: [Reg, Undef, BB, IntImm, FloatImm, Func, Data, RoData, Bss, Slot],
          uni_arm: {}
      }
    }

    let func_ids = self.cx.ir().funcs.collect_internal();

    for func_id in func_ids {
      self.init(BOperand::Func(func_id));
      while let Some(op_id) = self.worklist.pop_back() {
        let bb_id = self.cx.op_bb(op_id);
        self.cx.set_current_block(bb_id);

        let removed_op = self.cx.remove_op(op_id, Some(bb_id));

        // Check the operands of the removed instruction
        match removed_op.data {
          BOpData::L(lop_data) => match_src! {
              target: lop_data,
              bin_ops: [
                  AddI, SubI, MulI, DivI, ModI,
                  SNe, SEq, SGt, SLt, SGe, SLe,
                  Xor, And, Shl, Shr, Sar,
                  AddF, SubF, MulF, DivF,
                  ONe, OEq, OGt, OLt, OGe, OLe
              ],
              bin_arm: LOpData { lhs, rhs } => {
                  check(self, lhs);
                  check(self, rhs);
              },
              un_ops: [Sitofp, Fptosi],
              un_arm: LOpData { value } => {
                  check(self, value);
              },
              fallback: {
                  LOpData::Store { addr, value, .. } => {
                      check(self, addr);
                      check(self, value);
                  }
                  LOpData::Load { addr, .. } => {
                      check(self, addr);
                  }
                  LOpData::Move { src, .. } => {
                      check(self, src);
                  }
                  LOpData::Br { cond, .. } => {
                      check(self, cond);
                  }
                  LOpData::Call { func } => {
                      check(self, func);
                  }
                  LOpData::Jump { .. }
                  | LOpData::Ret
                  | LOpData::LoadIntImm { .. }
                  | LOpData::LoadFloatImm { .. }
                  | LOpData::LoadAddress { .. } => {}
              }
          },
          BOpData::M(mop_data) => match_src! {
              target: mop_data,
              bin_ops: [
                  Add, Sub, Addw, Subw, Mulw, Divw, Remw,
                  Sllw, Srlw, Sraw,
                  Slt, Sltu, Xor, And,
                  FaddS, FsubS, FmulS, FdivS,
                  FeqS, FltS, FleS,
              ],
              bin_arm: MOpData { rs1, rs2 } => {
                  check(self, rs1);
                  check(self, rs2);
              },
              un_ops: [Mv, FmvS, FcvtWS, FcvtSW, FmvWX, FmvXW],
              un_arm: MOpData { rs } => {
                  check(self, rs);
              },
              fallback: {
                  MOpData::Addi { rs1, imm, .. }
                  | MOpData::Slti { rs1, imm, .. }
                  | MOpData::Sltiu { rs1, imm, .. }
                  | MOpData::Addiw { rs1, imm, .. }
                  | MOpData::Slliw { rs1, imm, .. }
                  | MOpData::Srliw { rs1, imm, .. }
                  | MOpData::Sraiw { rs1, imm, .. }
                  | MOpData::Xori { rs1, imm, .. }
                  | MOpData::Andi { rs1, imm, .. } => {
                      check(self, rs1);
                      check(self, imm);
                  }
                  MOpData::Lw { base, offset, .. }
                  | MOpData::Flw { base, offset, .. }
                  | MOpData::Ld { base, offset, .. }
                  | MOpData::Fld { base, offset, .. } => {
                      check(self, base);
                      check(self, offset);
                  }
                  MOpData::Sw { rs, base, offset }
                  | MOpData::Fsw { rs, base, offset }
                  | MOpData::Sd { rs, base, offset }
                  | MOpData::Fsd { rs, base, offset } => {
                      check(self, rs);
                      check(self, base);
                      check(self, offset);
                  }

                  MOpData::Beq { rs1, rs2, offset }
                  | MOpData::Bne { rs1, rs2, offset }
                  | MOpData::Blt { rs1, rs2, offset }
                  | MOpData::Bge { rs1, rs2, offset }
                  | MOpData::Bltu { rs1, rs2, offset }
                  | MOpData::Bgeu { rs1, rs2, offset } => {
                      check(self, rs1);
                      check(self, rs2);
                      check(self, offset);
                  }
                  MOpData::Bnez { rs, .. } => {
                      check(self, rs);
                  }
                  MOpData::J { target } => {
                      check(self, target);
                  }
                  MOpData::Li { .. }
                  | MOpData::La { .. }
                  | MOpData::Call { .. }
                  | MOpData::Ret => {}
              }
          },
        }
      }
    }
  }
}
