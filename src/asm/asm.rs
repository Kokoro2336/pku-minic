use crate::asm::config::{RVOpCode, RVRegCode, RVREG_ALLOCATOR, RegAllocType, STK_FRM_MANAGER};
use crate::koopa_ir::config::KoopaOpCode;
use crate::koopa_ir::koopa_ir::{Func, InstData, Operand, Program};
use crate::config::config::CONTEXT_STACK;

use std::rc::Rc;

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
            CONTEXT_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                stack.enter_func_scope(Rc::clone(&func));
            });

            asm.blocks.push(AsmBlock::from(&func));

            CONTEXT_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                stack.exit_scope();
            });
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

    pub fn from(func: &Func) -> Self {
        let mut asm_block = AsmBlock::new(func.name.clone());

        // TODO: for now, we don't need to process funct_type and params

        // add prologue
        asm_block.prologue(func);

        // add inst
        for ir_block in &func.ir_blocks {
            CONTEXT_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                stack.enter_block_scope(Rc::clone(ir_block));
            });

            for inst in CONTEXT_STACK.with(|stack| {
                let stack = stack.borrow();
                stack.get_current_inst_list().borrow().clone()
            }) {
                let inst_data = {
                    let dfg = CONTEXT_STACK.with(|stack| stack.borrow().get_current_dfg());
                    let dfg_borrow = dfg.borrow();
                    dfg_borrow.get_inst(&inst).unwrap().clone()
                };

                let asm_insts = {
                    AsmInst::from(&inst, &inst_data)
                };

                asm_block.insts.extend(asm_insts);
            }

            CONTEXT_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                stack.exit_scope();
            });
        }

        STK_FRM_MANAGER.with(|manager| {
            let mut manager = manager.borrow_mut();
            manager.epilogue();
        });

        asm_block
    }

    /// this function manage stack frame's layout, including:
    /// 1. whether to allocate return address
    /// 2. how much space to allocate for callee-saved registers
    /// 3. how much space to allocate for local variables
    /// 4. whether to allocate space for function calling.
    pub fn prologue(&mut self, func: &Func) {
        STK_FRM_MANAGER.with(|manager| {
            let mut manager = manager.borrow_mut();
            manager.prologue(func);
        });

        let mut asm_insts: Vec<AsmInst> = vec![];

        // allocate stack frame
        asm_insts.push(AsmInst {
            opcode: RVOpCode::ADDI,
            rd: Some(RegAllocType::Temp(RVRegCode::SP)),
            rs1: Some(RegAllocType::Temp(RVRegCode::SP)),
            rs2: None,
            imm: Some(-STK_FRM_MANAGER.with(|manager| manager.borrow().get_size() as i32)),
        });

        // store return address
        if STK_FRM_MANAGER.with(|manager| manager.borrow().is_callee()) {
            asm_insts.push(AsmInst {
                opcode: RVOpCode::SW,
                rd: None,
                rs1: Some(RegAllocType::MemWithReg {
                    reg: RVRegCode::SP,
                    offset: 0, // Default offset value added
                }),
                rs2: None,
                imm: None,
            });
        }

        self.insts.extend(asm_insts);
    }

}

#[derive(Clone)]
pub struct AsmInst {
    pub opcode: RVOpCode,
    pub rd: Option<RegAllocType>,
    pub rs1: Option<RegAllocType>,
    pub rs2: Option<RegAllocType>,
    pub imm: Option<i32>,
}

impl std::fmt::Display for AsmInst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.opcode)?;
        match self.opcode {
            RVOpCode::LW => {
                write!(f, " {}, {}", &self.rs1.as_ref().unwrap(), &self.rd.as_ref().unwrap())?;
            }

            RVOpCode::SW => {
                write!(f, " {}, {}", &self.rs2.as_ref().unwrap(), &self.rs1.as_ref().unwrap())?;
            }

            _ => {
                if self.rd.is_some() {
                    write!(f, " {}", &self.rd.as_ref().unwrap())?;
                }
                if self.rs1.is_some() {
                    write!(f, ", {}", &self.rs1.as_ref().unwrap())?;
                }
                if self.rs2.is_some() {
                    write!(f, ", {}", &self.rs2.as_ref().unwrap())?;
                }
                if self.imm.is_some() {
                    write!(f, ", {}", self.imm.as_ref().unwrap())?;
                }
            }
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
        inst: &u32,
        inst_data: &InstData,
    ) -> Vec<Self> {
        let mut v = Vec::new();
        let inst_list = CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            stack.get_current_inst_list().borrow().clone()
        });

        let reg_used: RegAllocType = match inst_data.opcode {
            KoopaOpCode::EQ | KoopaOpCode::NE => {
                let rs1 = process_op(&mut v, inst, inst_data.operands.first().unwrap());
                let rs2 = process_op(&mut v, inst, inst_data.operands.get(1).unwrap());

                // manually specify the type of anonymous var.
                let rd1 = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: RVOpCode::XOR, // for now we just use xor to represent eq
                    rd: Some(rd1.clone()),
                    rs1: Some(rs1.clone()),
                    rs2: Some(rs2.clone()),
                    imm: None,
                });

                let rd2 = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: match inst_data.opcode {
                        KoopaOpCode::EQ => RVOpCode::SEQZ, // set if equal to zero
                        KoopaOpCode::NE => RVOpCode::SNEZ, // set if not equal to zero
                        _ => unreachable!(),
                    },
                    rd: Some(rd2.clone()),
                    rs1: Some(rd1.clone()),
                    rs2: None,
                    imm: None,
                });

                rs1.free_temp(); rs2.free_temp(); rd1.free_temp(); rd2.free_temp();
                // save value to mem
                v.push(AsmInst {
                    opcode: RVOpCode::SW,
                    rd: None,
                    rs1: Some(STK_FRM_MANAGER.with(|manager| manager.borrow_mut().alloc_named_var_wrapped(inst_data.ir_obj.to_string(), inst_data.typ.clone()))),
                    rs2: Some(rd2.clone()),
                    imm: None,
                });
                // Some(rd2)
                RegAllocType::None
            }

            KoopaOpCode::AND
            | KoopaOpCode::OR => {
                let rs1 = process_op(&mut v, inst, inst_data.operands.first().unwrap());
                let rs2 = process_op(&mut v, inst, inst_data.operands.get(1).unwrap());

                let rd1 = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: RVOpCode::SNEZ,
                    rd: Some(rd1.clone()),
                    rs1: Some(rs1.clone()),
                    rs2: None,
                    imm: None,
                });

                let rd2 = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: RVOpCode::SNEZ,
                    rd: Some(rd2.clone()),
                    rs1: Some(rs2.clone()),
                    rs2: None,
                    imm: None,
                });

                let rd = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: match inst_data.opcode {
                        KoopaOpCode::AND => RVOpCode::AND,
                        KoopaOpCode::OR => RVOpCode::OR,
                        _ => unreachable!(),
                    },
                    rd: Some(rd.clone()),
                    rs1: Some(rd1.clone()),
                    rs2: Some(rd2.clone()),
                    imm: None,
                });

                rs1.free_temp(); rs2.free_temp(); rd1.free_temp(); rd2.free_temp(); rd.free_temp();
                v.push(AsmInst {
                    opcode: RVOpCode::SW,
                    rd: None,
                    rs1: Some(STK_FRM_MANAGER.with(|manager| manager.borrow_mut().alloc_named_var_wrapped(inst_data.ir_obj.to_string(), inst_data.typ.clone()))),
                    rs2: Some(rd.clone()),
                    imm: None,
                });
                // Some(rd)
                RegAllocType::None
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
                let rs1 = process_op(&mut v, inst, inst_data.operands.first().unwrap());
                let rs2 = process_op(&mut v, inst, inst_data.operands.get(1).unwrap());

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

                let rd = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));
                v.push(AsmInst {
                    opcode: rv_opcode,
                    rd: Some(rd.clone()),
                    rs1: Some(rs1.clone()),
                    rs2: Some(rs2.clone()),
                    imm: None,
                });

                rs1.free_temp(); rs2.free_temp(); rd.free_temp();
                v.push(AsmInst {
                    opcode: RVOpCode::SW,
                    rd: None,
                    rs1: Some(STK_FRM_MANAGER.with(|manager| manager.borrow_mut().alloc_named_var_wrapped(inst_data.ir_obj.to_string(), inst_data.typ.clone()))),
                    rs2: Some(rd.clone()),
                    imm: None,
                });
                // Some(rd)
                RegAllocType::None
            }

            KoopaOpCode::ALLOC => {
                // only alloc space for ALLOC inst
                STK_FRM_MANAGER.with(|manager| manager.borrow_mut().alloc_named_var_wrapped(inst_data.ir_obj.to_string(), inst_data.typ.clone()));
                RegAllocType::None
            }

            KoopaOpCode::LOAD => {
                let rd = process_op(&mut v, inst, inst_data.operands.first().unwrap());
                let rs1 = RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst));

                v.push(AsmInst {
                    opcode: RVOpCode::LW,
                    rd: Some(rd.clone()), // the rd must be SP, so we directly use ::Temp
                    rs1: Some(rs1.clone()),
                    rs2: None,
                    imm: None,
                });

                rs1.free_temp(); rd.free_temp();
                v.push (AsmInst {
                    opcode: RVOpCode::SW,
                    rd: None,
                    rs1: Some(STK_FRM_MANAGER.with(|manager| manager.borrow_mut().alloc_named_var_wrapped(inst_data.ir_obj.to_string(), inst_data.typ.clone()))),
                    rs2: Some(rs1.clone()),
                    imm: None,
                });
                RegAllocType::None
            }

            KoopaOpCode::STORE => {
                let rs1 = process_op(&mut v, inst, inst_data.operands.get(1).unwrap());    
                let rs2 = process_op(&mut v, inst, inst_data.operands.first().unwrap());

                v.push(AsmInst {
                    opcode: RVOpCode::SW,
                    rd: None,
                    rs1: Some(rs1.clone()),
                    rs2: Some(rs2.clone()),
                    imm: None,
                });

                rs1.free_temp(); rs2.free_temp();
                RegAllocType::None
            }

            KoopaOpCode::RET => {
                // if we need to load imm at return point, we must use a0 anyway.
                let op_reg = process_op(&mut v, inst, inst_data.operands.first().unwrap());

                // epilogue here
                v.push(AsmInst {
                    opcode: RVOpCode::ADDI,
                    rd: Some(RegAllocType::Temp(RVRegCode::SP)),
                    rs1: Some(RegAllocType::Temp(RVRegCode::SP)),
                    rs2: None,
                    imm: Some(STK_FRM_MANAGER.with(|manager| manager.borrow().get_size() as i32)),
                });

                v.push(AsmInst {
                    opcode: RVOpCode::RET,
                    rd: None,
                    rs1: None,
                    rs2: None,
                    imm: None,
                });

                op_reg.free_temp();
                RegAllocType::None
            }
        };

        // only permanently allocated regs are recorded in DFG
        if let RegAllocType::Perm(reg) = reg_used {
            CONTEXT_STACK.with(|stack| {
                let stack = stack.borrow();
                stack.get_current_dfg().borrow_mut().set_reg(inst, Some(reg));
            });
        }
        v
    }
}

fn process_op(
    v: &mut Vec<AsmInst>,
    current_inst_id: &u32,
    operand: &Operand,
) -> RegAllocType {
    let opcode = CONTEXT_STACK.with(|stack| 
        stack
        .borrow()
        .get_current_dfg()
        .borrow()
        .get_inst(current_inst_id)
        .unwrap()
        .opcode
        .clone());

    match operand {
        Operand::Const(val) => {
            if *val == 0 {
                return RegAllocType::Temp(RVRegCode::ZERO);
            }

            // find a free temp reg
            if let RegAllocType::Temp(temp_reg) = match opcode {
                KoopaOpCode::RET => RegAllocType::Temp(RVRegCode::A0), // for return, we must use a0
                _ => RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*current_inst_id))
            } {
                // load imm into the free reg
                let asm_inst = AsmInst {
                    opcode: RVOpCode::LI,
                    rd: Some(RegAllocType::Temp(temp_reg)),
                    rs1: None,
                    rs2: None,
                    imm: Some(*val),
                };

                v.push(asm_inst);
                RegAllocType::Temp(temp_reg)
            } else {
                unimplemented!()
            }
        }

        Operand::InstId(inst_id) => {
            let mem_with_reg = STK_FRM_MANAGER.with(|manager| manager.borrow().get_named_var_wrapped(Operand::InstId(*inst_id).to_string()));
            let rs1 = match opcode {
                KoopaOpCode::RET => RegAllocType::Temp(RVRegCode::A0), // for return, we must use a0
                _ => RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().find_and_occupy_temp_reg(*inst_id))
            };

            v.push(AsmInst {
                opcode: RVOpCode::LW,
                rd: Some(mem_with_reg.clone()), // the rd
                rs1: Some(rs1.clone()),
                rs2: None,
                imm: None,
            });

            rs1
        }

        Operand::Pointer(pointer_id) => {
            STK_FRM_MANAGER.with(|manager| manager.borrow().get_named_var_wrapped(Operand::Pointer(*pointer_id).to_string()))
        }

        Operand::BType(_) => {
            unreachable!()  // b_type as a operand would never reach here.
        }

        Operand::None => RegAllocType::None,
    }
}
