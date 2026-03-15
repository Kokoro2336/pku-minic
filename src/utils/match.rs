//! Utils for pattern matching to reduce code duplication.

macro_rules! match_ops {
    (
        target: $target:expr,

        // Binary
        bin_ops: [ $($bin_op:ident),* $(,)? ],
        // Match arms.
        bin_arm: $SrcBin:ident { $lhs:ident, $rhs:ident } => $bin_body:tt,

        // Unary
        un_ops: [ $($un_op:ident),* $(,)? ],
        un_arm: $SrcUn:ident { $val:ident } => $un_body:tt,

        // Handwritten fallback branches (captured by tt)
        fallback: { $($rest:tt)* }
    ) => {
        match $target {
            // Unroll the binary operations.
            $(
                $SrcBin::$bin_op { $lhs, $rhs } => $bin_body,
            )*
            // Unroll the unary operations.
            $(
                $SrcUn::$un_op { $val } => $un_body,
            )*
            // Unroll the rest handwritten branches.
            $($rest)*
        }
    };
}

pub(crate) use match_ops;
