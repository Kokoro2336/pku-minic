//! Configuration for the compiler.

use crate::ir::back::{FReg, VReg, XReg};

pub const STK_FRM_ALIGN: u32 = 16; // 16 bytes for minimum
pub const RISCV_BITS: u32 = 64;
pub const PARAM_REG_MAX_NUM: u32 = 8;
pub const INT_IMM_MAX: i32 = 2047;
pub const INT_IMM_MIN: i32 = -2048;

// Register configuration.
pub const RESERVED_REG: XReg = XReg::T6; // Reserved for spill code.
pub const CALLER_SAVED_XREGS: &[XReg] = &[
  XReg::T0,
  XReg::T1,
  XReg::T2,
  XReg::T3,
  XReg::T4,
  XReg::T5,
  // XReg::T6 is reserved for spill code.
  XReg::A0,
  XReg::A1,
  XReg::A2,
  XReg::A3,
  XReg::A4,
  XReg::A5,
  XReg::A6,
  XReg::A7,
  // Though XReg::Ra is caller-saved, we save it at the beginning of each function and restore it at the end.
  // So we treat it as callee-saved in register allocation.
];
pub const CALLEE_SAVED_XREGS: &[XReg] = &[
  XReg::S0,
  XReg::S1,
  XReg::S2,
  XReg::S3,
  XReg::S4,
  XReg::S5,
  XReg::S6,
  XReg::S7,
  XReg::S8,
  XReg::S9,
  XReg::S10,
  XReg::S11,
];
pub const CALLER_SAVED_FREGS: &[FReg] = &[
  FReg::Ft0,
  FReg::Ft1,
  FReg::Ft2,
  FReg::Ft3,
  FReg::Ft4,
  FReg::Ft5,
  FReg::Ft6,
  FReg::Ft7,
  FReg::Fa0,
  FReg::Fa1,
  FReg::Fa2,
  FReg::Fa3,
  FReg::Fa4,
  FReg::Fa5,
  FReg::Fa6,
  FReg::Fa7,
];
pub const CALLEE_SAVED_FREGS: &[FReg] = &[
  FReg::Fs0,
  FReg::Fs1,
  FReg::Fs2,
  FReg::Fs3,
  FReg::Fs4,
  FReg::Fs5,
  FReg::Fs6,
  FReg::Fs7,
  FReg::Fs8,
  FReg::Fs9,
  FReg::Fs10,
  FReg::Fs11,
];
pub const CALLEE_SAVED_VREGS: &[VReg] = &[];
pub const CALLER_SAVED_VREGS: &[VReg] = &[
  VReg::V0,
  VReg::V1,
  VReg::V2,
  VReg::V3,
  VReg::V4,
  VReg::V5,
  VReg::V6,
  VReg::V7,
  VReg::V8,
  VReg::V9,
  VReg::V10,
  VReg::V11,
  VReg::V12,
  VReg::V13,
  VReg::V14,
  VReg::V15,
  VReg::V16,
  VReg::V17,
  VReg::V18,
  VReg::V19,
  VReg::V20,
  VReg::V21,
  VReg::V22,
  VReg::V23,
  VReg::V24,
  VReg::V25,
  VReg::V26,
  VReg::V27,
  VReg::V28,
  VReg::V29,
  VReg::V30,
  VReg::V31,
];

pub const COLOR_XREGS: usize = CALLER_SAVED_XREGS.len() + CALLEE_SAVED_XREGS.len();
pub const COLOR_FREGS: usize = CALLER_SAVED_FREGS.len() + CALLEE_SAVED_FREGS.len();
pub const COLOR_VREGS: usize = CALLER_SAVED_VREGS.len() + CALLEE_SAVED_VREGS.len();
pub const REGS_NUM: usize = 96;
