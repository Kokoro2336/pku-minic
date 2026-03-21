# Yachiyo

Definitions of infrastructures.

## Structures

```text
src/
├── ast/    - Abstract Syntax Tree definitions and nodes used by the frontend.
├── base/   - Base types (e.g., `Type`) and system library mappings.
├── cli/    - Command-line interface definitions and argument parsing logic.
├── config/ - Compiler-wide configurations and constants (e.g., register limits).
├── debug/  - Debugging utilities, logging macros, and IR dumpers (e.g., `DumpLLVM`).
├── ir/     - Intermediate Representation structures, encompassing Mid IR and Back/Lower IR.
├── pass/   - Pass management frameworks to orchestrate optimizations and transformations.
└── utils/  - Reusable data structures and helpers (e.g., Arena allocators, BitSet, SymbolTable).
```
