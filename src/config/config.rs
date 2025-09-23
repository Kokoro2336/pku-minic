/// type of value
#[derive(Debug, Clone)]
pub enum ValueType {
    INT,
    VOID,
}

impl ValueType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "int" => ValueType::INT,
            "void" => ValueType::VOID,
            _ => unreachable!(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::INT => "int",
            ValueType::VOID => "void",
        }
    }
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValueType::INT => write!(f, "i32"),
            ValueType::VOID => write!(f, "void"),
        }
    }
}
