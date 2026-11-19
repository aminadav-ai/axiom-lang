# Axiom (`axiom-lang`)

[![Axiom CI](https://github.com/aminadav-ai/axiom-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/aminadav-ai/axiom-lang/actions/workflows/ci.yml)
[![Deploy Showcase](https://github.com/aminadav-ai/axiom-lang/actions/workflows/pages.yml/badge.svg)](https://aminadav-ai.github.io/axiom-lang/)
[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue.svg)](LICENSE)

> **Live Interactive Wasm & Time-Travel Demo:** **[https://aminadav-ai.github.io/axiom-lang/](https://aminadav-ai.github.io/axiom-lang/)**

**Axiom** is an experimental AI-native programming language stack built for a world where autonomous AI agents write the code and humans verify **intent, mathematical guarantees, and capability boundaries** rather than reading 50,000 lines of source text.

---

## The Problem: Human Languages Are the Wrong Medium for AI

Every mainstream language (Python, TypeScript, C++, Go, Rust) was shaped around **human cognitive constraints**: typing comfort, named files and folders, indentation/syntax preferences, and dynamic or permissive defaults for fast manual prototyping.

When AI agents generate software in human-centric languages, three structural failures appear:
1. **Silent Placeholder Stubs Compile Cleanly:** An agent can write `return Ok(())` or return `balance` unchanged without deducting `amount`, and traditional typecheckers happily accept it because types do not prove postconditions.
2. **Hidden Ambient Authority:** Any helper buried 20 imports deep can silently read `~/.ssh` or open an outbound network socket unless a human manually audits the entire dependency tree.
3. **Context-Window & Dependency Bloat:** File trees, imports, naming collisions, and heavyweight runtime layers burn LLM tokens and system RAM.

---

## What Is Project Axiom? The 3-Language Stack

Instead of dozens of overlapping languages for scripts, backends, frontends, and configs, **Project Axiom** collapses computing into **three orthogonal layers** separated by the physics of computation:

| Layer | Role | Status in `v0.1` (Stage 1) |
| :--- | :--- | :--- |
| **`Axiom.Telos`** | **Intent, Contracts & Proofs.** Preconditions (`req`), postconditions (`ens`), and security boundaries proved mechanically via the **Z3 SMT solver** at compile time. | **Implemented** (`req`/`ens`, Z3 prover, vacuous-contract & counterexample engine) |
| **`Axiom.Flux`** | **Content-Addressed Logic & Effect Graph.** Zero files, zero imports. Every function is normalized (De Bruijn positional variables) and addressed by its 256-bit `BLAKE3` semantic hash (`#768a631876e7`), with explicit capability arrows `-[Net("..."), Audit]->` and opaque `Secret<T>` taint pipes. | **Implemented** (`.axiom/` graph store, semantic deduplication, static taint & passport analyzer) |
| **`Axiom.Strata`** | **Deterministic Silicon & Linear Memory.** Zero-overhead native machine & GPU execution without garbage-collector bloat or undefined behavior. | **Bootstrapped on Rust** (`rustc -O -C target-cpu=native` & `wasm32-unknown-unknown`) |

### Why Rust Is Used Right Now (The Rocket-Stage Bootstrap)

Building a production-grade register allocator and CPU/GPU instruction selector from scratch before testing the upper layers would lock the project in dark R&D for years. Instead, Axiom uses a **rocket-stage bootstrap**:

- **Today (Stage 1 — `axiom-lang`):** `Axiom.Telos` (Z3 proofs) and `Axiom.Flux` (content-addressed graph + taint/capability checker) are native to Axiom, while **Rust serves as the temporary `Axiom.Strata` backend**—compiling verified Axiom subgraphs into stripped **340 KB** native Linux binaries (cached in `1ms` by immutable node hash) and **32.3 KB** `.wasm` modules.
- **Later Stages (`axiom-zero`):** Once the higher layers are battle-tested, `Axiom.Strata` detaches Rust completely via direct self-hosted machine/GPU code generation.

### The 7-Stage Axiom Roadmap

1. **`axiom-lang` (This Repository — Stage 1):** Content-addressed semantic graph, Z3 anti-stub contract prover, `Secret<T>` taint pipes, native + 32 KB Wasm compiler, and Time-Travel Flight Recorder.
2. **`axiom-ui` (Stage 2):** Ultra-lightweight GPU/Wasm scene-graph engine replacing bloated Electron/HTML/CSS stacks (4 MB RAM, sub-10ms cold start) and defining capsules for the "New Web".
3. **`axiom-shell` (Stage 3):** Natural-language OS shell that translates human intent into Z3-verified capability graphs instead of error-prone `bash` strings.
4. **`axiom-studio` (Stage 4):** Native multi-agent graph IDE written in Axiom (conflict-free parallel node editing without text merge conflicts).
5. **`axiom-web` (Stage 5):** Native player for Axiom web capsules with a 100% local, sandboxed sanitizer for legacy `https://` sites (zero cloud MitM proxies).
6. **`axiom-zero` (Stage 6):** Native `Axiom.Strata` codegen backend & compiler self-hosting (**detaching Rust**).
7. **`axiom-os` (Stage 7):** Bootable minimal live-USB environment running the entire Axiom stack directly on hardware.

---

## Live Browser Demo (Why There Is a Thin HTML/JS Loader in Stage 1)

👉 **Try it in your browser:** **[https://aminadav-ai.github.io/axiom-lang/](https://aminadav-ai.github.io/axiom-lang/)**

> **Architecture Note on the Showcase:** Inside `showcase/checkout.wasm` (**32.3 KB**, zero external crates), **100% of the verified payment graph, `Secret<T>` redaction, embedded Security Passport, and Time-Travel Flight Recorder execute natively in WebAssembly**. Because legacy browsers cannot open a bare `.wasm` URL without a host page to bind DOM inputs to Wasm linear memory, Stage 1 wraps `checkout.wasm` in a minimal zero-dependency HTML/CSS/JS inspector. Starting in **Stage 2 (`axiom-ui`)**, UI rendering itself moves inside GPU/Wasm capsules.

---

## How Humans Debug & Audit Without Reading Code

1. **Unforgeable 4-Line Security Passport (`axiom passport`):**
   Generated deterministically by the compiler—never by an LLM. Lists every transitive capability (`Net("api.mybank.com")`, `Fs`, `Audit`) with its exact shortest call-chain witness, plus every `Secret<T>` data-flow pipe:
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
2. **Compile-Time Anti-Stub Counterexamples (`Z3`):**
   If an AI agent submits a fake stub (e.g., returning `balance` without subtracting `amount`), Z3 rejects it in milliseconds with a concrete counterexample:
   ```text
   error [E050_CONTRACT_VIOLATION] (1:1): Postcondition #1 failed for function 'withdraw_stub': ens ((result == (balance - amount)) && (result >= 0))
     counterexample: { amount = 1, balance = 1, result = 1 }
   ```
3. **Time-Travel Flight Recorder & Causal "Why?" Tree:**
   Every Wasm execution records deterministic frames that humans can scrub backward and forward with a timeline slider while keeping `Secret<T>` values redacted.

---

## Quick Start (Local CLI)

### Prerequisites
- Rust (`stable` toolchain + `wasm32-unknown-unknown` target)
- `z3` SMT solver binary on `PATH` (or `~/.local/bin/z3`)

```bash
# 1. Run full verification suite (Clippy, 18 integration tests, Z3 proofs, V8 Wasm replay)
./scripts/run_checks.sh

# 2. Verify & ingest an Axiom module into the content-addressed graph (.axiom/)
cargo run -- ingest examples/payment_demo.ax

# 3. Inspect semantic deduplication, minimal AI neighbor context, and Security Passport
cargo run -- graph
cargo run -- show checkout
cargo run -- passport checkout

# 4. Build & run a standalone 340 KB native binary (1ms cache hit on repeat builds)
cargo run -- build checkout
cargo run -- run checkout "sk_live_secret_token" 10000 --trace

# 5. Build 32.3 KB WebAssembly capsule or launch local Showcase HTTP server
cargo run -- build-wasm checkout -o checkout.wasm
cargo run -- serve-showcase
```

---

## License

Dual-licensed under **Business Source License 1.1 (`BUSL-1.1`)**—free for personal, academic, educational, non-commercial research, and evaluation use—and a separate **Commercial License** for commercial production deployments. See [LICENSE](LICENSE) and [CONTRIBUTING.md](CONTRIBUTING.md).
