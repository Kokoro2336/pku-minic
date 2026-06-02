//! Reassociate the instructions.

use yachiyo::ir::mid::{IR, Operand, OpData, Op};
use yachiyo::pass::{Pass, PassContext};

use kaguya::kaguya_hime;

#[derive(Default)]
pub struct Reassociate<'a> {
  cx: PassContext<'a>,
}

impl Reassociate<'_> {
  fn init(&mut self, func_id: Operand) {
    self.cx.set_current_func(Some(func_id));
  }

  fn run(&mut self) {
    let func_id = self.cx.get_current_func_id();
    for bb_id in self.cx.bbs(func_id) {
      self.cx.set_current_block(bb_id);

      for inst_id in self.cx.get_bb(bb_id).cur.clone() {
        self.cx.set_before_inst(Some(inst_id));

        kaguya_hime!(
          self.cx, 
          match inst_id {
            AddI(AddI($lhs, Int($c1)), Int($c2)) | AddI(Int($c1), AddI($lhs, Int($c2))) | AddI(AddI(Int($c1), $lhs), Int($c2)) | AddI(Int($c1), AddI(Int($c2), $lhs)) => {
              // Reassociate (a + c1) + c2 to a + (c1 + c2).
              let reassociated = self.cx.create(Op::new(
                self.cx.get_op_type(inst_id),
                vec![],
                OpData::AddI {
                  lhs,
                  rhs: Operand::Int(c1 + c2),
                },
              ));
              self.cx.replace_all_uses(inst_id, reassociated);
            },
            MulI(MulI($lhs, Int($c1)), Int($c2)) | MulI(Int($c1), MulI($lhs, Int($c2))) | MulI(MulI(Int($c1), $lhs), Int($c2)) | MulI(Int($c1), MulI(Int($c2), $lhs)) => {
              // Reassociate (a * c1) * c2 to a * (c1 * c2).
              let reassociated = self.cx.create(Op::new(
                self.cx.get_op_type(inst_id),
                vec![],
                OpData::MulI {
                  lhs,
                  rhs: Operand::Int(c1 * c2),
                },
              ));
              self.cx.replace_all_uses(inst_id, reassociated);
            },
          }
        );
      }
    }
  }
}

impl<'a> Pass<'a> for Reassociate<'a> {
  fn name(&self) -> &str {
    "Reassociate"
  }
  fn mount(&mut self, ir: &'a mut IR) {
    self.cx.mount(ir);
  }
  fn run(&mut self) {
    for func_id in self.cx.funcs_internal() {
      self.init(func_id);
      self.run();
    }
  }
}
