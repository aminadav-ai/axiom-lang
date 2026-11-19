# Axiom (`axiom-lang`)

**Axiom** is an AI-native programming language designed for autonomous code synthesis rather than human typing ergonomics. Source files, import paths, dependency version conflicts, silent AI placeholder stubs, and hidden ambient side effects do not exist in Axiom.

---

## Why Axiom?

Traditional languages (Python, TypeScript, C++, Rust) optimize for human cognitive habits while leaving massive blind spots when AI agents author software:
- **Silent Stubs Compile Cleanly:** Returning `Ok(())` or `balance` unchanged passes traditional typecheckers because types do not encode postconditions.
- **Hidden Ambient Authority:** Any transitive helper 20 imports deep can open a network socket or read `~/.ssh` unless a human manually audits thousands of lines of code.
- **Token & Context Bloat:** Verbose syntax, formatting bikesheds, and file/import trees burn LLM context windows.

### How Axiom Solves All Three

1. **Content-Addressed Semantic Graph (Zero Files, Zero Imports):**
   Every function is alpha-normalized via De Bruijn positional indexing (`p0, p1...`, `l0, l1...`) and stored by the `BLAKE3` cryptographic hash of its mathematical AST (`#3fca26293539`). Two agents writing identical logic with different variable names or formatting produce the **exact same node hash**. Renaming human aliases never breaks callers.
2. **Proofs Over Stubs (`Telos` + `Z3` SMT Solver):**
   Preconditions (`req`) and postconditions (`ens`) are verified mechanically via Z3 before any node is admitted to `.axiom/nodes/`. Placeholder stubs, vacuous preconditions (`req x > 10 && x < 5`), unguarded divisions by zero, and violated callee contracts are rejected at compile time with concrete counterexamples and machine-readable JSON repair hints.
3. **Unforgeable Security Passport & `Secret<T>` Taint Pipes:**
   Nodes hold zero ambient capabilities. Every side effect (`Net("domain")`, `Fs("/path")`, `Audit`, `Clock`) must be declared in `-[...]->`. `Secret<T>` values are opaque (blocking timing/branching side channels and log/disk leaks) and can only be unsealed via `reveal("domain", secret)` directly inside a node whose sole network capability is pinned to `Net("domain")`.
4. **Dual Native & 32 KB WebAssembly Targets + Time-Travel Flight Recorder:**
   Verified subgraphs prune all unreachable nodes and compile either to stripped native binaries (`rustc -O -C target-cpu=native`, cached in `1ms` by node hash) or compact **32.2 KB `.wasm` capsules** with an embedded Security Passport and a deterministic **Time-Travel Flight Recorder**.

---

## Quick Start

### Prerequisites
- Rust (`stable` toolchain + `wasm32-unknown-unknown` target)
- `z3` SMT solver binary on `PATH` (or `~/.local/bin/z3`)

### 1. Launch the Interactive Browser Showcase (Wasm + Time-Travel Slider)

```bash
cargo run -- serve-showcase
# Open http://127.0.0.1:8787 in your browser
```

### 2. Verify, Ingest, Inspect, and Compile from CLI

```bash
# Verify via Z3 + Taint Checker and store nodes in .axiom/
cargo run -- ingest examples/payment_demo.ax

# View content-addressed graph, minimal AI neighbor context, and Security Passport
cargo run -- graph
cargo run -- show checkout
cargo run -- passport checkout

# Compile & run native binary (cached by immutable BLAKE3 node hash)
cargo run -- build checkout
cargo run -- run checkout "sk_live_secret_token" 10000 --trace

# Compile 32.2 KB WebAssembly capsule with embedded Time-Travel Flight Recorder
cargo run -- build-wasm checkout -o checkout.wasm
```

### 3. Watch Axiom Reject AI Stubs & Secret Exfiltration

```bash
# Z3 catches a fake AI stub and outputs { amount = 1, balance = 1, result = 1 }
cargo run -- verify examples/anti_stub_demo.ax

# Static taint checker blocks domain-mismatched secret exfiltration
cargo run -- verify examples/secret_leak_demo.ax

# Machine-readable JSON diagnostics for autonomous agent repair loops
cargo run -- verify examples/anti_stub_demo.ax --json
```

---

## Example Axiom Code (`examples/payment_demo.ax`)

```text
fn calc_fee_agent_a(amount: i64, rate_bps: i64) -> i64
  req amount >= 0 && rate_bps >= 0 && rate_bps <= 5000
  ens result >= 0 && result <= amount / 2
{
  let raw = amount * rate_bps;
  raw / 10000
}

fn send_bank_wire(token: Secret<str>, net_amount: i64) -[Net("api.mybank.com"), Audit]-> bool
  req net_amount > 0
  ens result == true
{
  do Audit.record(net_amount);
  do Net.post(reveal("api.mybank.com", token), net_amount);
  true
}

fn checkout(auth_token: Secret<str>, total: i64) -> bool
  req total > 100
  ens result == true
{
  let fee = calc_fee_agent_a(total, 250);
  let net = total - fee;
  send_bank_wire(auth_token, net)
}
```

Running `cargo run -- passport checkout` produces an unforgeable 4-line audit without reading code:

```text
=== AXIOM SECURITY PASSPORT ===
Node:                 #768a631876e7 (checkout)
Reachable Subgraph:   3 node(s)
Touches Secret<T>:    YES
Capabilities:
  [CAP]  Audit                via: checkout (#768a631876e7) -> send_bank_wire (#644f79bfca37)
  [CAP]  Net("api.mybank.com") via: checkout (#768a631876e7) -> send_bank_wire (#644f79bfca37)
Secret Data-Flow Pipes:
  [PIPE] Secret<str> -> Net.post ("api.mybank.com")  via: checkout (#768a631876e7) -> send_bank_wire (#644f79bfca37)
  [SAFE] Disk / Log / Unpinned Leaks: 0 (verified by taint checker)
```

---

## Run Full Verification Suite

```bash
./scripts/run_checks.sh
```

---

## License

Dual-licensed under **Business Source License 1.1 (`BUSL-1.1`)** (free for personal, academic, non-commercial research, and evaluation use) and a **Commercial License** for commercial production use. See [LICENSE](LICENSE) and [CONTRIBUTING.md](CONTRIBUTING.md).
