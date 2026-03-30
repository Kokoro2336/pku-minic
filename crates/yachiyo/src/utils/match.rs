//! Utils for pattern matching to reduce code duplication.

#[macro_export]
macro_rules! match_src {
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
                $SrcBin::$bin_op { $lhs, $rhs, .. } => $bin_body,
            )*
            // Unroll the unary operations.
            $(
                $SrcUn::$un_op { $val, .. } => $un_body,
            )*
            // Unroll the rest handwritten branches.
            $($rest)*
        }
    };
}

#[macro_export]
/// For matching a few ops.
macro_rules! match_some {
    (
        target: $target:expr,
        enu: $SrcEnum:ident,
        // The few ops to match.
        minor_arms: { $($minor_arm:tt)* },

        // Struct-like variants to ignore. Macro expands each as `Variant { .. }`.
        uni_ops: [ $($uni_op:ident),* $(,)? ],
        uni_arm: $uni_arm:tt
    ) => {
        match $target {
            $($minor_arm)*
            $(
                $SrcEnum::$uni_op { .. } => $uni_arm,
            )*
        }
    };
}

#[macro_export]
macro_rules! match_full_ops {
    (
        target: $target:expr,

        // Binary
        bin_ops: [ $($bin_op:ident),* $(,)? ],
        // Match arms.
        bin_arm: $SrcBin:ident { $rd_bin:ident, $lhs:ident, $rhs:ident } => $bin_body:tt,

        // Unary
        un_ops: [ $($un_op:ident),* $(,)? ],
        un_arm: $SrcUn:ident { $rd_un:ident, $val:ident } => $un_body:tt,

        // Handwritten fallback branches (captured by tt)
        fallback: { $($rest:tt)* }
    ) => {
        match $target {
            // Unroll the binary operations.
            $(
                $SrcBin::$bin_op {
                    $rd_bin,
                    $lhs,
                    $rhs
                } => $bin_body,
            )*
            // Unroll the unary operations.
            $(
                $SrcUn::$un_op {
                    $rd_un,
                    $val,
                } => $un_body,
            )*
            // Unroll the rest handwritten branches.
            $($rest)*
        }
    };

    (
        target: $target:expr,

        // Binary
        bin_ops: [ $($bin_op:ident),* $(,)? ],
        // Match arms.
        bin_arm: $SrcBin:ident { $rd_bin:ident, $lhs:ident, $rhs:ident } => $bin_body:tt,

        // Unary
        un_ops: [ $($un_op:ident),* $(,)? ],
        un_arm: $SrcUn:ident { $rd_un:ident, $val:ident } => $un_body:tt
    ) => {
        match $target {
            // Unroll the binary operations.
            $(
                $SrcBin::$bin_op {
                    $rd_bin,
                    $lhs,
                    $rhs,
                    ..
                } => $bin_body,
            )*
            // Unroll the unary operations.
            $(
                $SrcUn::$un_op {
                    $rd_un,
                    value: $val,
                    ..
                } => $un_body,
            )*
        }
    };
}

#[macro_export]
macro_rules! match_rd {
    (
        target: $target:expr,

        op_with_rds: [ $($op_with_rd:ident),* $(,)? ],
        // Match arms.
        rd_arm: $SrcRd:ident($rd:ident) => $rd_body:block,

        // Handwritten fallback branches (captured by tt)
        fallback: { $($rest:tt)* }
    ) => {
        match $target {
            // Unroll the rd arms.
            $(
                $SrcRd::$op_with_rd { rd: $rd, .. } => $rd_body,
            )*
            // Unroll the rest handwritten branches.
            $($rest)*
        }
    };
}

pub use match_full_ops;
pub use match_rd;
pub use match_some;
pub use match_src;
