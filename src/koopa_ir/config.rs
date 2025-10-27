use std::cell::RefCell;

thread_local! {
    // initialize pointer id allocator
    pub static PTR_ID_ALLOCATOR: RefCell<PointerIdAllocator> = RefCell::new(PointerIdAllocator::new());
}

#[derive(Debug, Clone)]
pub enum KoopaOpCode {
    NE,
    EQ,
    GT,
    LT,
    GE,
    LE, // comparison
    ADD,
    SUB,
    MUL,
    DIV,
    MOD, // arithmetic
    AND,
    OR,
    XOR, // bitwise
    SHL,
    SHR,
    SAR, // bitwise shift
    STORE,
    LOAD,
    ALLOC, // store, load & ALLOC
    RET,
}

impl std::fmt::Display for KoopaOpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KoopaOpCode::NE => write!(f, "ne"),
            KoopaOpCode::EQ => write!(f, "eq"),
            KoopaOpCode::GT => write!(f, "gt"),
            KoopaOpCode::LT => write!(f, "lt"),
            KoopaOpCode::GE => write!(f, "ge"),
            KoopaOpCode::LE => write!(f, "le"),
            KoopaOpCode::ADD => write!(f, "add"),
            KoopaOpCode::SUB => write!(f, "sub"),
            KoopaOpCode::MUL => write!(f, "mul"),
            KoopaOpCode::DIV => write!(f, "div"),
            KoopaOpCode::MOD => write!(f, "mod"),
            KoopaOpCode::AND => write!(f, "and"),
            KoopaOpCode::OR => write!(f, "or"),
            KoopaOpCode::XOR => write!(f, "xor"),
            KoopaOpCode::SHL => write!(f, "shl"),
            KoopaOpCode::SHR => write!(f, "shr"),
            KoopaOpCode::SAR => write!(f, "sar"),
            KoopaOpCode::STORE => write!(f, "store"),
            KoopaOpCode::LOAD => write!(f, "load"),
            KoopaOpCode::ALLOC => write!(f, "alloc"),
            KoopaOpCode::RET => write!(f, "ret"),
        }
    }
}

impl KoopaOpCode {
    pub fn has_return_value(&self) -> bool {
        match self {
            // These opcodes produce a return value
            KoopaOpCode::NE
            | KoopaOpCode::EQ
            | KoopaOpCode::GT
            | KoopaOpCode::LT
            | KoopaOpCode::GE
            | KoopaOpCode::LE
            | KoopaOpCode::ADD
            | KoopaOpCode::SUB
            | KoopaOpCode::MUL
            | KoopaOpCode::DIV
            | KoopaOpCode::MOD
            | KoopaOpCode::AND
            | KoopaOpCode::OR
            | KoopaOpCode::XOR
            | KoopaOpCode::SHL
            | KoopaOpCode::SHR
            | KoopaOpCode::SAR 
            | KoopaOpCode::LOAD 
            | KoopaOpCode:: ALLOC => true,

            // These opcodes do not produce a return value
            KoopaOpCode::STORE | KoopaOpCode::RET => false,
        }
    }
}

#[derive(Debug)]
pub struct PointerIdAllocator {
    current_id: u32,
}

impl PointerIdAllocator {
    pub fn new() -> Self {
        PointerIdAllocator { current_id: 0 }
    }

    pub fn alloc(&mut self) -> u32 {
        let current_id = self.current_id;
        self.current_id += 1;
        current_id
    }
}
