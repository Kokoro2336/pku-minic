//! Dump Machine IR to RISC-V Assembly.

use crate::ir::back::{BOp, BOpData, BOperand, BackIR};

use std::collections::HashMap;

impl BackIR {
    pub fn dump(&self) -> String {
        self.dump_riscv_asm()
    }

    pub fn dump_riscv_asm(&self) -> String {
        let mut out = String::new();

        let data_name_map = reverse_name_map(&self.data_info.map);
        let rodata_name_map = reverse_name_map(&self.rodata_info.map);
        let func_name_map = reverse_name_map(&self.funcs.map);

        self.dump_data_section(&mut out, &data_name_map, &rodata_name_map, &func_name_map);
        self.dump_rodata_section(&mut out, &data_name_map, &rodata_name_map, &func_name_map);
        self.dump_text_section(&mut out);

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
            let label = symbol_name(data_name_map, id, ".Ldata");
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
            let label = symbol_name(rodata_name_map, id, ".Lrodata");
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

    fn dump_text_section(&self, out: &mut String) {
        if self.funcs.is_empty() {
            return;
        }

        out.push_str(".section .text\n");

        let mut func_ids = self.funcs.collect();
        func_ids.sort_unstable();

        for func_id in func_ids {
            let func = &self.funcs[func_id];
            out.push_str(&format!(".globl {}\n", func.name));
            out.push_str(&format!("{}:\n", func.name));

            let mut bb_ids = func.cfg.collect();
            bb_ids.sort_unstable();
            for bb_id in bb_ids {
                out.push_str(&format!(".L{}_bb{}:\n", func.name, bb_id));
                for inst in &func.cfg[bb_id].cur {
                    let inst_id = inst.get_inst_id();
                    let op = &func.dfg[inst_id];
                    out.push_str("  ");
                    out.push_str(&format_mop(op));
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
                let label = symbol_name(data_name_map, *id, ".Ldata");
                out.push_str(&format!("  .dword {}\n", label));
                written += 8;
            }
            BOperand::RoData(id) => {
                let label = symbol_name(rodata_name_map, *id, ".Lrodata");
                out.push_str(&format!("  .dword {}\n", label));
                written += 8;
            }
            BOperand::Func(id) => {
                let label = symbol_name(func_name_map, *id, ".Lfunc");
                out.push_str(&format!("  .dword {}\n", label));
                written += 8;
            }
            BOperand::Extern(name) => {
                out.push_str(&format!("  .extern {}\n", name));
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

fn format_mop(op: &BOp) -> String {
    match &op.data {
        BOpData::M(mop) => mop.to_string(),
        other => panic!(
            "dump_riscv_asm: expected BOpData::M for machine dump, got {:?}",
            other
        ),
    }
}
