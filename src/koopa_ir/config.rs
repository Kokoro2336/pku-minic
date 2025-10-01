use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    // mapping type of SysY to type of Koopa IR.
    pub static ref KOOPA_TYPE_MAP: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("int".to_string(), "i32".to_string());
        m
    };
}

#[derive(Debug, Clone)]
pub enum KoopaOpCode {
    NE, EQ, GT, LT, GE, LE,     // comparison
    ADD, SUB, MUL, DIV, MOD,    // arithmetic
    AND, OR, XOR,               // bitwise
    SHL, SHR, SAR,              // bitwise shift
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
            KoopaOpCode::RET => write!(f, "ret"),
        }
    }
}

impl KoopaOpCode {
    pub fn get_opcode(s: &str) -> Self {
        match s {
            "ne" => KoopaOpCode::NE,
            "eq" => KoopaOpCode::EQ,
            "gt" => KoopaOpCode::GT,
            "lt" => KoopaOpCode::LT,
            "ge" => KoopaOpCode::GE,
            "le" => KoopaOpCode::LE,
            "add" => KoopaOpCode::ADD,
            "sub" => KoopaOpCode::SUB,
            "mul" => KoopaOpCode::MUL,
            "div" => KoopaOpCode::DIV,
            "mod" => KoopaOpCode::MOD,
            "and" => KoopaOpCode::AND,
            "or" => KoopaOpCode::OR,
            "xor" => KoopaOpCode::XOR,
            "shl" => KoopaOpCode::SHL,
            "shr" => KoopaOpCode::SHR,
            "sar" => KoopaOpCode::SAR,
            "ret" => KoopaOpCode::RET,
            _ => unreachable!(),
        }
    }
}
