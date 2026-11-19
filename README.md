# Axiom (`axiom-lang`)

**Axiom** is an AI-native programming language where source files, dependency version conflicts, silent stubs, and hidden side effects do not exist.

## Core Principles
1. **No Files, No Imports — Content-Addressed Semantic Graph:** Every function is normalized (positional De Bruijn parameters/locals) and stored by the `BLAKE3` cryptographic hash of its mathematical meaning (`#a1b2c3...`). Two agents writing identical logic with different variable names produce the **exact same node hash**.
2. **Proofs Over Stubs (`Telos` + `Z3`):** Pre/post-conditions (`req` / `ens`) are part of the language grammar and verified mechanically via SMT solving.
3. **Unforgeable Security Passport:** Functions have zero ambient authority. Side effects (`Net`, `Fs`, `Clock`, `Audit`) and `Secret<T>` flows are tracked transitively across the call graph.

## Quick Start (Sprint 1 CLI)

```bash
cargo run -- ingest examples/payment_demo.ax
cargo run -- graph
cargo run -- show checkout
cargo run -- passport checkout
```

## Verification

```bash
./scripts/run_checks.sh
```
