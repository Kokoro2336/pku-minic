use std::collections::HashMap;
use lazy_static::lazy_static;

lazy_static! {
    // mapping type of SysY to type of Koopa IR.
    pub static ref KOOPA_TYPE_MAP: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("int".to_string(), "i32".to_string());
        m
    };
}