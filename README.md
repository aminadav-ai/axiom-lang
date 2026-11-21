# Axiom (`axiom-lang` v0.2)

[![Axiom CI](https://github.com/aminadav-ai/axiom-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/aminadav-ai/axiom-lang/actions/workflows/ci.yml)
[![Deploy Showcase](https://github.com/aminadav-ai/axiom-lang/actions/workflows/pages.yml/badge.svg)](https://aminadav-ai.github.io/axiom-lang/)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

> **Official Axiom Ecosystem Website & Live Launcher:** **[https://axiom-lang.org](https://axiom-lang.org)**  
> **Stages 3 & 4 — Hybrid AI-First IDE & Shell (`2.54 MB` Daemon + `11.68 KB` Cyrillic/EN GPU Wasm):** **[Axiom Studio v1.0](https://axiom-lang.org/axiom-studio/)** ([Repo](https://github.com/aminadav-ai/axiom-studio))  
> **Live Interactive Compiler & Time-Travel Demo:** **[https://aminadav-ai.github.io/axiom-lang/](https://aminadav-ai.github.io/axiom-lang/)**  
> **Stage 2 — Zero-DOM Hardware GPU UI Engine (`15.34 KB` Wasm, `1` Draw Call):** **[Axiom-UI v1.0 Gallery](https://aminadav-ai.github.io/axiom-ui/)** ([Repo](https://github.com/aminadav-ai/axiom-ui))  
> **Stage 2 — 1,000,000-Cell Hardware GPU Spreadsheet (`21.65 KB` Wasm):** **[Axiom Grid v1.0](https://aminadav-ai.github.io/axiom-grid/)** ([Repo](https://github.com/aminadav-ai/axiom-grid))  
> **Stage 2 — Multi-Window GPU Desktop OS ("Electron Killer", `20.57 KB` Wasm):** **[Axiom Workspace v1.0](https://aminadav-ai.github.io/axiom-workspace/)** ([Repo](https://github.com/aminadav-ai/axiom-workspace))  
> **Stage 2 — Capability Gatekeeper & Legacy HTML Clean-Reader (`21.21 KB` Wasm):** **[Axiom Capsule Browser v1.0](https://aminadav-ai.github.io/axiom-capsule-browser/)** ([Repo](https://github.com/aminadav-ai/axiom-capsule-browser))  
> **Stage 1 — True-3D Polygonal GPU Shooter (`30.38 KB` Wasm):** **[Axiom Slipgate v2.0 GPU HD](https://aminadav-ai.github.io/axiom-slipgate/)** ([Repo](https://github.com/aminadav-ai/axiom-slipgate))  
> **Stage 1 — True-3D Sculpted Savanna Shepherd (`17.87 KB` Wasm):** **[Axiom Lion Herd v3.0 GPU HD](https://aminadav-ai.github.io/axiom-lion-herd/)** ([Repo](https://github.com/aminadav-ai/axiom-lion-herd))  
> **Stage 1 — True-3D Refractive Glass & Marble Puzzle (`15.32 KB` Wasm):** **[Axiom Color Lines 98 v3.0 GPU HD](https://aminadav-ai.github.io/axiom-color-lines/)** ([Repo](https://github.com/aminadav-ai/axiom-color-lines))

**Axiom** is an AI-native programming language stack built for a world where autonomous AI agents write the software and humans verify **intent, mathematical guarantees, and capability boundaries** rather than auditing 50,000 lines of source text.

---

## Stage 2 Flagship Suite: `axiom-ui` Zero-DOM Hardware GPU Applications

All four Stage 2 applications below eliminate **HTML DOM widgets, CSS Cascade/Reflow, Virtual-DOM diffing, and Electron (`~250 MB`)**, rendering rounded SDF surfaces, drop shadows, anti-aliased vector charts/wires, hardware scissor-clipped scrolling viewports, and HiDPI typography in **a single `gl.drawArraysInstanced` GPU draw call** directly from `Axiom.Strata` linear memory (`zero rustc`):

| Application | Launch Live | Source | `.wasm` Binary | Gzipped Wire Size | Z3 Proofs | Draw Calls / Frame | CPU Layout Time |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Axiom-UI v1.0 Engine & Gallery** *(Interactive GPU Text Input, Switches, Sliders, Live SDF Waveform, Z3 Proofs & 120-Frame Time-Travel)* | **[Launch App](https://aminadav-ai.github.io/axiom-ui/)** | [`axiom_ui_showcase.ax`](https://github.com/aminadav-ai/axiom-ui/blob/main/src/axiom_ui_showcase.ax) | **`15.34 KB`** *(15,707 B)* | **`5.38 KB`** | **100% Proved** | **`1` Instanced Call** (`624` prims) | **`0.20 ms`** |
| **Axiom Grid v1.0** *(1,000,000-Cell `1000×1000` `A1..ALL1000` Spreadsheet, Live `=SUM`/`=AVG`/`=MAX`, `fx` Bar, Live GPU Charts & Time-Travel)* | **[Launch App](https://aminadav-ai.github.io/axiom-grid/)** | [`grid.ax`](https://github.com/aminadav-ai/axiom-grid/blob/main/src/grid.ax) | **`21.65 KB`** *(22,167 B)* | **`7.93 KB`** | **100% Proved** | **`1` Instanced Call** (`1,347` prims) | **`0.50 ms`** |
| **Axiom Workspace v1.0** *(Multi-Window GPU Desktop OS: 4 Draggable/Resizable Z-Ordered Windows, `Flux` Security Graph, Stress-Test, Z3 Calc & Time-Travel)* | **[Launch App](https://aminadav-ai.github.io/axiom-workspace/)** | [`workspace.ax`](https://github.com/aminadav-ai/axiom-workspace/blob/main/src/workspace.ax) | **`20.57 KB`** *(21,062 B)* | **`6.84 KB`** | **100% Proved** | **`1` Instanced Call** (`1,327` prims) | **`0.23 ms`** |
| **Axiom Capsule Browser v1.0** *(Prototype of `axiom-web`: Pre-Execution `axiom.passport` Spyware Gatekeeper + Pure-Axiom Legacy HTML Clean-Reader)* | **[Launch App](https://aminadav-ai.github.io/axiom-capsule-browser/)** | [`capsule_browser.ax`](https://github.com/aminadav-ai/axiom-capsule-browser/blob/main/src/capsule_browser.ax) | **`21.21 KB`** *(21,714 B)* | **`7.04 KB`** | **100% Proved** | **`1` Instanced Call** (`762` prims) | **`0.14 ms`** |

---

## Stage 1 Flagship Suite: 3D Hardware GPU Games Built in 100% Axiom (`v0.2`)

All three complete browser games below are written **100% in Axiom** (`Telos` Z3 contracts + `Flux` content-addressed graph + `Strata` 8 MiB linear-memory physics, procedural `256×256` Worley/FBM bump-mapped textures, `-[Mem, Gpu]->` WebGL2 hardware 3D rendering, procedural Web Audio triggers, and Time-Travel ring buffers) and compiled **directly to standalone W3C WebAssembly bytecode with ZERO `rustc`, ZERO `wasm-bindgen`, and ZERO game-engine runtime**:

| Project | Play Live | Source | `.wasm` Binary | Gzipped Wire Size | Z3 Proofs Proved | Resolution & Engine | Runtime FPS |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Axiom Slipgate v2.0 GPU HD** *(True 3D Polygonal Citadel, `256×256` Bump-Mapped Textures, Pillar/Wall LOS Cover, Walkable Terraces, 3D Shambler/Knight/Grunt, Cylindrical Super-Shotgun)* | **[Play Live](https://aminadav-ai.github.io/axiom-slipgate/)** | [`slipgate.ax`](https://github.com/aminadav-ai/axiom-slipgate/blob/main/src/slipgate.ax) | **`30.38 KB`** *(31,109 B)* | **`11.66 KB`** | **100% Proved** | **Native HD `WebGL2`** (`-[Mem, Gpu]->` Bump + Multi-Light) | **60+ FPS** (`0.39 ms` CPU) |
| **Axiom Lion Herd v3.0 GPU HD** *(True 3D Savanna Plateau, 14 Ellipsoid Bump-Mapped Merino Sheep, 3D Maned Lion + Roar Shockwave, Split-Rail Corral, Acacia Trees, Sun Shadows)* | **[Play Live](https://aminadav-ai.github.io/axiom-lion-herd/)** | [`lion_herd.ax`](https://github.com/aminadav-ai/axiom-lion-herd/blob/main/src/lion_herd.ax) | **`17.87 KB`** *(18,300 B)* | **`7.24 KB`** | **100% Proved** | **Native HD `WebGL2`** (`10,284` 3D Tris + Sun Shadows) | **60+ FPS** (`1.16 ms` CPU) |
| **Axiom Color Lines 98 v3.0 GPU HD** *(True 3D Refractive Glass Jewel Spheres, Carved Marble & Walnut Board, Colored Caustics, Parabolic 3D BFS Hops, Audio, Turn Time-Travel)* | **[Play Live](https://aminadav-ai.github.io/axiom-color-lines/)** | [`color_lines.ax`](https://github.com/aminadav-ai/axiom-color-lines/blob/main/src/color_lines.ax) | **`15.32 KB`** *(15,690 B)* | **`5.82 KB`** | **100% Proved** | **Native HD `WebGL2`** (GLSL Refraction + Caustics) | **60+ FPS** (`0.24 ms` CPU) |
| **Axiom Payment & Taint Inspector** *(Secret\<T\> Redaction, Security Passport, Flight Recorder)* | **[Open Demo](https://aminadav-ai.github.io/axiom-lang/)** | [`payment_demo.ax`](examples/payment_demo.ax) | **`32.3 KB`** | **`~11.4 KB`** | **8 / 8** | Interactive Inspector | Interactive |

> **Universal Cross-Repository Deduplication in Action:** Although [`axiom-slipgate`](https://github.com/aminadav-ai/axiom-slipgate), [`axiom-lion-herd`](https://github.com/aminadav-ai/axiom-lion-herd), and [`axiom-color-lines`](https://github.com/aminadav-ai/axiom-color-lines) live in three separate repositories with zero shared imports or packages, shared pure kernels (`clamp_byte` -> `#3ab09c0f9b38`, `abs_val` -> `#5f2b18594c39`, `pack_rgba` -> `#89767d82293f`) automatically converge to the **exact same 256-bit semantic hashes**.

---

## The Problem: Human Languages Are the Wrong Medium for AI

Every mainstream language (Python, TypeScript, C++, Go, Rust) was shaped around **human cognitive constraints**: typing comfort, named files and folders, indentation preferences, and permissive defaults for manual prototyping.

When AI agents generate software in human-centric languages, three structural failures appear:
1. **Silent Placeholder Stubs Compile Cleanly:** An agent can write `return Ok(())` or return `balance` unchanged without deducting `amount`, and traditional typecheckers accept it because types do not prove postconditions.
2. **Hidden Ambient Authority:** Any helper buried 20 imports deep can silently read `~/.ssh` or open an outbound network socket unless a human manually audits the entire dependency tree.
3. **Context-Window & Binary Bloat:** File trees, imports, naming collisions, and megabyte-scale runtime dependencies waste LLM context tokens, compile minutes, and user bandwidth.

---

## What Is Project Axiom? All 3 Layers Live in `v0.2`

Instead of dozens of overlapping languages for scripts, backends, frontends, and shaders, **Project Axiom** unifies software into **three orthogonal layers** separated by the physics of computation:

| Layer | Role | Status in `v0.2` |
| :--- | :--- | :--- |
| **`Axiom.Telos`** | **Intent, Contracts & Proofs.** Preconditions (`req`), postconditions (`ens`), non-zero divisor obligations, and vacuous-contract guards proved mechanically via **Microsoft Z3** at compile time. | **Implemented Natively** (`src/verify.rs`) |
| **`Axiom.Flux`** | **Content-Addressed Logic & Effect Graph.** Zero files, zero imports. Every function is normalized (De Bruijn positional variables) and addressed by its 256-bit `SHA-256` semantic hash (`#768a631876e7`), with explicit capability arrows `-[Net("..."), Audit, Mem]->` and opaque `Secret<T>` taint pipes. | **Implemented Natively** (`src/canon.rs`, `src/store.rs`, `src/typecheck.rs`) |
| **`Axiom.Strata`** | **Deterministic Silicon, Linear Memory & Hardware GPU.** Bounded loops (`while cond max N`), bitwise SIMD-friendly math, 8 MiB hardware-masked linear memory (`mem_load`/`mem_store`), explicit `-[Mem, Gpu]->` WebGL2 3D capability, **direct W3C WebAssembly binary emission** (`0.67 KB`–`28.64 KB`), and **direct Linux `x86_64` ELF64 machine-code emission** (`1.45 KB` executables) with **zero `rustc`**. | **Implemented Natively (`v0.2`)** (`src/strata_wasm.rs`, `src/strata_elf.rs`) + Rust host bridge (`src/codegen.rs`) for high-level `Secret<str>` I/O |

### Dual Backend Architecture in `v0.2`

- **Direct Zero-`rustc` `Axiom.Strata` Emitter (`compile-wasm`, `build-strata-wasm`, `build-strata-elf`, `run-strata`):** Emits raw W3C WebAssembly 1.0 bytecode (embedding the unforgeable `axiom.passport` custom section) or standalone `1.4 KB` Linux `x86_64` ELF64 binaries directly from the verified Axiom graph in milliseconds — no LLVM, no `rustc`, no `libc`.
- **High-Level `Axiom.Flux` Host Bridge (`build`, `build-wasm`, `run`):** Lowers high-level `Secret<str>` / domain-pinned network capability workflows through `rustc -O` with immutable content-hash binary caching (`0 ms` cache hits).

---

## How Humans Debug & Audit Without Reading Code

1. **Unforgeable Security Passport (`axiom passport`):**
   Generated deterministically by the compiler—never by an LLM. Lists every transitive capability (`Net("api.mybank.com")`, `Mem`, `Audit`) with its exact shortest call-chain witness, plus every `Secret<T>` data-flow pipe:
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
3. **Linear-Memory Time-Travel Scrubbing:**
   Deterministic state layout in `Axiom.Strata` makes full-frame and turn-by-turn Time-Travel scrubbing trivial inside a few kilobytes of WebAssembly.

---

## Quick Start (Local CLI)

### Prerequisites
- Rust (`stable` toolchain to build the `axiom` CLI binary once)
- `z3` SMT solver binary on `PATH` (or `~/.local/bin/z3`)

```bash
# 1. Run full v0.2 verification suite (Clippy, Sprints 1..7 tests, Z3 proofs, Strata Wasm & ELF64)
./scripts/run_checks.sh

# 2. Direct Axiom.Strata -> WebAssembly (.wasm) & Linux x86_64 ELF64 (ZERO rustc invoked!)
cargo run -- compile-wasm examples/strata_framebuffer_demo.ax -o demo.wasm
cargo run -- build-strata-elf render_gradient -o demo_elf
cargo run -- run-strata render_gradient 32 32 7

# 3. Verify & ingest high-level Secret<T> payment graph and inspect its Security Passport
cargo run -- ingest examples/payment_demo.ax
cargo run -- passport checkout
cargo run -- run checkout "sk_live_secret_token" 10000 --trace
```

---

## The 7-Stage Axiom Roadmap

1. **`axiom-lang` (Completed — `v0.1` & `v0.2`):** Content-addressed semantic graph, Z3 anti-stub contract prover, `Secret<T>` taint pipes, native `Axiom.Strata` zero-`rustc` `.wasm` (with static zero-copy string pool) & `x86_64` ELF64 emitters, and Time-Travel execution.
2. **`axiom-ui` (Completed — Stage 2):** Unified 1-Draw-Call WebGL2 SDF & HiDPI Font-Atlas GPU UI engine replacing Electron/HTML/CSS stacks, demonstrated via [`axiom-ui`](https://github.com/aminadav-ai/axiom-ui), [`axiom-grid`](https://github.com/aminadav-ai/axiom-grid) (1M cells), [`axiom-workspace`](https://github.com/aminadav-ai/axiom-workspace) (Multi-Window Desktop OS), and [`axiom-capsule-browser`](https://github.com/aminadav-ai/axiom-capsule-browser).
3. **`axiom-shell` & `axiom-studio` (Completed — Stages 3 & 4):** Ultra-lightweight native Hybrid AI IDE (`2.54 MB` Rust daemon + `11.68 KB` multilingual Latin/Cyrillic/`₪` WebGL2 DAG engine) with integrated `axiom-shell`, 1-click Antigravity session migration, and 3-mode AI (`Google One Ultra Bridge`, `Gemini 4 Carbon API`, `Ollama Offline`) in [`aminadav-ai/axiom-studio`](https://github.com/aminadav-ai/axiom-studio).
5. **`axiom-web` (Stage 5):** Native player for Axiom web capsules with local sandboxed sanitization.
6. **`axiom-zero` (Stage 6):** Full compiler self-hosting on `Axiom.Strata`.
7. **`axiom-os` (Stage 7):** Bootable minimal live-USB environment running the Axiom stack directly on hardware.

---

## License

Copyright (c) 2026 Yehuda Aminadav. Licensed under the **Apache License, Version 2.0 (`Apache-2.0`)**. See [LICENSE](LICENSE) and [CONTRIBUTING.md](CONTRIBUTING.md).
