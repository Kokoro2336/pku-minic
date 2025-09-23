use crate::asm::config::{RVOpCode, RVReg};
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{Func, InstData, Operand, Program};
use std::cell::Ref;

pub struct Asm {
    pub global_vals: Vec<AsmGlobalVal>,
    pub blocks: Vec<AsmBlock>,
}

impl std::fmt::Display for Asm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // data section
        writeln!(f, ".data")?;
        for val in &self.global_vals {
            writeln!(f, ".global {}", val)?;
        }

        for val in &self.global_vals {
            writeln!(f, "{}", val)?;
        }

        // text section
        writeln!(f, ".text")?;
        // print the global symbols
        for block in &self.blocks {
            writeln!(f, ".global {}", block.label)?;
        }

        // print the blocks
        for block in &self.blocks {
            writeln!(f, "{}", block)?;
        }
        Ok(())
    }
}

impl Asm {
    pub fn new() -> Self {
        Self {
            global_vals: Vec::new(),
            blocks: Vec::new(),
        }
    }

    pub fn from(program: &Program) -> Result<Self, Box<dyn std::error::Error>> {
        let mut asm = Asm::new();

        // add global_syms
        for val in &program.global_vals {
            asm.global_vals
                .push(AsmGlobalVal::new(val.name.clone(), val.val));
        }

        // add blocks
        for func in &program.funcs {
            asm.blocks.push(AsmBlock::from(func.borrow()));
        }

        Ok(asm)
    }
}

pub struct AsmGlobalVal {
    pub name: String,
    pub val: i32,
}

impl std::fmt::Display for AsmGlobalVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}:", self.name)?;
        writeln!(f, "    .word {}", self.val)?;
        Ok(())
    }
}

impl AsmGlobalVal {
    pub fn new(name: String, val: i32) -> Self {
        Self { name, val }
    }
}

pub struct AsmBlock {
    pub label: String,
    pub insts: Vec<AsmInst>,
}

impl std::fmt::Display for AsmBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}:", self.label)?;
        for inst in &self.insts {
            write!(f, "    {}", inst)?;
        }
        Ok(())
    }
}

impl AsmBlock {
    pub fn new(label: String) -> Self {
        Self {
            label,
            insts: Vec::new(),
        }
    }

    pub fn from(func: Ref<'_, Func>) -> Self {
        let mut asm_block = AsmBlock::new(func.name.clone());

        // for now, we don't need to process funct_type and params

        // add inst
        for basic_block in &func.basic_blocks {
            for inst in &basic_block.inst_list {
                let inst_data = basic_block.get_inst(inst).unwrap();
                let asm_insts = AsmInst::from(&inst_data);
                asm_block.insts.extend(asm_insts);
            }
        }

        asm_block
    }
}

pub struct AsmInst {
    pub opcode: RVOpCode,
    pub rd: Option<RVReg>,
    pub rs1: Option<RVReg>,
    pub rs2: Option<RVReg>,
    pub imm: Option<i32>,
}

impl std::fmt::Display for AsmInst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.opcode)?;
        if let Some(rd) = &self.rd {
            write!(f, " {}", rd)?;
        }
        if let Some(rs1) = &self.rs1 {
            write!(f, ", {}", rs1)?;
        }
        if let Some(rs2) = &self.rs2 {
            write!(f, ", {}", rs2)?;
        }
        if let Some(imm) = &self.imm {
            write!(f, ", {}", imm)?;
        }
        writeln!(f, "")?;
        Ok(())
    }
}

impl AsmInst {
    pub fn from(inst_data: &InstData) -> Vec<Self> {
        let mut v = vec![];

        match inst_data.opcode {
            KoopaOpCode::RET => {
                let li_inst = AsmInst {
                    opcode: RVOpCode::LI,
                    rd: Some(RVReg::A0),
                    rs1: None,
                    rs2: None,
                    imm: {
                        if let Some(operand) = inst_data.operands.get(0) {
                            match operand {
                                Operand::Const(val) => Some(*val as i32), // Extract the constant value and cast it to i32
                                _ => None, // Handle other cases (e.g., Operand::InstId)
                            }
                        } else {
                            None
                        }
                    },
                };
                v.push(li_inst);

                let rv_inst = AsmInst {
                    opcode: RVOpCode::RET,
                    rd: None,
                    rs1: None,
                    rs2: None,
                    imm: None,
                };
                v.push(rv_inst);
            }
        }
        v
    }
}
