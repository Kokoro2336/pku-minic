//! SysY runtime library definitions.

use crate::base::Type;

thread_local! {
    pub static SYSY_LIB: Vec<(&'static str, Type)> = vec![
        (
            "getint",
            Type::Function {
                return_type: Box::new(Type::Int),
                param_types: vec![],
            },
        ),
        (
            "getfloat",
            Type::Function {
                return_type: Box::new(Type::Float),
                param_types: vec![],
            },
        ),
        (
            "getch",
            Type::Function {
                return_type: Box::new(Type::Int),
                param_types: vec![],
            },
        ),
        (
            "getarray",
            Type::Function {
                return_type: Box::new(Type::Int),
                param_types: vec![Type::Int.with_ptr()],
            },
        ),
        (
            "getfarray",
            Type::Function {
                return_type: Box::new(Type::Int),
                param_types: vec![Type::Float.with_ptr()],
            },
        ),
        (
            "putint",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![Type::Int],
            },
        ),
        (
            "putfloat",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![Type::Float],
            },
        ),
        (
            "putch",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![Type::Int],
            },
        ),
        (
            "putarray",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![
                    Type::Int,
                    Type::Int.with_ptr(),
                ],
            },
        ),
        (
            "putfarray",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![
                    Type::Int,
                    Type::Float.with_ptr(),
                ],
            },
        ),
        (
            "putf",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![
                    Type::Char.with_ptr(),
                    /*only store the string type, since the trailing params are dynamic according to the format string*/
                ],
            },
        ),
        (
            "_sysy_starttime",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![],
            },
        ),
        (
            "_sysy_stoptime",
            Type::Function {
                return_type: Box::new(Type::Void),
                param_types: vec![],
            },
        ),
    ];
}
