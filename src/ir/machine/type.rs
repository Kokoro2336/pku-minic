use crate::base::Type;

#[derive(Debug, Clone)]
pub enum MType {
    Void,
    I32,
    F32,
    // For pointer
    U64,
    Function {
        return_type: Box<MType>,
        param_types: Vec<MType>,
    },
}

impl From<Type> for MType {
    fn from(ty: Type) -> Self {
        match ty {
            Type::Int => MType::I32,
            Type::Float => MType::F32,
            Type::Void => MType::Void,
            Type::Bool => MType::I32, // bool is represented as i32 in machine code
            Type::Array { .. } => unimplemented!("Array type is not supported in MType"),
            Type::Pointer { .. } => MType::U64,
            Type::Function {
                return_type,
                param_types,
            } => MType::Function {
                return_type: Box::new(MType::from(*return_type)),
                param_types: param_types.into_iter().map(MType::from).collect(),
            },
            Type::Char => MType::I32, // char is represented as i32 in machine code
        }
    }
}
