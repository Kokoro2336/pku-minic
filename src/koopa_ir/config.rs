
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
    RET,
}

impl std::fmt::Display for KoopaOpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KoopaOpCode::RET => write!(f, "ret"),
        }
    }
}

impl KoopaOpCode {
    pub fn get_opcode(s: &str) -> Self {
        match s {
            "ret" => KoopaOpCode::RET,
            _ => unreachable!(),
        }
    }
}
