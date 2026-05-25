//! Instruction definition of Machine IR.

use crate::ir::back::{BOpData, BOperand};

#[derive(Debug, Clone)]
pub enum MOpData {
  // Pseudo-instructions & Data Movement
  /// Load Immediate: Materializes a 32-bit constant.
  Li {
    rd: BOperand,
    imm: i32,
  },
  /// Load Address: Materializes the absolute address of a global variable or array.
  La {
    rd: BOperand,
    target: BOperand,
  },
  /// Move: Register-to-register copy.
  /// Crucial for Phi elimination and register spilling/reloading.
  Mv {
    rd: BOperand,
    rs: BOperand,
  },
  /// FP Move (Single): Copy between floating-point registers.
  FmvS {
    rd: BOperand,
    rs: BOperand,
  },

  // Integer Arithmetic & Logic
  // Register-Register ALU ops (32-bit)
  Add {
    rd: BOperand,
    rs1: BOperand,
    rs2: BOperand,
  },
  Sub {
    rd: BOperand,
    rs1: BOperand,
    rs2: BOperand,
  },
  Addi {
    rd: BOperand,
    rs1: BOperand,
    imm: BOperand,
  },
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
  },
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
  },

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
  },

  Xor {
    rd: BOperand,
    rs1: BOperand,
    rs2: BOperand,
  },
  Xori {
    rd: BOperand,
    rs1: BOperand,
    imm: BOperand,
  },

  And {
    rd: BOperand,
    rs1: BOperand,
    rs2: BOperand,
  },
  Andi {
    rd: BOperand,
    rs1: BOperand,
    imm: BOperand,
  },

  // Floating-Point Arithmetic (F-Extension)
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
  },

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

  /// Float to Int conversion.
  /// Matches SysY semantic: truncate/round towards zero (RTZ).
  FcvtWS {
    rd: BOperand,
    rs: BOperand,
  },
  /// Int to Float conversion.
  FcvtSW {
    rd: BOperand,
    rs: BOperand,
  },

  /// Move 64-bit bit-pattern from Integer to FP register.
  /// Required by RISC-V ABI when passing float args in integer registers.
  FmvDX {
    rd: BOperand,
    rs: BOperand,
  },
  /// Move 64-bit bit-pattern from FP to Integer register.
  FmvXD {
    rd: BOperand,
    rs: BOperand,
  },

  // Memory Access
  Lw {
    rd: BOperand,
    base: BOperand,
    offset: BOperand,
  },
  Sw {
    rs: BOperand,
    base: BOperand,
    offset: BOperand,
  },
  Flw {
    rd: BOperand,
    base: BOperand,
    offset: BOperand,
  },
  Fsw {
    rs: BOperand,
    base: BOperand,
    offset: BOperand,
  },

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
  Fld {
    rd: BOperand,
    base: BOperand,
    offset: BOperand,
  },
  Fsd {
    rs: BOperand,
    base: BOperand,
    offset: BOperand,
  },

  // ==========================================
  // 5. Control Flow
  // ==========================================
  /// Unconditional jump (translates 'break', 'continue', or block merges).
  J {
    target: BOperand,
  },
  /// Function call. Use this pseudo-instruction and let the assembler handle ra/auipc/jalr.
  Call {
    target: BOperand,
  },

  /// Return. Pseudo for 'jalr x0, 0(ra)'.
  Ret,

  /// Branching
  Bnez {
    rs: BOperand,
    target: BOperand,
  },
  Beqz {
    rs: BOperand,
    target: BOperand,
  },

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

impl MOpData {
  pub fn is_impure(&self) -> bool {
    matches!(
      self,
      MOpData::Sw { .. }
        | MOpData::Fsw { .. }
        | MOpData::Sd { .. }
        | MOpData::Fsd { .. }
        | MOpData::J { .. }
        | MOpData::Call { .. }
        | MOpData::Bnez { .. }
        | MOpData::Beqz { .. }
        | MOpData::Beq { .. }
        | MOpData::Bne { .. }
        | MOpData::Blt { .. }
        | MOpData::Bge { .. }
        | MOpData::Bltu { .. }
        | MOpData::Bgeu { .. }
        | MOpData::Ret
    )
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
