//! Peephole Optimization.

use yachiyo::ir::back::{BOpData, BOperand, BackIR, LOpData, MOpData};
use yachiyo::pass::{BPass, BPassContext};
use yachiyo::utils::r#match::match_some;

#[derive(Default)]
pub struct Peephole<'a> {
  cx: BPassContext<'a>,
}

impl Peephole<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.cx.set_current_func(func_id);
  }

  pub fn combine(&mut self) {
    let func_id = self.cx.current_func();
    let bb_ids = self.cx.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      let inst_ids = self.cx.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        let op_data = self.cx.get_func(func_id).dfg[inst_id].data.clone();
        match op_data {
          BOpData::L(lop_data) => match_some! {
              target: lop_data,
              enu: LOpData,
              minor_arms: {
                  LOpData::Move { rd, src } => {
                      // If the source and destination are the same, we can remove this instruction directly.
                      if rd == src {
                          self.cx.remove_op(inst_id, Some(bb_id));
                      }
                  }
              },
              uni_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Store, Xor, Shl, Sar, Shr, AddF, SubF, MulF, DivF, Sitofp, Fptosi, Load, LoadIntImm, LoadFloatImm, LoadAddress, Call, Br, Jump, Ret],
              uni_arm: {}
          },
          BOpData::M(mop_data) => match_some! {
              target: mop_data,
              enu: MOpData,
              minor_arms: {
                  MOpData::Mv { rd, rs } => {
                      if rd == rs {
                          self.cx.remove_op(inst_id, Some(bb_id));
                      }
                  },
                  MOpData::FmvS { rd, rs } => {
                      if rd == rs {
                          self.cx.remove_op(inst_id, Some(bb_id));
                      }
                  }
              },
              uni_ops: [Li, La, Add, Sub, Addi, Addw, Subw, Mulw, Divw, Remw, Sllw, Sraw, Srlw, Slt, Slti, Sltu, Sltiu, Addiw, Slliw, Srliw, Sraiw, Xor, FmvS, FaddS, FsubS, FmulS, FdivS, FeqS, Xori, FltS, FleS, FcvtSW, FcvtWS, FmvWX, FmvXW, Lw, Sw, Flw, Ld, Sd, Fld, Fsd, Fsw, J, Bnez, Ret, Bne, Beq, Blt, Bge, Bltu, Bgeu, Call],
              uni_arm: {}
          },
        }
      }
    }
  }
}

impl<'a> BPass<'a> for Peephole<'a> {
  fn name(&self) -> &str {
    "Peephole"
  }

  fn mount(&mut self, ir: &'a mut BackIR) {
    self.cx.mount(ir);
  }

  fn run(&mut self) {
    for func_id in self.cx.ir().funcs.collect_internal() {
      self.init(BOperand::Func(func_id));
      self.combine();
    }
  }
}
