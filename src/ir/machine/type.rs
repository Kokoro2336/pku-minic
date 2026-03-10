#[derive(Debug, Clone)]
pub enum MType {
    Void,
    I32,
    F32,
    Function {
        return_type: Box<MType>,
        param_types: Vec<MType>,
    }
}
