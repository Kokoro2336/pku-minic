# Iroha

The main pipeline, including frontend, optimizer and backend.

## Structures

```text
src/
├── analysis/ - Program analysis passes (e.g., Dominator Tree, Dominance Frontier).
├── backend/  - Code generation and lowering to machine instructions.
├── frontend/ - Lexing, parsing, AST generation, and semantic analysis.
├── opt/      - Mid-level IR optimizations (e.g., Mem2Reg, SCCP).
└── main.rs   - Entry point of the compiler.
```
