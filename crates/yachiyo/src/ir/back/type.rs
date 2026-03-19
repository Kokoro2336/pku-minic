use crate::base::Type;

#[derive(Debug, Clone)]
pub enum BType {
    Void,
    I32,
    F32,
    // For pointer
    U64,
}

impl From<Type> for BType {
    fn from(ty: Type) -> Self {
        match ty {
            Type::Int => BType::I32,
            Type::Float => BType::F32,
            Type::Void => BType::Void,
            Type::Bool => BType::I32, // bool is represented as i32 in machine code
            Type::Array { .. } | Type::Function { .. } => {
                unimplemented!("Array type is not supported in BType")
            }
            Type::Pointer { .. } => BType::U64,
            Type::Char => BType::I32, // char is represented as i32 in machine code
        }
    }
}
