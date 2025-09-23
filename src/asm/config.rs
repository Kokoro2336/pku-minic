pub enum RVOpCode {
    LI,
    RET,
}

impl std::fmt::Display for RVOpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RVOpCode::LI => write!(f, "li"),
            RVOpCode::RET => write!(f, "ret"),
        }
    }
}

pub enum RVReg {
    ZERO,   // hardwired zero
    RA,     // return address
    SP,     // stack pointer
    GP,     // global pointer
    TP,     // thread pointer
    T0, T1, T2,     // temporaries
    S0, S1,         // saved registers / frame pointer
    A0, A1, A2, A3, A4, A5, A6, A7, // function arguments / return values
    S2, S3, S4, S5, S6, S7, S8, S9, S10, S11,   // saved registers
    T3, T4, T5, T6, // temporaries
}

impl std::fmt::Display for RVReg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RVReg::ZERO => write!(f, "x0"),
            RVReg::RA => write!(f, "x1"),
            RVReg::SP => write!(f, "x2"),
            RVReg::GP => write!(f, "x3"),
            RVReg::TP => write!(f, "x4"),
            RVReg::T0 => write!(f, "x5"),
            RVReg::T1 => write!(f, "x6"),
            RVReg::T2 => write!(f, "x7"),
            RVReg::S0 => write!(f, "x8"),
            RVReg::S1 => write!(f, "x9"),
            RVReg::A0 => write!(f, "x10"),
            RVReg::A1 => write!(f, "x11"),
            RVReg::A2 => write!(f, "x12"),
            RVReg::A3 => write!(f, "x13"),
            RVReg::A4 => write!(f, "x14"),
            RVReg::A5 => write!(f, "x15"),
            RVReg::A6 => write!(f, "x16"),
            RVReg::A7 => write!(f, "x17"),
            RVReg::S2 => write!(f, "x18"),
            RVReg::S3 => write!(f, "x19"),
            RVReg::S4 => write!(f, "x20"),
            RVReg::S5 => write!(f, "x21"),
            RVReg::S6 => write!(f, "x22"),
            RVReg::S7 => write!(f, "x23"),
            RVReg::S8 => write!(f, "x24"),
            RVReg::S9 => write!(f, "x25"),
            RVReg::S10 => write!(f, "x26"),
            RVReg::S11 => write!(f, "x27"),
            RVReg::T3 => write!(f, "x28"),
            RVReg::T4 => write!(f, "x29"),
            RVReg::T5 => write!(f, "x30"),
            RVReg::T6 => write!(f, "x31"),
        }
    }
}
