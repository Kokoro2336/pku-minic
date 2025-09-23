#!/bin/bash

# Set default values if not provided
COMPILER="${COMPILER:-./target/debug/sysy_compiler}"
MODE="${MODE:-koopa}"
LEVEL="${LEVEL:-1}"
TEST="${TEST:-0_main}"

# Function for gdbserver
gdbserver() {
    gdbserver :1234 "$COMPILER" "-$MODE" "./testcases/lv${LEVEL}/${TEST}.c" -o "./testcases/lv${LEVEL}/${TEST}.out"
}

# Function for gdb
gdb() {
    rust-gdb --args "$COMPILER" "-$MODE" "./testcases/lv${LEVEL}/${TEST}.c" -o "./testcases/lv${LEVEL}/${TEST}.out"
}

# Main script logic
case "${1:-}" in
    "gdbserver")
        gdbserver
        ;;
    "gdb")
        gdb
        ;;
    *)
        echo "Usage: $0 {gdbserver|gdb}"
        echo "Environment variables:"
        echo "  COMPILER: Path to compiler (default: ./target/debug/sysy_compiler)"
        echo "  MODE: Compiler mode (default: koopa)"
        echo "  LEVEL: Test level (default: 1)"
        echo "  TEST: Test name (default: 0_main)"
        exit 1
        ;;
esac
