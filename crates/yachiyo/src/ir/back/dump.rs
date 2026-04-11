//! Dump Machine IR to RISC-V Assembly.

use crate::ir::back::{BOp, BOpData, BOperand, BackIR, MOpData};

use std::collections::HashMap;

impl BackIR {
  pub fn dump(&self) -> String {
    self.dump_riscv_asm()
  }

  pub fn dump_riscv_asm(&self) -> String {
    let mut out = String::new();

    let data_name_map = reverse_name_map(&self.data_info.map);
    let rodata_name_map = reverse_name_map(&self.rodata_info.map);
    let bss_name_map = reverse_name_map(&self.bss_info.map);
    let func_name_map = self
      .funcs
      .collect()
      .into_iter()
      .map(|id| (id, self.funcs[id].name.clone()))
      .collect();

    self.dump_data_section(&mut out, &data_name_map, &rodata_name_map, &func_name_map);
    self.dump_rodata_section(&mut out, &data_name_map, &rodata_name_map, &func_name_map);
    self.dump_bss_section(&mut out, &bss_name_map);
    self.dump_text_section(
      &mut out,
      &data_name_map,
      &rodata_name_map,
      &bss_name_map,
      &func_name_map,
    );

    out
  }

  fn dump_data_section(
    &self,
    out: &mut String,
    data_name_map: &HashMap<usize, String>,
    rodata_name_map: &HashMap<usize, String>,
    func_name_map: &HashMap<usize, String>,
  ) {
    if self.data_info.is_empty() {
      return;
    }

    let mut ids = self.data_info.collect();
    ids.sort_unstable();

    out.push_str(".section .data\n");

    for id in ids {
      let data = &self.data_info[id];
      let label = symbol_name(data_name_map, id, ".data");
      out.push_str(&format!(".globl {label}\n"));
      if data.align() > 1 {
        out.push_str(&format!(".align {}\n", data.align().trailing_zeros()));
      }
      out.push_str(&format!("{label}:\n"));
      dump_initializer(
        out,
        data.inner(),
        data.size(),
        data_name_map,
        rodata_name_map,
        func_name_map,
      );
    }

    out.push('\n');
  }

  fn dump_rodata_section(
    &self,
    out: &mut String,
    data_name_map: &HashMap<usize, String>,
    rodata_name_map: &HashMap<usize, String>,
    func_name_map: &HashMap<usize, String>,
  ) {
    if self.rodata_info.is_empty() {
      return;
    }

    let mut ids = self.rodata_info.collect();
    ids.sort_unstable();

    out.push_str(".section .rodata\n");

    for id in ids {
      let rodata = &self.rodata_info[id];
      let label = symbol_name(rodata_name_map, id, ".rodata");
      if rodata.align() > 1 {
        out.push_str(&format!(".align {}\n", rodata.align().trailing_zeros()));
      }
      out.push_str(&format!("{label}:\n"));
      dump_initializer(
        out,
        rodata.inner(),
        rodata.size(),
        data_name_map,
        rodata_name_map,
        func_name_map,
      );
    }

    out.push('\n');
  }

  fn dump_bss_section(&self, out: &mut String, bss_name_map: &HashMap<usize, String>) {
    if self.bss_info.is_empty() {
      return;
    }

    let mut ids = self.bss_info.collect();
    ids.sort_unstable();

    out.push_str(".section .bss\n");

    for id in ids {
      let bss = &self.bss_info[id];
      let label = symbol_name(bss_name_map, id, ".bss");
      out.push_str(&format!(".globl {label}\n"));
      if bss.align() > 1 {
        out.push_str(&format!(".align {}\n", bss.align().trailing_zeros()));
      }
      out.push_str(&format!("{label}:\n"));
      out.push_str(&format!("  .zero {}\n", bss.size()));
    }

    out.push('\n');
  }

  fn dump_text_section(
    &self,
    out: &mut String,
    data_name_map: &HashMap<usize, String>,
    rodata_name_map: &HashMap<usize, String>,
    bss_name_map: &HashMap<usize, String>,
    func_name_map: &HashMap<usize, String>,
  ) {
    if self.funcs.is_empty() {
      return;
    }

    out.push_str(".section .text\n");

    let mut func_ids = self.funcs.collect();
    func_ids.sort_unstable();

    for func_id in func_ids {
      let func = &self.funcs[func_id];
      let format_ctx = AsmFormatCtx {
        data_name_map,
        rodata_name_map,
        bss_name_map,
        func_name_map,
        current_func_name: Some(&func.name),
      };
      out.push_str(&format!(".globl {}\n", func.name));
      out.push_str(&format!("{}:\n", func.name));

      let mut bb_ids = func.cfg.collect();
      bb_ids.sort_unstable();
      for bb_id in bb_ids {
        out.push_str(&format!(".{}_bb{}:\n", func.name, bb_id));
        for inst in &func.cfg[bb_id].cur {
          let inst_id = inst.get_inst_id();
          let op = &func.dfg[inst_id];
          out.push_str("  ");
          out.push_str(&format_ctx.format_mop(op));
          out.push('\n');
        }
      }

      out.push('\n');
    }
  }
}

fn reverse_name_map(map: &HashMap<String, usize>) -> HashMap<usize, String> {
  let mut rev = HashMap::with_capacity(map.len());
  for (name, id) in map {
    rev.insert(*id, name.clone());
  }
  rev
}

fn symbol_name(name_map: &HashMap<usize, String>, id: usize, fallback_prefix: &str) -> String {
  name_map
    .get(&id)
    .cloned()
    .unwrap_or_else(|| format!("{fallback_prefix}{id}"))
}

struct AsmFormatCtx<'a> {
  data_name_map: &'a HashMap<usize, String>,
  rodata_name_map: &'a HashMap<usize, String>,
  bss_name_map: &'a HashMap<usize, String>,
  func_name_map: &'a HashMap<usize, String>,
  current_func_name: Option<&'a str>,
}

impl AsmFormatCtx<'_> {
  fn format_operand(&self, operand: &BOperand) -> String {
    match operand {
      BOperand::Func(id) => symbol_name(self.func_name_map, *id, ".func"),
      BOperand::BB(id) => match self.current_func_name {
        Some(func_name) => format!(".{func_name}_bb{id}"),
        None => format!("bb{id}"),
      },
      BOperand::Inst(id) => format!("inst.{id}"),
      BOperand::Reg(reg) => reg.to_string(),
      BOperand::IntImm(imm) => imm.to_string(),
      BOperand::FloatImm(imm) => format!("0x{imm:08x}"),
      BOperand::Slot(id) => format!("slot.{id}"),
      BOperand::Data(id) => symbol_name(self.data_name_map, *id, ".data"),
      BOperand::RoData(id) => symbol_name(self.rodata_name_map, *id, ".rodata"),
      BOperand::Bss(id) => symbol_name(self.bss_name_map, *id, ".bss"),
      BOperand::Undef => "undef".to_string(),
    }
  }

  fn format_mop(&self, op: &BOp) -> String {
    match &op.data {
      BOpData::M(mop) => match mop {
        MOpData::Li { rd, imm } => format!("li {}, {}", self.format_operand(rd), imm),
        MOpData::La { rd, target } => {
          format!(
            "la {}, {}",
            self.format_operand(rd),
            self.format_operand(target)
          )
        }
        MOpData::Mv { rd, rs } => {
          format!(
            "mv {}, {}",
            self.format_operand(rd),
            self.format_operand(rs)
          )
        }
        MOpData::FmvS { rd, rs } => {
          format!(
            "fmv.s {}, {}",
            self.format_operand(rd),
            self.format_operand(rs)
          )
        }
        MOpData::Addw { rd, rs1, rs2 } => format!(
          "addw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Subw { rd, rs1, rs2 } => format!(
          "subw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Mulw { rd, rs1, rs2 } => format!(
          "mulw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Divw { rd, rs1, rs2 } => format!(
          "divw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Remw { rd, rs1, rs2 } => format!(
          "remw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Sllw { rd, rs1, rs2 } => format!(
          "sllw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Srlw { rd, rs1, rs2 } => format!(
          "srlw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Sraw { rd, rs1, rs2 } => format!(
          "sraw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Slt { rd, rs1, rs2 } => format!(
          "slt {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Slti { rd, rs1, imm } => format!(
          "slti {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Sltu { rd, rs1, rs2 } => format!(
          "sltu {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Sltiu { rd, rs1, imm } => format!(
          "sltiu {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Addiw { rd, rs1, imm } => format!(
          "addiw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Subiw { rd, rs1, imm } => format!(
          "subiw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Muliw { rd, rs1, imm } => format!(
          "muliw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Diviw { rd, rs1, imm } => format!(
          "diviw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Remiw { rd, rs1, imm } => format!(
          "remiw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Slliw { rd, rs1, imm } => format!(
          "slliw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Srliw { rd, rs1, imm } => format!(
          "srliw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Sraiw { rd, rs1, imm } => format!(
          "sraiw {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::Xor { rd, rs1, rs2 } => format!(
          "xor {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::Xori { rd, rs1, imm } => format!(
          "xori {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(imm)
        ),
        MOpData::FaddS { rd, rs1, rs2 } => format!(
          "fadd.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FsubS { rd, rs1, rs2 } => format!(
          "fsub.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FmulS { rd, rs1, rs2 } => format!(
          "fmul.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FdivS { rd, rs1, rs2 } => format!(
          "fdiv.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FeqS { rd, rs1, rs2 } => format!(
          "feq.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FltS { rd, rs1, rs2 } => format!(
          "flt.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FleS { rd, rs1, rs2 } => format!(
          "fle.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FneS { rd, rs1, rs2 } => format!(
          "fne.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FgtS { rd, rs1, rs2 } => format!(
          "fgt.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FgeS { rd, rs1, rs2 } => format!(
          "fge.s {}, {}, {}",
          self.format_operand(rd),
          self.format_operand(rs1),
          self.format_operand(rs2)
        ),
        MOpData::FcvtWS { rd, rs } => format!(
          "fcvt.w.s {}, {}",
          self.format_operand(rd),
          self.format_operand(rs)
        ),
        MOpData::FcvtSW { rd, rs } => format!(
          "fcvt.s.w {}, {}",
          self.format_operand(rd),
          self.format_operand(rs)
        ),
        MOpData::FmvWX { rd, rs } => format!(
          "fmv.w.x {}, {}",
          self.format_operand(rd),
          self.format_operand(rs)
        ),
        MOpData::FmvXW { rd, rs } => format!(
          "fmv.x.w {}, {}",
          self.format_operand(rd),
          self.format_operand(rs)
        ),
        MOpData::Lw { rd, base, offset } => format!(
          "lw {}, {}({})",
          self.format_operand(rd),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::Sw { rs, base, offset } => format!(
          "sw {}, {}({})",
          self.format_operand(rs),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::Flw { rd, base, offset } => format!(
          "flw {}, {}({})",
          self.format_operand(rd),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::Fsw { rs, base, offset } => format!(
          "fsw {}, {}({})",
          self.format_operand(rs),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::Ld { rd, base, offset } => format!(
          "ld {}, {}({})",
          self.format_operand(rd),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::Sd { rs, base, offset } => format!(
          "sd {}, {}({})",
          self.format_operand(rs),
          self.format_operand(offset),
          self.format_operand(base)
        ),
        MOpData::J { target } => format!("j {}", self.format_operand(target)),
        MOpData::Call { target } => format!("call {}", self.format_operand(target)),
        MOpData::Ret => "ret".to_string(),
        MOpData::Bnez { rs, target } => format!(
          "bnez {}, {}",
          self.format_operand(rs),
          self.format_operand(target)
        ),
        MOpData::Beq { rs1, rs2, offset } => format!(
          "beq {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
        MOpData::Bne { rs1, rs2, offset } => format!(
          "bne {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
        MOpData::Blt { rs1, rs2, offset } => format!(
          "blt {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
        MOpData::Bge { rs1, rs2, offset } => format!(
          "bge {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
        MOpData::Bltu { rs1, rs2, offset } => format!(
          "bltu {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
        MOpData::Bgeu { rs1, rs2, offset } => format!(
          "bgeu {}, {}, {}",
          self.format_operand(rs1),
          self.format_operand(rs2),
          self.format_operand(offset)
        ),
      },
      BOpData::L(lop) => format!("{lop}"),
    }
  }
}

fn dump_initializer(
  out: &mut String,
  inner: &[BOperand],
  total_size: u32,
  data_name_map: &HashMap<usize, String>,
  rodata_name_map: &HashMap<usize, String>,
  func_name_map: &HashMap<usize, String>,
) {
  let mut written = 0u32;

  for op in inner {
    match op {
      BOperand::IntImm(v) => {
        out.push_str(&format!("  .word {}\n", *v));
        written += 4;
      }
      BOperand::FloatImm(v) => {
        out.push_str(&format!("  .word 0x{:08x}\n", v));
        written += 4;
      }
      BOperand::Undef => {
        out.push_str("  .zero 4\n");
        written += 4;
      }
      BOperand::Data(id) => {
        let label = symbol_name(data_name_map, *id, ".data");
        out.push_str(&format!("  .dword {}\n", label));
        written += 8;
      }
      BOperand::RoData(id) => {
        let label = symbol_name(rodata_name_map, *id, ".rodata");
        out.push_str(&format!("  .dword {}\n", label));
        written += 8;
      }
      BOperand::Bss(_) => {
        panic!("dump_initializer: .bss symbol should not appear in concrete global initializers");
      }
      BOperand::Func(id) => {
        let label = symbol_name(func_name_map, *id, ".func");
        out.push_str(&format!("  .dword {}\n", label));
        written += 8;
      }
      BOperand::Reg(_) | BOperand::BB(_) | BOperand::Inst(_) | BOperand::Slot(_) => {
        panic!(
          "dump_initializer: unsupported operand in global initializer: {:?}",
          op
        );
      }
    }
  }

  if total_size > written {
    out.push_str(&format!("  .zero {}\n", total_size - written));
  }
}
