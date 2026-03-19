//! Definition of registers for Lower IR (LIR).

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum XReg {
    Zero = 0, // hardwired zero
    Ra = 1,   // return address
    Sp = 2,   // stack pointer
    Gp = 3,   // global pointer
    Tp = 4,   // thread pointer
    T0 = 5,
    T1 = 6,
    T2 = 7, // temporaries
    S0 = 8, // fp
    S1 = 9, // saved registers / frame sc_var
    A0 = 10,
    A1 = 11,
    A2 = 12,
    A3 = 13,
    A4 = 14,
    A5 = 15,
    A6 = 16,
    A7 = 17, // FnDecl arguments / return values
    S2 = 18,
    S3 = 19,
    S4 = 20,
    S5 = 21,
    S6 = 22,
    S7 = 23,
    S8 = 24,
    S9 = 25,
    S10 = 26,
    S11 = 27, // saved registers
    T3 = 28,
    T4 = 29,
    T5 = 30,
    T6 = 31, // temporaries
}

impl XReg {
    #[inline(always)]
    pub fn is_temp(&self) -> bool {
        matches!(
            self,
            XReg::T0 | XReg::T1 | XReg::T2 | XReg::T3 | XReg::T4 | XReg::T5 | XReg::T6
        )
    }
    #[inline(always)]
    pub fn get_param_regs() -> Vec<XReg> {
        vec![
            XReg::A0,
            XReg::A1,
            XReg::A2,
            XReg::A3,
            XReg::A4,
            XReg::A5,
            XReg::A6,
            XReg::A7,
        ]
    }
}

impl std::fmt::Display for XReg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XReg::Zero => write!(f, "x0"),
            XReg::Ra => write!(f, "x1"),
            XReg::Sp => write!(f, "x2"),
            XReg::Gp => write!(f, "x3"),
            XReg::Tp => write!(f, "x4"),
            XReg::T0 => write!(f, "x5"),
            XReg::T1 => write!(f, "x6"),
            XReg::T2 => write!(f, "x7"),
            XReg::S0 => write!(f, "x8"),
            XReg::S1 => write!(f, "x9"),
            XReg::A0 => write!(f, "x10"),
            XReg::A1 => write!(f, "x11"),
            XReg::A2 => write!(f, "x12"),
            XReg::A3 => write!(f, "x13"),
            XReg::A4 => write!(f, "x14"),
            XReg::A5 => write!(f, "x15"),
            XReg::A6 => write!(f, "x16"),
            XReg::A7 => write!(f, "x17"),
            XReg::S2 => write!(f, "x18"),
            XReg::S3 => write!(f, "x19"),
            XReg::S4 => write!(f, "x20"),
            XReg::S5 => write!(f, "x21"),
            XReg::S6 => write!(f, "x22"),
            XReg::S7 => write!(f, "x23"),
            XReg::S8 => write!(f, "x24"),
            XReg::S9 => write!(f, "x25"),
            XReg::S10 => write!(f, "x26"),
            XReg::S11 => write!(f, "x27"),
            XReg::T3 => write!(f, "x28"),
            XReg::T4 => write!(f, "x29"),
            XReg::T5 => write!(f, "x30"),
            XReg::T6 => write!(f, "x31"),
        }
    }
}

impl From<XReg> for u8 {
    fn from(reg: XReg) -> Self {
        reg as u8
    }
}

/**
 * Float Point Register (D Extension)
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FReg {
    // ==========================================
    // Temporaries (Caller-Saved)
    // These registers are volatile across function calls.
    // ==========================================
    Ft0 = 0,
    Ft1 = 1,
    Ft2 = 2,
    Ft3 = 3,
    Ft4 = 4,
    Ft5 = 5,
    Ft6 = 6,
    Ft7 = 7,

    // ==========================================
    // Saved Registers (Callee-Saved)
    // The Callee MUST save and restore these if they are mutated.
    // ==========================================
    Fs0 = 8,
    Fs1 = 9,

    // ==========================================
    // Arguments / Return Values (Caller-Saved)
    // Used to pass the first 8 floating-point arguments.
    // `Fa0` and `Fa1` are additionally used for FP return values.
    // ==========================================
    Fa0 = 10,
    Fa1 = 11,
    Fa2 = 12,
    Fa3 = 13,
    Fa4 = 14,
    Fa5 = 15,
    Fa6 = 16,
    Fa7 = 17,

    // ==========================================
    // More Saved Registers (Callee-Saved)
    // ==========================================
    Fs2 = 18,
    Fs3 = 19,
    Fs4 = 20,
    Fs5 = 21,
    Fs6 = 22,
    Fs7 = 23,
    Fs8 = 24,
    Fs9 = 25,
    Fs10 = 26,
    Fs11 = 27,

    // ==========================================
    // More Temporaries (Caller-Saved)
    // ==========================================
    Ft8 = 28,
    Ft9 = 29,
    Ft10 = 30,
    Ft11 = 31,
}

impl std::fmt::Display for FReg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FReg::Ft0 => write!(f, "f0"),
            FReg::Ft1 => write!(f, "f1"),
            FReg::Ft2 => write!(f, "f2"),
            FReg::Ft3 => write!(f, "f3"),
            FReg::Ft4 => write!(f, "f4"),
            FReg::Ft5 => write!(f, "f5"),
            FReg::Ft6 => write!(f, "f6"),
            FReg::Ft7 => write!(f, "f7"),
            FReg::Fs0 => write!(f, "f8"),
            FReg::Fs1 => write!(f, "f9"),
            FReg::Fa0 => write!(f, "f10"),
            FReg::Fa1 => write!(f, "f11"),
            FReg::Fa2 => write!(f, "f12"),
            FReg::Fa3 => write!(f, "f13"),
            FReg::Fa4 => write!(f, "f14"),
            FReg::Fa5 => write!(f, "f15"),
            FReg::Fa6 => write!(f, "f16"),
            FReg::Fa7 => write!(f, "f17"),
            FReg::Fs2 => write!(f, "f18"),
            FReg::Fs3 => write!(f, "f19"),
            FReg::Fs4 => write!(f, "f20"),
            FReg::Fs5 => write!(f, "f21"),
            FReg::Fs6 => write!(f, "f22"),
            FReg::Fs7 => write!(f, "f23"),
            FReg::Fs8 => write!(f, "f24"),
            FReg::Fs9 => write!(f, "f25"),
            FReg::Fs10 => write!(f, "f26"),
            FReg::Fs11 => write!(f, "f27"),
            FReg::Ft8 => write!(f, "f28"),
            FReg::Ft9 => write!(f, "f29"),
            FReg::Ft10 => write!(f, "f30"),
            FReg::Ft11 => write!(f, "f31"),
        }
    }
}

impl FReg {
    /// Determines if the register is callee-saved.
    /// Critical for generating the function Prologue/Epilogue (stack spill/reload).
    #[inline(always)]
    pub fn is_callee_saved(self) -> bool {
        matches!(
            self,
            FReg::Fs0
                | FReg::Fs1
                | FReg::Fs2
                | FReg::Fs3
                | FReg::Fs4
                | FReg::Fs5
                | FReg::Fs6
                | FReg::Fs7
                | FReg::Fs8
                | FReg::Fs9
                | FReg::Fs10
                | FReg::Fs11
        )
    }
    #[inline(always)]
    pub fn get_param_regs() -> Vec<FReg> {
        vec![
            FReg::Fa0,
            FReg::Fa1,
            FReg::Fa2,
            FReg::Fa3,
            FReg::Fa4,
            FReg::Fa5,
            FReg::Fa6,
            FReg::Fa7,
        ]
    }
}

impl From<FReg> for u8 {
    fn from(reg: FReg) -> Self {
        reg as u8
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reg {
    Virt(usize),
    X(XReg),
    F(FReg),
}

impl std::fmt::Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reg::Virt(id) => write!(f, "v{id}"),
            Reg::X(xreg) => write!(f, "{xreg}"),
            Reg::F(freg) => write!(f, "{freg}"),
        }
    }
}
