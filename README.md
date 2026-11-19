# Axiom (`axiom-lang`)

**Axiom** is an AI-native programming language where source files, dependency version conflicts, silent stubs, and hidden side effects do not exist.

## Core Principles

1. **No Files, No Imports — Content-Addressed Semantic Graph:** Every function is alpha-normalized (positional De Bruijn parameters and locals) and stored by the `BLAKE3` cryptographic hash of its mathematical meaning (`#839a7cdc5893...`). Two agents writing identical logic with different variable names or formatting produce the **exact same node hash**.
2. **Proofs Over Stubs (`Telos` + `Z3`):** Preconditions (`req`) and postconditions (`ens`) are first-class syntax verified mechanically via Z3 SMT solving before a node is allowed into the graph. Placeholder stubs, vacuous preconditions, unguarded divisions by zero, and violated callee contracts are rejected at compile time with concrete counterexamples.
3. **Unforgeable Security Passport:** Functions have zero ambient authority. Side effects (`Net`, `Fs`, `Clock`, `Audit`) and `Secret<T>` flows are tracked transitively across the call graph with exact call-chain witnesses.

## Quick Start

```bash
# 1. Verify and ingest a module into the content-addressed graph (.axiom/)
cargo run -- ingest examples/payment_demo.ax

# 2. Inspect the semantic graph and transitive Security Passport
cargo run -- graph
cargo run -- show checkout
cargo run -- passport checkout

# 3. Watch Z3 catch an AI stub red-handed (with JSON diagnostic support)
cargo run -- verify examples/anti_stub_demo.ax
cargo run -- verify examples/anti_stub_demo.ax --json
```

## Automated Verification Suite

```bash
./scripts/run_checks.sh
```
