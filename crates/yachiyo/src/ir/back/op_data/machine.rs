//! Instruction definition of Lower IR.

use crate::ir::back::{BOpData, BOperand};

#[derive(Debug, Clone)]
pub enum MOpData {
    // ==========================================
    // 1. Pseudo-instructions & Data Movement
    // ==========================================
    /// Load Immediate: Materializes a 32-bit constant.
    Li { rd: BOperand, imm: i32 },
    /// Load Address: Materializes the absolute address of a global variable or array.
    La { rd: BOperand, target: BOperand },
    /// Move: Register-to-register copy.
    /// Crucial for Phi elimination and register spilling/reloading.
    Mv { rd: BOperand, rs: BOperand },
    /// FP Move (Single): Copy between floating-point registers.
    FmvS { rd: BOperand, rs: BOperand },

    // ==========================================
    // 2. Integer Arithmetic & Logic
    // CRITICAL for SysY: SysY 'int' is strictly 32-bit.
    // If your target is RV64, you MUST use the 'w' (word) suffix for ALU ops
    // to ensure proper sign-extension and prevent silent overflow bugs.
    // ==========================================

    // Register-Register ALU ops (32-bit)
    Addw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Subw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Mulw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Divw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Remw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    }, // SysY +, -, *, /, % (32-bit math on 64-bit arch)
    Sllw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Srlw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Sraw {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    }, // Shift by register

    /// For relational ops of integer.
    Slt {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Slti {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Sltu {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    Sltiu {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },

    // Immediate ALU ops (32-bit)
    Addiw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Subiw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Muliw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Diviw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Remiw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Slliw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Srliw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },
    Sraiw {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    }, // Shift by immediate (e.g., array index scaling: i * 4)

    Xor {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    }, // Bitwise/Logical operations
    Xori {
        rd: BOperand,
        rs1: BOperand,
        imm: BOperand,
    },

    // ==========================================
    // 3. Floating-Point Arithmetic (F-Extension)
    // ==========================================
    FaddS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FsubS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FmulS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FdivS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    }, // Single-precision math

    // Relational ops
    FeqS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FltS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FleS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },

    // Pesudo relational ops
    FneS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FgtS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },
    FgeS {
        rd: BOperand,
        rs1: BOperand,
        rs2: BOperand,
    },

    /// Float to Int conversion.
    /// Matches SysY semantic: truncate/round towards zero (RTZ).
    FcvtWS { rd: BOperand, rs: BOperand },
    /// Int to Float conversion.
    FcvtSW { rd: BOperand, rs: BOperand },

    /// Move bit-pattern from Integer to FP register.
    /// Required by RISC-V ABI when passing float args in integer registers.
    FmvWX { rd: BOperand, rs: BOperand },
    /// Move bit-pattern from FP to Integer register.
    FmvXW { rd: BOperand, rs: BOperand },

    // ==========================================
    // 4. Memory Access
    // ==========================================
    Lw {
        rd: BOperand,
        base: BOperand,
        offset: BOperand,
    },
    Sw {
        rs: BOperand,
        base: BOperand,
        offset: BOperand,
    }, // Load/Store 32-bit word (SysY int variable/array element)
    Flw {
        rd: BOperand,
        base: BOperand,
        offset: BOperand,
    },
    Fsw {
        rs: BOperand,
        base: BOperand,
        offset: BOperand,
    }, // Load/Store 32-bit float (SysY float variable/array element)

    /// Load/Store 64-bit doubleword.
    /// ONLY used for Pointers (e.g., array base addresses) or Stack Frame management in RV64.
    Ld {
        rd: BOperand,
        base: BOperand,
        offset: BOperand,
    },
    Sd {
        rs: BOperand,
        base: BOperand,
        offset: BOperand,
    },

    // ==========================================
    // 5. Control Flow
    // ==========================================
    /// Unconditional jump (translates 'break', 'continue', or block merges).
    J { target: BOperand },
    /// Function call. Use this pseudo-instruction and let the assembler handle ra/auipc/jalr.
    Call { target: BOperand },

    /// Return. Pseudo for 'jalr x0, 0(ra)'.
    Ret,

    /// Branching
    Bnez { rs: BOperand, target: BOperand },

    Beq {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
    Bne {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
    Blt {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
    Bge {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
    Bltu {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
    Bgeu {
        rs1: BOperand,
        rs2: BOperand,
        offset: BOperand,
    },
}

impl std::fmt::Display for MOpData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MOpData::Li { rd, imm } => write!(f, "li {rd}, {imm}"),
            MOpData::La { rd, target } => write!(f, "la {rd}, {target}"),
            MOpData::Mv { rd, rs } => write!(f, "mv {rd}, {rs}"),
            MOpData::FmvS { rd, rs } => write!(f, "fmv.s {rd}, {rs}"),

            MOpData::Addw { rd, rs1, rs2 } => write!(f, "addw {rd}, {rs1}, {rs2}"),
            MOpData::Subw { rd, rs1, rs2 } => write!(f, "subw {rd}, {rs1}, {rs2}"),
            MOpData::Mulw { rd, rs1, rs2 } => write!(f, "mulw {rd}, {rs1}, {rs2}"),
            MOpData::Divw { rd, rs1, rs2 } => write!(f, "divw {rd}, {rs1}, {rs2}"),
            MOpData::Remw { rd, rs1, rs2 } => write!(f, "remw {rd}, {rs1}, {rs2}"),
            MOpData::Addiw { rd, rs1, imm } => write!(f, "addiw {rd}, {rs1}, {imm}"),
            MOpData::Subiw { rd, rs1, imm } => write!(f, "subiw {rd}, {rs1}, {imm}"),
            MOpData::Muliw { rd, rs1, imm } => write!(f, "muliw {rd}, {rs1}, {imm}"),
            MOpData::Diviw { rd, rs1, imm } => write!(f, "diviw {rd}, {rs1}, {imm}"),
            MOpData::Remiw { rd, rs1, imm } => write!(f, "remiw {rd}, {rs1}, {imm}"),
            MOpData::Slliw { rd, rs1, imm } => write!(f, "slliw {rd}, {rs1}, {imm}"),
            MOpData::Srliw { rd, rs1, imm } => write!(f, "srliw {rd}, {rs1}, {imm}"),
            MOpData::Sraiw { rd, rs1, imm } => write!(f, "sraiw {rd}, {rs1}, {imm}"),
            MOpData::Sllw { rd, rs1, rs2 } => write!(f, "sllw {rd}, {rs1}, {rs2}"),
            MOpData::Srlw { rd, rs1, rs2 } => write!(f, "srlw {rd}, {rs1}, {rs2}"),
            MOpData::Sraw { rd, rs1, rs2 } => write!(f, "sraw {rd}, {rs1}, {rs2}"),
            MOpData::Slt { rd, rs1, rs2 } => write!(f, "slt {rd}, {rs1}, {rs2}"),
            MOpData::Slti { rd, rs1, imm } => write!(f, "slti {rd}, {rs1}, {imm}"),
            MOpData::Sltu { rd, rs1, rs2 } => write!(f, "sltu {rd}, {rs1}, {rs2}"),
            MOpData::Sltiu { rd, rs1, imm } => write!(f, "sltiu {rd}, {rs1}, {imm}"),
            MOpData::Xor { rd, rs1, rs2 } => write!(f, "xor {rd}, {rs1}, {rs2}"),
            MOpData::Xori { rd, rs1, imm } => write!(f, "xori {rd}, {rs1}, {imm}"),

            MOpData::FaddS { rd, rs1, rs2 } => write!(f, "fadd.s {rd}, {rs1}, {rs2}"),
            MOpData::FsubS { rd, rs1, rs2 } => write!(f, "fsub.s {rd}, {rs1}, {rs2}"),
            MOpData::FmulS { rd, rs1, rs2 } => write!(f, "fmul.s {rd}, {rs1}, {rs2}"),
            MOpData::FdivS { rd, rs1, rs2 } => write!(f, "fdiv.s {rd}, {rs1}, {rs2}"),
            MOpData::FeqS { rd, rs1, rs2 } => write!(f, "feq.s {rd}, {rs1}, {rs2}"),
            MOpData::FltS { rd, rs1, rs2 } => write!(f, "flt.s {rd}, {rs1}, {rs2}"),
            MOpData::FleS { rd, rs1, rs2 } => write!(f, "fle.s {rd}, {rs1}, {rs2}"),
            MOpData::FneS { rd, rs1, rs2 } => write!(f, "fne.s {rd}, {rs1}, {rs2}"),
            MOpData::FgtS { rd, rs1, rs2 } => write!(f, "fgt.s {rd}, {rs1}, {rs2}"),
            MOpData::FgeS { rd, rs1, rs2 } => write!(f, "fge.s {rd}, {rs1}, {rs2}"),
            MOpData::FcvtWS { rd, rs } => write!(f, "fcvt.w.s {rd}, {rs}"),
            MOpData::FcvtSW { rd, rs } => write!(f, "fcvt.s.w {rd}, {rs}"),
            MOpData::FmvWX { rd, rs } => write!(f, "fmv.w.x {rd}, {rs}"),
            MOpData::FmvXW { rd, rs } => write!(f, "fmv.x.w {rd}, {rs}"),

            MOpData::Lw { rd, base, offset } => write!(f, "lw {rd}, {offset}({base})"),
            MOpData::Sw { rs, base, offset } => write!(f, "sw {rs}, {offset}({base})"),
            MOpData::Flw { rd, base, offset } => write!(f, "flw {rd}, {offset}({base})"),
            MOpData::Fsw { rs, base, offset } => write!(f, "fsw {rs}, {offset}({base})"),
            MOpData::Ld { rd, base, offset } => write!(f, "ld {rd}, {offset}({base})"),
            MOpData::Sd { rs, base, offset } => write!(f, "sd {rs}, {offset}({base})"),

            MOpData::J { target } => write!(f, "j {target}"),
            MOpData::Bnez { rs, target } => write!(f, "bnez {rs}, {target}"),
            MOpData::Call { target } => write!(f, "call {target}"),
            MOpData::Ret => write!(f, "ret"),
            MOpData::Beq { rs1, rs2, offset } => write!(f, "beq {rs1}, {rs2}, {offset}"),
            MOpData::Bne { rs1, rs2, offset } => write!(f, "bne {rs1}, {rs2}, {offset}"),
            MOpData::Blt { rs1, rs2, offset } => write!(f, "blt {rs1}, {rs2}, {offset}"),
            MOpData::Bge { rs1, rs2, offset } => write!(f, "bge {rs1}, {rs2}, {offset}"),
            MOpData::Bltu { rs1, rs2, offset } => write!(f, "bltu {rs1}, {rs2}, {offset}"),
            MOpData::Bgeu { rs1, rs2, offset } => write!(f, "bgeu {rs1}, {rs2}, {offset}"),
        }
    }
}

impl From<MOpData> for BOpData {
    fn from(op_data: MOpData) -> Self {
        BOpData::M(op_data)
    }
}

impl From<BOpData> for MOpData {
    fn from(op_data: BOpData) -> Self {
        match op_data {
            BOpData::M(m_op_data) => m_op_data,
            _ => panic!("Cannot convert LOpData to MOpData"),
        }
    }
}
