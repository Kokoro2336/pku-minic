//! Dump BackIR to RISC-V assembly files.

use crate::ir::back::BackIR;

use std::path::Path;

pub struct DumpASM<'a> {
    program: &'a BackIR,
    filename: String,
}

impl<'a> DumpASM<'a> {
    pub fn new(program: &'a BackIR, filename: String) -> Self {
        Self { program, filename }
    }

    pub fn run(&self) {
        let dump_dir = Path::new("dump_asm");
        let file_path = dump_dir.join(format!("{}.asm", self.filename));
        if let Err(e) = self.program.dump_riscv_asm_to_file(&file_path) {
            panic!("Error writing assembly dump: {}", e);
        }
    }
}

impl BackIR {
    pub fn dump_riscv_asm_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, self.dump_riscv_asm())
    }
}
