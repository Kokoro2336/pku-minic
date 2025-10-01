use crate::asm::config::RVREG_ALLOCATOR;
use crate::asm::config::{RVOpCode, RVRegCode};
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{DataFlowGraph, Func, InstData, InstId, Operand, Program};
use std::cell::{Ref, RefMut};

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

        // TODO: for now, we don't need to process funct_type and params

        // add inst
        for basic_block in &func.basic_blocks {
            for inst in &basic_block.inst_list {
                let inst_data = {
                    let dfg = func.dfg.borrow();
                    dfg.get_inst(inst).unwrap().clone()
                };

                let asm_insts = {
                    let dfg = &mut func.dfg.borrow_mut();
                    AsmInst::from(dfg, inst, &inst_data)
                };

                asm_block.insts.extend(asm_insts);
            }
        }

        asm_block
    }
}

#[derive(Clone)]
pub struct AsmInst {
    pub opcode: RVOpCode,
    pub rd: Option<RVRegCode>,
    pub rs1: Option<RVRegCode>,
    pub rs2: Option<RVRegCode>,
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
    pub fn new() -> Self {
        Self {
            opcode: RVOpCode::LI,
            rd: None,
            rs1: None,
            rs2: None,
            imm: None,
        }
    }

    pub fn from(
        dfg: &mut RefMut<'_, DataFlowGraph>,
        inst: &u32,
        inst_data: &InstData,
    ) -> Vec<Self> {
        let mut v = Vec::new();

        let reg_used = match inst_data.opcode {
            KoopaOpCode::EQ | KoopaOpCode::NE => {
                let rs1 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(0).unwrap());
                let rs2 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(1).unwrap());

                let rd1 = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: RVOpCode::XOR, // for now we just use xor to represent eq
                    rd: rd1,
                    rs1: Some(rs1),
                    rs2: Some(rs2),
                    imm: None,
                });

                let rd2 = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: match inst_data.opcode {
                        KoopaOpCode::EQ => RVOpCode::SEQZ, // set if equal to zero
                        KoopaOpCode::NE => RVOpCode::SNEZ, // set if not equal to zero
                        _ => unreachable!(),
                    },
                    rd: rd2,
                    rs1: rd1,
                    rs2: None,
                    imm: None,
                });

                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs1);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs2);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd1.unwrap()); // free the temporary register
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd2.unwrap()); // free the temporary register
                rd2
            }

            KoopaOpCode::AND
            | KoopaOpCode::OR => {
                let rs1 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(0).unwrap());
                let rs2 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(1).unwrap());

                let rd1 = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: RVOpCode::SNEZ,
                    rd: rd1,
                    rs1: Some(rs1),
                    rs2: None,
                    imm: None,
                });

                let rd2 = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: RVOpCode::SNEZ,
                    rd: rd2,
                    rs1: Some(rs2),
                    rs2: None,
                    imm: None,
                });

                let rd = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: match inst_data.opcode {
                        KoopaOpCode::AND => RVOpCode::AND,
                        KoopaOpCode::OR => RVOpCode::OR,
                        _ => unreachable!(),
                    },
                    rd,
                    rs1: rd1,
                    rs2: rd2,
                    imm: None,
                });

                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs1);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs2);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd1.unwrap()); // free the temporary register
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd2.unwrap()); // free the temporary
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd.unwrap()); // free the temporary
                rd
            }

            KoopaOpCode::ADD
            | KoopaOpCode::SUB
            | KoopaOpCode::MUL
            | KoopaOpCode::DIV
            | KoopaOpCode::MOD

            // warning: SysY doesn't have bitwise and, or, xor,
            // but bitwise ops are the default ops in rv
            | KoopaOpCode::XOR 

            | KoopaOpCode::LT
            | KoopaOpCode::LE
            | KoopaOpCode::GT 
            | KoopaOpCode::GE 

            | KoopaOpCode::SAR
            | KoopaOpCode::SHL
            | KoopaOpCode::SHR => {
                let rs1 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(0).unwrap());
                let rs2 = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(1).unwrap());

                let rv_opcode = match inst_data.opcode {
                    KoopaOpCode::ADD => RVOpCode::ADD,
                    KoopaOpCode::SUB => RVOpCode::SUB,
                    KoopaOpCode::MUL => RVOpCode::MUL,
                    KoopaOpCode::DIV => RVOpCode::DIV,
                    KoopaOpCode::MOD => RVOpCode::REM,

                    KoopaOpCode::XOR => RVOpCode::XOR,

                    KoopaOpCode::LT => RVOpCode::SLT,
                    KoopaOpCode::LE => RVOpCode::SLT,
                    KoopaOpCode::GT => RVOpCode::SGT, 
                    KoopaOpCode::GE => RVOpCode::SGT, 

                    KoopaOpCode::SAR => RVOpCode::SRA,
                    KoopaOpCode::SHL => RVOpCode::SLL,
                    KoopaOpCode::SHR => RVOpCode::SRL,
                    _ => unreachable!(),
                };

                let rd = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst);
                v.push(AsmInst {
                    opcode: rv_opcode,
                    rd,
                    rs1: Some(rs1),
                    rs2: Some(rs2),
                    imm: None,
                });

                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs1);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rs2);
                RVREG_ALLOCATOR.lock().unwrap().free_reg(rd.unwrap()); // free the temporary register
                rd
            }

            KoopaOpCode::RET => {
                // if we need to load imm at return point, we must use a0 anyway.
                let op_reg = Self::process_op(dfg, &mut v, inst, inst_data.operands.get(0).unwrap());

                v.push(AsmInst {
                    opcode: RVOpCode::MV,
                    rd: Some(RVRegCode::A0),    // fixed to a0
                    rs1: Some(op_reg), // the imm must be loaded into t0
                    rs2: None,
                    imm: None,
                });

                v.push(AsmInst {
                    opcode: RVOpCode::RET,
                    rd: None,
                    rs1: None,
                    rs2: None,
                    imm: None,
                });

                RVREG_ALLOCATOR.lock().unwrap().free_reg(op_reg);
                None
            }
        };

        if let Some(reg) = reg_used {
            dfg.set_reg(inst, Some(reg));
            RVREG_ALLOCATOR.lock().unwrap().occupy_reg(reg, *inst); // we use 0 as a dummy inst id for
        }
        v
    }

    fn process_op(
        dfg: &mut RefMut<'_, DataFlowGraph>,
        v: &mut Vec<AsmInst>,
        inst_id: &u32,
        operand: &Operand,
    ) -> RVRegCode {
        match operand {
            Operand::Const(val) => {
                if *val == 0 {
                    return RVRegCode::ZERO;
                }

                // find a free temp reg
                if let Some(free_reg) = RVREG_ALLOCATOR.lock().unwrap().find_and_occupy_reg(*inst_id) {
                    // load imm into the free reg
                    let asm_inst = AsmInst {
                        opcode: RVOpCode::LI,
                        rd: Some(free_reg),
                        rs1: None,
                        rs2: None,
                        imm: Some(*val),
                    };
                    v.push(asm_inst);
                    free_reg
                } else {
                    unimplemented!()
                }
            }

            Operand::InstId(inst_id) => {
                let reg_used = {
                    let inst = dfg.get_inst(inst_id).unwrap();
                    inst.reg_used.unwrap()
                };

                // TODO: maybe we would check if reg_used would still be used by the original inst in the future
                dfg.set_reg_none(*inst_id);

                RVREG_ALLOCATOR.lock().unwrap().free_reg(reg_used);
                reg_used
            }
        }
    }
}
