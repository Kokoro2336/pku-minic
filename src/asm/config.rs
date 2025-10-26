use crate::config::config::BType;
use crate::koopa_ir::koopa_ir::Func;

use std::cell::RefCell;
use std::collections::HashMap;

const STK_FRM_BASE_LENGTH: u32 = 16; // 16 bytes for minimum
const RISCV_BITS: u32 = 32;
const REG_IDLE: u32 = u32::MAX;

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

#[derive(Debug, Clone)]
pub enum RegAllocType {
    Temp(RVRegCode), // reg temporarily allocated, often for asms transformed from the same IR.
    Perm(RVRegCode), // reg permanently allocated, often for the reg eventually used by the whole IR.

    MemWithReg { offset: u32, reg: RVRegCode }, // memory location in stack frame
    None,                                       // no reg allocated
}

impl std::fmt::Display for RegAllocType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegAllocType::Temp(reg) => write!(f, "{}", reg),
            RegAllocType::Perm(reg) => write!(f, "{}", reg),
            RegAllocType::MemWithReg { offset, reg } => {
                write!(f, "{}({})", offset, reg)
            }
            RegAllocType::None => write!(f, ""),
        }
    }
}

impl RegAllocType {
    pub fn get_reg(&self) -> RVRegCode {
        match self {
            RegAllocType::Temp(reg) => *reg,
            RegAllocType::Perm(reg) => *reg,
            RegAllocType::MemWithReg { reg, .. } => *reg,
            RegAllocType::None => panic!("No register allocated!"),
        }
    }

    pub fn get_offset(&self) -> u32 {
        match self {
            RegAllocType::MemWithReg { offset, .. } => *offset,
            _ => panic!("Not a MemWithReg type!"),
        }
    }

    /// this function would only free temporary registers.
    pub fn free_temp(&self) {
        if let RegAllocType::Temp(reg) = self {
            RVREG_ALLOCATOR.with(|allocator| allocator.borrow_mut().free_reg(*reg));
        }
    }
}

pub struct RVRegAllocator {
    pub map: [u32; 32], // each item means the inst id that occupies this register.
}

impl RVRegAllocator {
    pub fn new() -> Self {
        Self {
            map: [REG_IDLE; 32],
        }
    }

    pub fn get_reg_occupation(&self, reg: RVRegCode) -> u32 {
        self.map[reg as usize]
    }

    /// temp regs: t0-t6, a0-a7
    pub fn find_free_reg(&self) -> Option<RVRegCode> {
        for (i, &inst_id) in self.map.iter().enumerate() {
            if inst_id == REG_IDLE
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

    pub fn find_and_occupy_temp_reg(&mut self, inst_id: u32) -> RegAllocType {
        if let Some(reg) = self.find_free_reg() {
            self.occupy_reg(reg, inst_id);
            RegAllocType::Temp(reg)
        } else {
            RegAllocType::None
        }
    }

    pub fn find_and_occupy_perm_reg(&mut self, inst_id: u32) -> RegAllocType {
        if let Some(reg) = self.find_free_reg() {
            self.occupy_reg(reg, inst_id);
            RegAllocType::Perm(reg)
        } else {
            RegAllocType::None
        }
    }

    pub fn free_reg(&mut self, reg: RVRegCode) {
        self.map[reg as usize] = REG_IDLE;
    }

    pub fn from_idx(idx: usize) -> RVRegCode {
        assert!(idx < 32);
        unsafe { std::mem::transmute::<u8, RVRegCode>(idx as u8) }
    }
}

#[derive(Debug, Clone)]
/// sp, fp, size of frame
pub struct StackFrame {
    // current sp's offset relative to initiative sp
    sp_offset: u32,
    // current fp's offset relative to initiative fp
    fp_offset: u32,
    // total size of stack frame
    size: u32,
    // offset relative to current sp.
    cur_offset: u32,
    // variable map in this stack frame
    var_map: HashMap<String, (u32, u32)>, // variable name to (offset, size) in stack frame
}

#[derive(Debug)]
pub struct StackFrameManager {
    frames: Vec<StackFrame>,
}

impl StackFrameManager {
    pub fn new() -> Self {
        Self { frames: vec![] }
    }

    pub fn prologue(&mut self, func: &Func) {
        // iterate inst_map, calculate stack frame size. as koopa ir is in SSA form,
        // each inst is only assigned once, so we can simply sum up the size of all as long as the inst have return value.
        let dfg = func.dfg.borrow();
        let origin_size = dfg
            .inst_map
            .iter()
            .fold(0, |acc, (_, inst)| acc + inst.typ.size_in_bytes());

        // actually manager doesn't know the initial value of sp and fp,
        // so we use positive offset to represent them for convenience.
        let size =
            ((origin_size + (STK_FRM_BASE_LENGTH - 1)) / STK_FRM_BASE_LENGTH) * STK_FRM_BASE_LENGTH;

        let (sp_offset, fp_offset) = if let Some(stack_frame) = self.frames.last() {
            (stack_frame.sp_offset + size, stack_frame.sp_offset)
        } else {
            (0, 0)
        };

        self.frames.push(StackFrame {
            sp_offset,
            fp_offset,
            size,
            cur_offset: 0,
            var_map: HashMap::new(),
        });
    }

    pub fn get_sp_offset(&self) -> u32 {
        self.frames.last().map_or(0, |frame| frame.sp_offset)
    }

    pub fn get_fp_offset(&self) -> u32 {
        self.frames.last().map_or(0, |frame| frame.fp_offset)
    }

    pub fn get_size(&self) -> u32 {
        self.frames.last().map_or(0, |frame| frame.size)
    }

    pub fn epilogue(&mut self) {
        self.frames.pop();
    }

    pub fn is_callee(&self) -> bool {
        self.frames.len() > 1
    }

    // @return (offset, size)
    pub fn alloc_var(&mut self, name: String, typ: BType) -> (u32, u32) {
        let frame = self.frames.last_mut().unwrap();

        let offset = frame.cur_offset;
        let size = match typ {
            BType::Int => 4,
            BType::Void => 0,
        };

        frame.var_map.insert(name, (offset, size));
        frame.cur_offset += size;

        (offset, size)
    }

    /// this would return RegAllocType with eventual offset in stack frame
    pub fn alloc_named_var_wrapped(&mut self, name: String, typ: BType) -> RegAllocType {
        let (offset, _) = self.alloc_var(name, typ);
        RegAllocType::MemWithReg {
            offset,
            reg: RVRegCode::SP,
        }
    }

    /// often for temporary variables without name
    pub fn alloc_anonymous_var_wrapped(&mut self, typ: BType) -> RegAllocType {
        let (offset, _) = self.alloc_var("".to_string(), typ);
        RegAllocType::MemWithReg {
            offset,
            reg: RVRegCode::SP,
        }
    }

    pub fn get_named_var_wrapped(&self, name: String) -> RegAllocType {
        let frame = self.frames.last().unwrap();
        let (offset, _) = frame.var_map.get(&name).cloned().unwrap();

        RegAllocType::MemWithReg {
            offset,
            reg: RVRegCode::SP,
        }
    }
}

thread_local! {
    /// this array records the occupation status of each register.
    /// each item means the inst id that occupies this register.

    pub static RVREG_ALLOCATOR: RefCell<RVRegAllocator> = RefCell::new(RVRegAllocator::new());
    pub static STK_FRM_MANAGER: RefCell<StackFrameManager> = RefCell::new(StackFrameManager::new());
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
