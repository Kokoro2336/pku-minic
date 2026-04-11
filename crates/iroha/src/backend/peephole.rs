//! Peephole Optimization for BackIR.

use yachiyo::ir::back::{BBuilder, BFunction, BOpData, BOperand, BackIR, LOpData, MOpData};
use yachiyo::pass::BPass;
use yachiyo::utils::r#match::match_some;

#[derive(Default)]
pub struct Peephole<'a> {
  ir: Option<&'a mut BackIR>,
  builder: BBuilder,
}

impl Peephole<'_> {
  #[inline(always)]
  fn init(&mut self, func_id: BOperand) {
    self.builder.set_current_func(func_id);
  }

  #[inline(always)]
  fn get_func(&self, func_id: BOperand) -> &BFunction {
    &self.ir.as_ref().unwrap().funcs[func_id]
  }

  #[inline(always)]
  pub fn get_func_mut(&mut self, func_id: BOperand) -> &mut BFunction {
    &mut self.ir.as_mut().unwrap().funcs[func_id]
  }

  #[inline(always)]
  pub fn remove_op(&mut self, op_id: BOperand, bb_id: BOperand) {
    let func_id = self.builder.current_function;
    self
      .ir
      .as_mut()
      .unwrap()
      .remove_op(func_id, op_id, Some(bb_id));
  }

  pub fn combine(&mut self) {
    let func_id = self.builder.current_function.unwrap();
    let bb_ids = self.get_func(func_id).cfg.ids();
    for bb_id in bb_ids {
      let bb_id = BOperand::BB(bb_id);
      let inst_ids = self.get_func(func_id).cfg[bb_id].cur.clone();
      for inst_id in inst_ids {
        let op_data = self.get_func(func_id).dfg[inst_id].data.clone();
        match op_data {
          BOpData::L(lop_data) => match_some! {
              target: lop_data,
              enu: LOpData,
              minor_arms: {
                  LOpData::Move { rd, src } => {
                      // If the source and destination are the same, we can remove this instruction directly.
                      if rd == src {
                          self.remove_op(inst_id, bb_id);
                      }
                  }
              },
              uni_ops: [AddI, SubI, MulI, DivI, ModI, SNe, SEq, SGt, SLt, SGe, SLe, ONe, OEq, OGt, OLt, OGe, OLe, Store, Xor, Shl, Sar, Shr, AddF, SubF, MulF, DivF, Sitofp, Fptosi, Load, LoadIntImm, LoadFloatImm, Call, Br, Jump, Ret],
              uni_arm: {}
          },
          BOpData::M(mop_data) => match_some! {
              target: mop_data,
              enu: MOpData,
              minor_arms: {
                  MOpData::Mv { rd, rs } => {
                      if rd == rs {
                          self.remove_op(inst_id, bb_id);
                      }
                  },
                  MOpData::FmvS { rd, rs } => {
                      if rd == rs {
                          self.remove_op(inst_id, bb_id);
                      }
                  }
              },
              uni_ops: [Li, La, Addw, Subw, Mulw, Divw, Remw, Sllw, Sraw, Srlw, Slt, Slti, Sltu, Sltiu, Addiw, Slliw, Srliw, Sraiw, Subiw, Muliw, Xor, FmvS, FaddS, FsubS, FmulS, FdivS, FeqS, Diviw, Remiw, Xori, FltS, FleS, FneS, FgtS, FgeS, FcvtSW, FcvtWS, FmvWX, FmvXW, Lw, Sw, Flw, Ld, Sd, Fsw, J, Bnez, Ret, Bne, Beq, Blt, Bge, Bltu, Bgeu, Call],
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
    self.ir = Some(ir);
  }

  fn run(&mut self) {
    for func_id in self.ir.as_ref().unwrap().funcs.ids() {
      self.init(BOperand::Func(func_id));
      self.combine();
    }
  }
}
