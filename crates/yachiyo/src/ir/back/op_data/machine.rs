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

  // ==========================================
  // 6. Vector Instrucitons
  // ==========================================
  /// vsetvli rd, rs1, e32, m1, ta, ma
  VSetVLi {
    rd: BOperand,
    rs1: BOperand,
  },

  /// vadd.vv vd, vs2, vs1
  VAddVV {
    vd: BOperand,
    vs2: BOperand,
    vs1: BOperand,
  },

  /// vmul.vv vd, vs2, vs1
  VMulVV {
    vd: BOperand,
    vs2: BOperand,
    vs1: BOperand,
  },

  /// vfadd.vv vd, vs2, vs1
  VFAddVV {
    vd: BOperand,
    vs2: BOperand,
    vs1: BOperand,
  },

  /// vfmul.vv vd, vs2, vs1
  VFMulVV {
    vd: BOperand,
    vs2: BOperand,
    vs1: BOperand,
  },

  /// vmv.v.x vd, rs1
  VMvVX {
    vd: BOperand,
    rs1: BOperand,
  },

  /// Move between vector register
  VMvVV {
    rd: BOperand,
    rs: BOperand,
  },

  /// vfmv.v.f vd, fs1
  VFMvVF {
    vd: BOperand,
    fs1: BOperand,
  },

  /// vmul.vx vd, vs2, rs1
  VMulVX {
    vd: BOperand,
    vs2: BOperand,
    rs1: BOperand,
  },

  /// vadd.vx vd, vs2, rs1
  VAddVX {
    vd: BOperand,
    vs2: BOperand,
    rs1: BOperand,
  },

  /// vfmul.vf vd, vs2, fs1
  VFMulVF {
    vd: BOperand,
    vs2: BOperand,
    fs1: BOperand,
  },

  /// vfadd.vf vd, vs2, fs1
  VFAddVF {
    vd: BOperand,
    vs2: BOperand,
    fs1: BOperand,
  },

  /// vadd.vi vd, vs2, imm
  VAddVI {
    vd: BOperand,
    vs2: BOperand,
    imm: BOperand,
  },

  /// vle32.v vd, (base)
  VLe32V {
    vd: BOperand,
    base: BOperand,
    offset: BOperand,
  },

  /// vse32.v vs3, (base)
  VSe32V {
    vs3: BOperand,
    base: BOperand,
    offset: BOperand,
  },

  /// vmv.s.x vd, rs1
  VMvSX {
    vd: BOperand,
    rs1: BOperand,
  },

  /// vfmv.s.f vd, fs1
  VMvSF {
    vd: BOperand,
    fs1: BOperand,
  },

  /// vmv.x.s rd, vs2
  VMvXS {
    rd: BOperand,
    vs2: BOperand,
  },

  /// vfmv.f.s fd, vs2
  VMvFS {
    fd: BOperand,
    vs2: BOperand,
  },

  /// vredsum.vs vd, vs2, vs1
  VRedSumVS {
    vd: BOperand,
    vs2: BOperand,
    vs1: BOperand,
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
        | MOpData::VSetVLi { .. }
        | MOpData::VSe32V { .. }
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
