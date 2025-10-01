use lazy_static::lazy_static;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub enum RVOpCode {
    BEQZ,
    BNEZ,
    J,
    CALL,
    RET,
    LW,
    SW,
    ADD,
    ADDI,
    SUB,
    SLT,
    SGT,
    SEQZ,
    SNEZ,
    XOR,
    XORI,
    OR,
    ORI,
    AND,
    ANDI,
    SLL,
    SRL,
    SRA,
    MUL,
    DIV,
    REM,
    LI,
    LA,
    MV,
}

impl std::fmt::Display for RVOpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RVOpCode::BEQZ => write!(f, "beqz"),
            RVOpCode::BNEZ => write!(f, "bnez"),
            RVOpCode::J => write!(f, "j"),
            RVOpCode::CALL => write!(f, "call"),
            RVOpCode::RET => write!(f, "ret"),
            RVOpCode::LW => write!(f, "lw"),
            RVOpCode::SW => write!(f, "sw"),
            RVOpCode::ADD => write!(f, "add"),
            RVOpCode::ADDI => write!(f, "addi"),
            RVOpCode::SUB => write!(f, "sub"),
            RVOpCode::SLT => write!(f, "slt"),
            RVOpCode::SGT => write!(f, "sgt"),
            RVOpCode::SEQZ => write!(f, "seqz"),
            RVOpCode::SNEZ => write!(f, "snez"),
            RVOpCode::XOR => write!(f, "xor"),
            RVOpCode::XORI => write!(f, "xori"),
            RVOpCode::OR => write!(f, "or"),
            RVOpCode::ORI => write!(f, "ori"),
            RVOpCode::AND => write!(f, "and"),
            RVOpCode::ANDI => write!(f, "andi"),
            RVOpCode::SLL => write!(f, "sll"),
            RVOpCode::SRL => write!(f, "srl"),
            RVOpCode::SRA => write!(f, "sra"),
            RVOpCode::MUL => write!(f, "mul"),
            RVOpCode::DIV => write!(f, "div"),
            RVOpCode::REM => write!(f, "rem"),
            RVOpCode::LI => write!(f, "li"),
            RVOpCode::LA => write!(f, "la"),
            RVOpCode::MV => write!(f, "mv"),
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RVRegCode {
    ZERO = 0, // hardwired zero
    RA = 1,   // return address
    SP = 2,   // stack pointer
    GP = 3,   // global pointer
    TP = 4,   // thread pointer
    T0 = 5,
    T1 = 6,
    T2 = 7, // temporaries
    S0 = 8,
    S1 = 9, // saved registers / frame pointer
    A0 = 10,
    A1 = 11,
    A2 = 12,
    A3 = 13,
    A4 = 14,
    A5 = 15,
    A6 = 16,
    A7 = 17, // function arguments / return values
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

pub struct RVRegAllocator {
    pub map: [u32; 32], // each item means the inst id that occupies this register.
}

impl RVRegAllocator {
    pub fn new() -> Self {
        Self {
            map: [std::u32::MAX; 32],
        }
    }

    pub fn get_reg_occupation(&self, reg: RVRegCode) -> u32 {
        self.map[reg as usize]
    }

    /// temp regs: t0-t6, a0-a7
    pub fn find_free_temp_reg(&self) -> Option<RVRegCode> {
        for (i, &inst_id) in self.map.iter().enumerate() {
            if inst_id == std::u32::MAX
                && (i >= RVRegCode::T0 as usize && i <= RVRegCode::T2 as usize
                    || i >= RVRegCode::T3 as usize && i <= RVRegCode::T6 as usize
                    || i >= RVRegCode::A0 as usize && i <= RVRegCode::A7 as usize)
            {
                return Some(unsafe { std::mem::transmute(i as u8) });
            }
        }
        // TODO: maybe we need to have mem allocation strategy in the future
        None
    }

    pub fn occupy_reg(&mut self, reg: RVRegCode, inst_id: u32) {
        self.map[reg as usize] = inst_id;
    }

    pub fn find_and_occupy_reg(&mut self, inst_id: u32) -> Option<RVRegCode> {
        if let Some(reg) = self.find_free_temp_reg() {
            self.occupy_reg(reg, inst_id);
            Some(reg)
        } else {
            None
        }
    }

    pub fn free_reg(&mut self, reg: RVRegCode) {
        self.map[reg as usize] = std::u32::MAX;
    }

    pub fn from_idx(idx: usize) -> RVRegCode {
        assert!(idx < 32);
        unsafe { std::mem::transmute::<u8, RVRegCode>(idx as u8) }
    }
}

lazy_static! {
    /// this array records the occupation status of each register.
    /// each item means the inst id that occupies this register.
    pub static ref RVREG_ALLOCATOR: Mutex<RVRegAllocator> = Mutex::new(RVRegAllocator::new());
}

impl std::fmt::Display for RVRegCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RVRegCode::ZERO => write!(f, "x0"),
            RVRegCode::RA => write!(f, "x1"),
            RVRegCode::SP => write!(f, "x2"),
            RVRegCode::GP => write!(f, "x3"),
            RVRegCode::TP => write!(f, "x4"),
            RVRegCode::T0 => write!(f, "x5"),
            RVRegCode::T1 => write!(f, "x6"),
            RVRegCode::T2 => write!(f, "x7"),
            RVRegCode::S0 => write!(f, "x8"),
            RVRegCode::S1 => write!(f, "x9"),
            RVRegCode::A0 => write!(f, "x10"),
            RVRegCode::A1 => write!(f, "x11"),
            RVRegCode::A2 => write!(f, "x12"),
            RVRegCode::A3 => write!(f, "x13"),
            RVRegCode::A4 => write!(f, "x14"),
            RVRegCode::A5 => write!(f, "x15"),
            RVRegCode::A6 => write!(f, "x16"),
            RVRegCode::A7 => write!(f, "x17"),
            RVRegCode::S2 => write!(f, "x18"),
            RVRegCode::S3 => write!(f, "x19"),
            RVRegCode::S4 => write!(f, "x20"),
            RVRegCode::S5 => write!(f, "x21"),
            RVRegCode::S6 => write!(f, "x22"),
            RVRegCode::S7 => write!(f, "x23"),
            RVRegCode::S8 => write!(f, "x24"),
            RVRegCode::S9 => write!(f, "x25"),
            RVRegCode::S10 => write!(f, "x26"),
            RVRegCode::S11 => write!(f, "x27"),
            RVRegCode::T3 => write!(f, "x28"),
            RVRegCode::T4 => write!(f, "x29"),
            RVRegCode::T5 => write!(f, "x30"),
            RVRegCode::T6 => write!(f, "x31"),
        }
    }
}

impl RVRegCode {
    /// Get numeric index (0..=31) for use as array index
    pub fn idx(self) -> usize {
        self as usize
    }
}
