struct BaseAST<T>{
    BaseUnit: T,
}

impl BaseAST {
    fn new<T>(base_unit: T) -> Self {
        BaseAST { BaseUnit: base_unit }
    }
}

pub struct CompUnit;
