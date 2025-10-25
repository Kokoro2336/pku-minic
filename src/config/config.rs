/// type of value
#[derive(Debug, Clone)]
pub enum BType {
    Int,
    Void,
}

impl BType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BType::Int => "int",
            BType::Void => "void",
        }
    }

    pub fn size_in_bytes(&self) -> u32 {
        match self {
            BType::Int => 4,
            BType::Void => 0,
        }
    }
}

impl std::fmt::Display for BType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BType::Int => write!(f, "i32"),
            BType::Void => write!(f, "void"),
        }
    }
}
