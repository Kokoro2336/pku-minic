/// type of value
#[derive(Debug, Clone)]
pub enum BType {
    Int(i32),
    Void,
}

impl BType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BType::INT => "int",
            BType::VOID => "void",
        }
    }
}

impl std::fmt::Display for BType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BType::INT => write!(f, "i32"),
            BType::VOID => write!(f, "void"),
        }
    }
}
