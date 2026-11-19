# Axiom (`axiom-lang`)

**Axiom** is an AI-native programming language where source files, dependency version conflicts, silent stubs, and hidden side effects do not exist.

## Core Principles

1. **No Files, No Imports — Content-Addressed Semantic Graph:** Every function is alpha-normalized (positional De Bruijn parameters and locals) and stored by the `BLAKE3` cryptographic hash of its mathematical meaning (`#3fca26293539...`). Two agents writing identical logic with different variable names or formatting produce the **exact same node hash**.
2. **Proofs Over Stubs (`Telos` + `Z3`):** Preconditions (`req`) and postconditions (`ens`) are first-class syntax verified mechanically via Z3 SMT solving before a node is allowed into the graph. Placeholder stubs, vacuous preconditions, unguarded divisions by zero, and violated callee contracts are rejected at compile time with concrete counterexamples.
3. **Unforgeable Security Passport & `Secret<T>` Taint Pipes:** Functions have zero ambient authority. Side effects (`Net`, `Fs`, `Clock`, `Audit`) and opaque `Secret<T>` values are statically checked: secrets cannot be logged to disk/audit, compared in side-channel branches, or transmitted to unpinned/mismatched network hosts.

## Quick Start

```bash
# 1. Verify and ingest a module into the content-addressed graph (.axiom/)
cargo run -- ingest examples/payment_demo.ax

# 2. Inspect the semantic graph, capabilities, and Secret<T> data-flow pipes
cargo run -- graph
cargo run -- show checkout
cargo run -- passport checkout

# 3. Watch Z3 catch an AI stub red-handed
cargo run -- verify examples/anti_stub_demo.ax

# 4. Watch the Taint Checker block unauthorized secret exfiltration
cargo run -- verify examples/secret_leak_demo.ax
```

## Automated Verification Suite

```bash
./scripts/run_checks.sh
```

## License

Dual-licensed under **Business Source License 1.1 (`BUSL-1.1`)** (free for non-commercial, academic, personal, and evaluation use) and a **Commercial License** for commercial production deployments. See [LICENSE](LICENSE) and [CONTRIBUTING.md](CONTRIBUTING.md) for details.
