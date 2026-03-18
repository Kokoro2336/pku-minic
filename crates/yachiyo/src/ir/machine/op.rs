//! Definition of Lower IR (LIR) instructions.

use super::Reg;

/// Instruction definition of Lower IR.
#[derive(Debug, Clone)]
pub enum MOp {
    // ==========================================
    // 1. Pseudo-instructions & Data Movement
    // ==========================================
    /// Load Immediate: Materializes a 32-bit constant.
    Li { rd: MOperand, imm: MOperand },
    /// Load Address: Materializes the absolute address of a global variable or array.
    La { rd: MOperand, imm: MOperand },
    /// Move: Register-to-register copy.
    /// Crucial for Phi elimination and register spilling/reloading.
    Mv { rd: MOperand, rs: MOperand },
    /// FP Move (Single): Copy between floating-point registers.
    FmvS { rd: MOperand, rs: MOperand },

    // ==========================================
    // 2. Integer Arithmetic & Logic
    // CRITICAL for SysY: SysY 'int' is strictly 32-bit.
    // If your target is RV64, you MUST use the 'w' (word) suffix for ALU ops
    // to ensure proper sign-extension and prevent silent overflow bugs.
    // ==========================================
    Addw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Subw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Mulw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Divw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Remw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    }, // SysY +, -, *, /, % (32-bit math on 64-bit arch)
    Slliw {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    },
    Srliw {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    },
    Sraiw {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    }, // Shift by immediate (e.g., array index scaling: i * 4)
    Sllw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Srlw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Sraw {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    }, // Shift by register
    And {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Or {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    Xor {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    }, // Bitwise/Logical operations
    Andi {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    },
    Ori {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    },
    Xori {
        rd: MOperand,
        rs1: MOperand,
        imm: MOperand,
    }, // Bitwise/Logical with immediate

    // ==========================================
    // 3. Floating-Point Arithmetic (F-Extension)
    // ==========================================
    FaddS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    FsubS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    FmulS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    FdivS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    }, // Single-precision math
    FeqS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    FltS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    },
    FleS {
        rd: MOperand,
        rs1: MOperand,
        rs2: MOperand,
    }, // FP comparisons (==, <, <=)

    /// Float to Int conversion.
    /// Matches SysY semantic: truncate/round towards zero (RTZ).
    FcvtWS { rd: MOperand, rs: MOperand },
    /// Int to Float conversion.
    FcvtSW { rd: MOperand, rs: MOperand },

    /// Move bit-pattern from Integer to FP register.
    /// Required by RISC-V ABI when passing float args in integer registers.
    FmvWX { rd: MOperand, rs: MOperand },
    /// Move bit-pattern from FP to Integer register.
    FmvXW { rd: MOperand, rs: MOperand },

    // ==========================================
    // 4. Memory Access
    // ==========================================
    Lw {
        rd: MOperand,
        base: MOperand,
        offset: MOperand,
    },
    Sw {
        rs: MOperand,
        base: MOperand,
        offset: MOperand,
    }, // Load/Store 32-bit word (SysY int variable/array element)
    Flw {
        rd: MOperand,
        base: MOperand,
        offset: MOperand,
    },
    Fsw {
        rs: MOperand,
        base: MOperand,
        offset: MOperand,
    }, // Load/Store 32-bit float (SysY float variable/array element)

    /// Load/Store 64-bit doubleword.
    /// ONLY used for Pointers (e.g., array base addresses) or Stack Frame management in RV64.
    Ld {
        rd: MOperand,
        base: MOperand,
        offset: MOperand,
    },
    Sd {
        rs: MOperand,
        base: MOperand,
        offset: MOperand,
    },

    // ==========================================
    // 5. Control Flow
    // ==========================================
    /// Unconditional jump (translates 'break', 'continue', or block merges).
    J { offset: MOperand },
    /// Function call. Use this pseudo-instruction and let the assembler handle ra/auipc/jalr.
    Call { target: MOperand },
    /// Return. Pseudo for 'jalr x0, 0(ra)'.
    Ret,
    Beq {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    },
    Bne {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    }, // Branch if Equal / Not Equal
    Blt {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    },
    Bge {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    }, // Branch if Less Than / Greater or Equal (Signed - SysY default)
    Bltu {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    },
    Bgeu {
        rs1: MOperand,
        rs2: MOperand,
        offset: MOperand,
    }, // Branch Unsigned (Used strictly for pointer/address bound checks)
}

impl std::fmt::Display for MOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MOp::Li { rd, imm } => write!(f, "li {rd}, {imm}"),
            MOp::La { rd, imm } => write!(f, "la {rd}, {imm}"),
            MOp::Mv { rd, rs } => write!(f, "mv {rd}, {rs}"),
            MOp::FmvS { rd, rs } => write!(f, "fmv.s {rd}, {rs}"),

            MOp::Addw { rd, rs1, rs2 } => write!(f, "addw {rd}, {rs1}, {rs2}"),
            MOp::Subw { rd, rs1, rs2 } => write!(f, "subw {rd}, {rs1}, {rs2}"),
            MOp::Mulw { rd, rs1, rs2 } => write!(f, "mulw {rd}, {rs1}, {rs2}"),
            MOp::Divw { rd, rs1, rs2 } => write!(f, "divw {rd}, {rs1}, {rs2}"),
            MOp::Remw { rd, rs1, rs2 } => write!(f, "remw {rd}, {rs1}, {rs2}"),
            MOp::Slliw { rd, rs1, imm } => write!(f, "slliw {rd}, {rs1}, {imm}"),
            MOp::Srliw { rd, rs1, imm } => write!(f, "srliw {rd}, {rs1}, {imm}"),
            MOp::Sraiw { rd, rs1, imm } => write!(f, "sraiw {rd}, {rs1}, {imm}"),
            MOp::Sllw { rd, rs1, rs2 } => write!(f, "sllw {rd}, {rs1}, {rs2}"),
            MOp::Srlw { rd, rs1, rs2 } => write!(f, "srlw {rd}, {rs1}, {rs2}"),
            MOp::Sraw { rd, rs1, rs2 } => write!(f, "sraw {rd}, {rs1}, {rs2}"),
            MOp::And { rd, rs1, rs2 } => write!(f, "and {rd}, {rs1}, {rs2}"),
            MOp::Or { rd, rs1, rs2 } => write!(f, "or {rd}, {rs1}, {rs2}"),
            MOp::Xor { rd, rs1, rs2 } => write!(f, "xor {rd}, {rs1}, {rs2}"),
            MOp::Andi { rd, rs1, imm } => write!(f, "andi {rd}, {rs1}, {imm}"),
            MOp::Ori { rd, rs1, imm } => write!(f, "ori {rd}, {rs1}, {imm}"),
            MOp::Xori { rd, rs1, imm } => write!(f, "xori {rd}, {rs1}, {imm}"),

            MOp::FaddS { rd, rs1, rs2 } => write!(f, "fadd.s {rd}, {rs1}, {rs2}"),
            MOp::FsubS { rd, rs1, rs2 } => write!(f, "fsub.s {rd}, {rs1}, {rs2}"),
            MOp::FmulS { rd, rs1, rs2 } => write!(f, "fmul.s {rd}, {rs1}, {rs2}"),
            MOp::FdivS { rd, rs1, rs2 } => write!(f, "fdiv.s {rd}, {rs1}, {rs2}"),
            MOp::FeqS { rd, rs1, rs2 } => write!(f, "feq.s {rd}, {rs1}, {rs2}"),
            MOp::FltS { rd, rs1, rs2 } => write!(f, "flt.s {rd}, {rs1}, {rs2}"),
            MOp::FleS { rd, rs1, rs2 } => write!(f, "fle.s {rd}, {rs1}, {rs2}"),
            MOp::FcvtWS { rd, rs } => write!(f, "fcvt.w.s {rd}, {rs}"),
            MOp::FcvtSW { rd, rs } => write!(f, "fcvt.s.w {rd}, {rs}"),
            MOp::FmvWX { rd, rs } => write!(f, "fmv.w.x {rd}, {rs}"),
            MOp::FmvXW { rd, rs } => write!(f, "fmv.x.w {rd}, {rs}"),

            MOp::Lw { rd, base, offset } => write!(f, "lw {rd}, {offset}({base})"),
            MOp::Sw { rs, base, offset } => write!(f, "sw {rs}, {offset}({base})"),
            MOp::Flw { rd, base, offset } => write!(f, "flw {rd}, {offset}({base})"),
            MOp::Fsw { rs, base, offset } => write!(f, "fsw {rs}, {offset}({base})"),
            MOp::Ld { rd, base, offset } => write!(f, "ld {rd}, {offset}({base})"),
            MOp::Sd { rs, base, offset } => write!(f, "sd {rs}, {offset}({base})"),

            MOp::J { offset } => write!(f, "j {offset}"),
            MOp::Call { target } => write!(f, "call {target}"),
            MOp::Ret => write!(f, "ret"),
            MOp::Beq { rs1, rs2, offset } => write!(f, "beq {rs1}, {rs2}, {offset}"),
            MOp::Bne { rs1, rs2, offset } => write!(f, "bne {rs1}, {rs2}, {offset}"),
            MOp::Blt { rs1, rs2, offset } => write!(f, "blt {rs1}, {rs2}, {offset}"),
            MOp::Bge { rs1, rs2, offset } => write!(f, "bge {rs1}, {rs2}, {offset}"),
            MOp::Bltu { rs1, rs2, offset } => write!(f, "bltu {rs1}, {rs2}, {offset}"),
            MOp::Bgeu { rs1, rs2, offset } => write!(f, "bgeu {rs1}, {rs2}, {offset}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VirtReg {
    pub defs: Vec<MOperand>,
    pub uses: Vec<MOperand>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MOperand {
    Func(usize),
    BB(usize),
    Inst(usize),
    Reg(Reg),

    // Immediate
    IntImm(i32),
    FloatImm(f32),

    /// Id of frame slot
    Slot(usize),
    /// Id of .data arena.
    Data(usize),
    /// Id of .rodata arena.
    RoData(usize),

    Undef,
}

impl std::fmt::Display for MOperand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MOperand::Func(id) => write!(f, "fn.{id}"),
            MOperand::BB(id) => write!(f, "bb.{id}"),
            MOperand::Inst(id) => write!(f, "inst.{id}"),
            MOperand::Reg(reg) => write!(f, "{reg}"),
            MOperand::IntImm(imm) => write!(f, "{imm}"),
            MOperand::FloatImm(imm) => write!(f, "{imm}"),
            MOperand::Slot(id) => write!(f, "slot.{id}"),
            MOperand::Data(id) => write!(f, "data.{id}"),
            MOperand::RoData(id) => write!(f, "rodata.{id}"),
            MOperand::Undef => write!(f, "undef"),
        }
    }
}

#[allow(unused)]
impl MOperand {
    pub fn get_bb_id(&self) -> usize {
        match self {
            MOperand::BB(id) => *id,
            _ => panic!("Not a basic block operand"),
        }
    }
    pub fn get_inst_id(&self) -> usize {
        match self {
            MOperand::Inst(id) => *id,
            _ => panic!("Not an instruction operand"),
        }
    }
    pub fn get_virt_id(&self) -> usize {
        match self {
            MOperand::Reg(Reg::Virt(id)) => *id,
            _ => panic!("Not a virtual register operand"),
        }
    }
    pub fn get_func_id(&self) -> usize {
        match self {
            MOperand::Func(id) => *id,
            _ => panic!("Not a function operand"),
        }
    }
    pub fn hi(imm: i32) -> Self {
        MOperand::IntImm(imm >> 16)
    }
    pub fn lo(imm: i32) -> Self {
        MOperand::IntImm(imm & 0xFFFF)
    }
}
