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

/// For matching a few ops.
macro_rules! match_minor {
    (
        target: $target:expr,

        // The few ops to match.
        minor_arms: { $($minor_arm:tt)* },

        // Struct-like variants to ignore. Macro expands each as `Variant { .. }`.
        uni_ops: [ $($uni_op:path),* $(,)? ],
        // Optional tuple/unit variants or custom patterns to ignore.
        other_patterns: [ $($other_pat:pat),* $(,)? ],
        uni_arm: $uni_arm:tt
    ) => {
        match $target {
            $($minor_arm)*
            $(
                $uni_op { .. } => $uni_arm,
            )*
            $(
                $other_pat => $uni_arm,
            )*
        }
    };

    (
        target: $target:expr,

        // The few ops to match.
        minor_arms: { $($minor_arm:tt)* },

        // Struct-like variants to ignore. Macro expands each as `Variant { .. }`.
        uni_ops: [ $($uni_op:path),* $(,)? ],
        uni_arm: $uni_arm:tt
    ) => {
        match $target {
            $($minor_arm)*
            $(
                $uni_op { .. } => $uni_arm,
            )*
        }
    };
}

pub(crate) use match_minor;
pub(crate) use match_ops;
