//! Direct W3C WebAssembly 1.0 Binary Bytecode Emitter for `Axiom.Strata`.
//!
//! Compiles verified Axiom graphs directly into raw `.wasm` machine bytes in ~1-3ms
//! with ZERO intermediate Rust code generation and ZERO external `rustc` invocation.

use crate::ast::{BinOp, Expr, FnDef, Module, Stmt, TargetRef, Type, UnaryOp};
use crate::canon::CanonicalNode;
use crate::diag::{AxiomDiagnostic, Span};
use crate::store::GraphStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::time::Instant;

const WASM_TYPE_I32: u8 = 0x7F;
const WASM_TYPE_I64: u8 = 0x7E;
const WASM_BLOCK_VOID: u8 = 0x40;
const WASM_BLOCK_I64: u8 = 0x7E;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrataWasmArtifact {
    pub exported_functions: Vec<String>,
    pub compiled_nodes: usize,
    pub wasm_path: String,
    pub wasm_size_bytes: u64,
    pub build_us: u128,
}

/// Compile an ingested module or entrypoint directly from `.axiom/` to a pure `.wasm` binary.
pub fn build_strata_wasm_from_module(
    store: &mut GraphStore,
    module: &Module,
    out_path: &Path,
) -> Result<StrataWasmArtifact, AxiomDiagnostic> {
    let t0 = Instant::now();
    let entries = store.ingest_module(module)?;

    // Determine exported functions: any `export fn` in module, or the last function if none marked `export`
    let mut export_pairs: Vec<(String, String)> = Vec::new();
    for raw_fn in &module.functions {
        if raw_fn.exported {
            let h = store.resolve_ref(&raw_fn.name)?;
            export_pairs.push((raw_fn.name.clone(), h));
        }
    }
    if export_pairs.is_empty() {
        if let Some(last) = entries.last() {
            export_pairs.push((last.alias.clone(), last.hash.clone()));
        }
    }

    let root_hashes: Vec<String> = export_pairs.iter().map(|(_, h)| h.clone()).collect();
    let nodes = collect_multi_root_subgraph(store, &root_hashes)?;

    // Embed SecurityPassport of primary entrypoint in Wasm custom section `axiom.passport`
    let passport_bytes = if let Some((alias, _)) = export_pairs.first() {
        let p = store.compute_passport(alias)?;
        serde_json::to_vec(&p).unwrap_or_default()
    } else {
        Vec::new()
    };

    let wasm_bytes = emit_strata_wasm_bytes(&nodes, &export_pairs, &passport_bytes)?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(out_path, &wasm_bytes).map_err(|e| {
        AxiomDiagnostic::new(
            "E080_WASM_IO",
            "strata-wasm",
            format!("Failed to write Strata .wasm '{}': {e}", out_path.display()),
            Span::zero(),
            "Check write permissions on target path",
        )
    })?;

    Ok(StrataWasmArtifact {
        exported_functions: export_pairs.into_iter().map(|(name, _)| name).collect(),
        compiled_nodes: nodes.len(),
        wasm_path: out_path.display().to_string(),
        wasm_size_bytes: wasm_bytes.len() as u64,
        build_us: t0.elapsed().as_micros(),
    })
}

pub fn build_strata_wasm_target(
    store: &GraphStore,
    target_ref: &str,
    out_path: &Path,
) -> Result<StrataWasmArtifact, AxiomDiagnostic> {
    let t0 = Instant::now();
    let root_hash = store.resolve_ref(target_ref)?;
    let aliases = store.aliases_for_hash(&root_hash);
    let export_name = aliases
        .first()
        .cloned()
        .unwrap_or_else(|| "main".to_string());
    let export_pairs = vec![(export_name.clone(), root_hash.clone())];

    let nodes = collect_multi_root_subgraph(store, &[root_hash])?;
    let passport = store.compute_passport(target_ref)?;
    let passport_bytes = serde_json::to_vec(&passport).unwrap_or_default();

    let wasm_bytes = emit_strata_wasm_bytes(&nodes, &export_pairs, &passport_bytes)?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(out_path, &wasm_bytes).map_err(|e| {
        AxiomDiagnostic::new(
            "E080_WASM_IO",
            "strata-wasm",
            format!("Failed to write Strata .wasm '{}': {e}", out_path.display()),
            Span::zero(),
            "Check write permissions on target path",
        )
    })?;

    Ok(StrataWasmArtifact {
        exported_functions: vec![export_name],
        compiled_nodes: nodes.len(),
        wasm_path: out_path.display().to_string(),
        wasm_size_bytes: wasm_bytes.len() as u64,
        build_us: t0.elapsed().as_micros(),
    })
}

pub fn collect_multi_root_subgraph(
    store: &GraphStore,
    roots: &[String],
) -> Result<Vec<CanonicalNode>, AxiomDiagnostic> {
    let mut visited: BTreeMap<String, CanonicalNode> = BTreeMap::new();
    let mut queue: VecDeque<String> = roots.iter().cloned().collect();

    while let Some(h) = queue.pop_front() {
        if visited.contains_key(&h) {
            continue;
        }
        let node = store.load_node(&h)?;
        check_strata_compatible(&node)?;
        for dep in &node.direct_deps {
            if !visited.contains_key(dep) {
                queue.push_back(dep.clone());
            }
        }
        visited.insert(h, node);
    }

    Ok(visited.into_values().collect())
}

pub fn check_strata_compatible(node: &CanonicalNode) -> Result<(), AxiomDiagnostic> {
    let f = &node.ast;
    for p in &f.params {
        if !matches!(p.ty, Type::I64 | Type::Bool | Type::Unit | Type::Str) {
            return Err(AxiomDiagnostic::new(
                "E085_STRATA_UNSUPPORTED_TYPE",
                "strata",
                format!(
                    "Node #{} uses high-level type '{}' not allowed in direct Strata silicon/Wasm mode",
                    node.short_hash, p.ty
                ),
                Span::zero(),
                "Strata direct machine/Wasm emitter operates on unboxed i64, bool, str, unit, and linear memory (Mem)",
            ));
        }
    }
    if !matches!(f.ret_type, Type::I64 | Type::Bool | Type::Unit | Type::Str) {
        return Err(AxiomDiagnostic::new(
            "E085_STRATA_UNSUPPORTED_TYPE",
            "strata",
            format!(
                "Node #{} returns high-level type '{}' not allowed in direct Strata mode",
                node.short_hash, f.ret_type
            ),
            Span::zero(),
            "Return i64, bool, str, or unit from Strata functions",
        ));
    }
    Ok(())
}

const STRATA_STR_WORD_BASE: u32 = 2_030_000; // Byte offset 8,120,000 inside 8 MiB linear memory

struct StrataStrPool {
    map: BTreeMap<String, i64>,
    words: Vec<i32>,
}

impl StrataStrPool {
    fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            words: Vec::new(),
        }
    }

    fn intern(&mut self, s: &str) -> i64 {
        if let Some(&addr) = self.map.get(s) {
            return addr;
        }
        let word_addr = (STRATA_STR_WORD_BASE as usize + self.words.len()) as i64;
        let bytes = s.as_bytes();
        self.words.push(bytes.len() as i32);
        for &b in bytes {
            self.words.push(b as i32);
        }
        self.map.insert(s.to_string(), word_addr);
        word_addr
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.words.len() * 4);
        for &w in &self.words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }
}

fn emit_strata_wasm_bytes(
    nodes: &[CanonicalNode],
    exports: &[(String, String)],
    passport_json: &[u8],
) -> Result<Vec<u8>, AxiomDiagnostic> {
    let mut hash_to_func_idx: BTreeMap<String, u32> = BTreeMap::new();
    for (idx, node) in nodes.iter().enumerate() {
        hash_to_func_idx.insert(node.hash.clone(), idx as u32);
    }

    // Internal functions `0 .. nodes.len()-1` all use uniform i64 ABI: (i64 * arity) -> i64.
    // For each exported function, we ALSO emit a JS-ergonomic i32-marshaled wrapper if desired,
    // OR export both `<name>` (i32 params -> i32 return for zero-BigInt JS canvas loops!)
    // and `<name>_i64` (full 64-bit ABI)!
    // WAIT: Exporting `<name>` with `(i32 * arity) -> i32` (sign-extending i32 args to i64 and wrapping i64 result to i32)
    // lets 60-120 FPS browser games call `wasm.tick(keys, mx, my)` directly with plain JS numbers (zero BigInt GC overhead!),
    // while `<name>_i64` provides full 64-bit access!
    let internal_count = nodes.len() as u32;

    let mut type_entries: Vec<Vec<u8>> = Vec::new();
    let mut func_type_indices: Vec<u32> = Vec::new();

    // 1. Internal functions (uniform i64 -> i64)
    for node in nodes {
        let arity = node.ast.params.len();
        let mut sig = Vec::with_capacity(4 + arity);
        sig.push(0x60); // func
        push_u32_leb(&mut sig, arity as u32);
        sig.extend(std::iter::repeat_n(WASM_TYPE_I64, arity));
        push_u32_leb(&mut sig, 1);
        sig.push(WASM_TYPE_I64);

        let tidx = intern_sig(&mut type_entries, sig);
        func_type_indices.push(tidx);
    }

    // 2. JS-friendly i32 wrapper functions for each exported entry (`<name>`)
    let mut wrapper_specs: Vec<(String, u32, usize, bool)> = Vec::new();
    for (exp_name, target_hash) in exports {
        let target_idx = *hash_to_func_idx.get(target_hash).ok_or_else(|| {
            AxiomDiagnostic::new(
                "E032_NODE_NOT_FOUND",
                "strata-wasm",
                format!("Missing target hash #{target_hash} in Strata module"),
                Span::zero(),
                "Ensure exported node is ingested",
            )
        })?;
        let target_node = &nodes[target_idx as usize];
        let arity = target_node.ast.params.len();
        let is_unit = target_node.ast.ret_type == Type::Unit;

        let mut sig = Vec::with_capacity(4 + arity);
        sig.push(0x60);
        push_u32_leb(&mut sig, arity as u32);
        sig.extend(std::iter::repeat_n(WASM_TYPE_I32, arity));
        if is_unit {
            push_u32_leb(&mut sig, 0);
        } else {
            push_u32_leb(&mut sig, 1);
            sig.push(WASM_TYPE_I32);
        }

        let tidx = intern_sig(&mut type_entries, sig);
        let wrapper_func_idx = internal_count + wrapper_specs.len() as u32;
        func_type_indices.push(tidx);
        wrapper_specs.push((exp_name.clone(), target_idx, arity, is_unit));
        let _ = wrapper_func_idx;
    }

    let mut wasm = Vec::with_capacity(4096);
    // Wasm 1.0 header
    wasm.extend_from_slice(b"\x00asm\x01\x00\x00\x00");

    // Section 1: Type section
    let mut sec_type = Vec::new();
    push_u32_leb(&mut sec_type, type_entries.len() as u32);
    for sig in &type_entries {
        sec_type.extend_from_slice(sig);
    }
    push_section(&mut wasm, 1, &sec_type);

    // Section 3: Function section
    let mut sec_func = Vec::new();
    push_u32_leb(&mut sec_func, func_type_indices.len() as u32);
    for &tidx in &func_type_indices {
        push_u32_leb(&mut sec_func, tidx);
    }
    push_section(&mut wasm, 3, &sec_func);

    // Section 5: Linear Memory section (128 pages = 8 MiB: state + 256x256 textures + 3D vertex buffers)
    let mut sec_mem = Vec::new();
    push_u32_leb(&mut sec_mem, 1); // 1 memory
    sec_mem.push(0x00); // flags: min only
    push_u32_leb(&mut sec_mem, 128); // 128 * 64 KiB = 8 MiB
    push_section(&mut wasm, 5, &sec_mem);

    // Section 7: Export section (`memory` + `<name>` i32 wrapper + `<name>_i64` raw i64 fn)
    let mut sec_exp = Vec::new();
    let total_exports = 1 + (wrapper_specs.len() * 2);
    push_u32_leb(&mut sec_exp, total_exports as u32);

    // Export linear memory as "memory"
    push_wasm_str(&mut sec_exp, "memory");
    sec_exp.push(0x02); // memory export
    push_u32_leb(&mut sec_exp, 0);

    for (i, (exp_name, target_idx, _, _)) in wrapper_specs.iter().enumerate() {
        let wrapper_idx = internal_count + i as u32;
        push_wasm_str(&mut sec_exp, exp_name);
        sec_exp.push(0x00); // func export
        push_u32_leb(&mut sec_exp, wrapper_idx);

        let i64_name = format!("{exp_name}_i64");
        push_wasm_str(&mut sec_exp, &i64_name);
        sec_exp.push(0x00);
        push_u32_leb(&mut sec_exp, *target_idx);
    }
    push_section(&mut wasm, 7, &sec_exp);

    // Section 10: Code section (simultaneously interns static StrLit constants into StrPool)
    let mut str_pool = StrataStrPool::new();
    let mut sec_code = Vec::new();
    push_u32_leb(&mut sec_code, func_type_indices.len() as u32);

    for (self_idx, node) in nodes.iter().enumerate() {
        let body_bytes =
            compile_node_body(node, self_idx as u32, &hash_to_func_idx, &mut str_pool)?;
        push_u32_leb(&mut sec_code, body_bytes.len() as u32);
        sec_code.extend_from_slice(&body_bytes);
    }

    // Emit thin i32 -> i64 -> i32 wrapper bodies
    for (_, target_idx, arity, is_unit) in &wrapper_specs {
        let mut wbody = Vec::new();
        push_u32_leb(&mut wbody, 0); // 0 additional locals
        for p in 0..*arity {
            wbody.push(0x20); // local.get p
            push_u32_leb(&mut wbody, p as u32);
            wbody.push(0xAC); // i64.extend_i32_s (MUST sign-extend negative i32 args like -1!)
        }
        wbody.push(0x10); // call target_idx
        push_u32_leb(&mut wbody, *target_idx);
        if *is_unit {
            wbody.push(0x1A); // drop
        } else {
            wbody.push(0xA7); // i32.wrap_i64
        }
        wbody.push(0x0B); // end

        push_u32_leb(&mut sec_code, wbody.len() as u32);
        sec_code.extend_from_slice(&wbody);
    }
    push_section(&mut wasm, 10, &sec_code);

    // Section 11: Data section (static read-only string pool at word base 2,030,000 = byte 8,120,000)
    if !str_pool.words.is_empty() {
        let raw_str_bytes = str_pool.to_le_bytes();
        let mut sec_data = Vec::with_capacity(16 + raw_str_bytes.len());
        push_u32_leb(&mut sec_data, 1); // 1 active data segment
        sec_data.push(0x00); // active memory 0
        sec_data.push(0x41); // i32.const
        push_i32_leb(&mut sec_data, (STRATA_STR_WORD_BASE * 4) as i32);
        sec_data.push(0x0B); // end const expr
        push_u32_leb(&mut sec_data, raw_str_bytes.len() as u32);
        sec_data.extend_from_slice(&raw_str_bytes);
        push_section(&mut wasm, 11, &sec_data);
    }

    // Custom Section 0: "axiom.passport" (embedded compile-time SecurityPassport JSON)
    if !passport_json.is_empty() {
        let mut sec_custom = Vec::new();
        push_wasm_str(&mut sec_custom, "axiom.passport");
        sec_custom.extend_from_slice(passport_json);
        push_section(&mut wasm, 0, &sec_custom);
    }

    Ok(wasm)
}

struct FnCodegenCtx<'a> {
    self_idx: u32,
    hash_to_func_idx: &'a BTreeMap<String, u32>,
    str_pool: &'a mut StrataStrPool,
    local_map: BTreeMap<String, u32>,
    next_local_idx: u32,
    extra_i64_locals: u32,
}

impl<'a> FnCodegenCtx<'a> {
    fn alloc_local(&mut self, name: Option<&str>) -> u32 {
        let idx = self.next_local_idx;
        self.next_local_idx += 1;
        self.extra_i64_locals += 1;
        if let Some(n) = name {
            self.local_map.insert(n.to_string(), idx);
        }
        idx
    }
}

fn compile_node_body(
    node: &CanonicalNode,
    self_idx: u32,
    hash_to_func_idx: &BTreeMap<String, u32>,
    str_pool: &mut StrataStrPool,
) -> Result<Vec<u8>, AxiomDiagnostic> {
    let f: &FnDef = &node.ast;
    let mut local_map = BTreeMap::new();
    for (i, p) in f.params.iter().enumerate() {
        local_map.insert(p.name.clone(), i as u32);
    }

    let mut ctx = FnCodegenCtx {
        self_idx,
        hash_to_func_idx,
        str_pool,
        local_map,
        next_local_idx: f.params.len() as u32,
        extra_i64_locals: 0,
    };

    // Pre-allocate all named `let` locals across the body
    prealloc_expr_locals(&f.body, &mut ctx);

    let mut instrs = Vec::with_capacity(256);
    emit_expr(&f.body, &mut ctx, &mut instrs)?;
    instrs.push(0x0B); // end of function

    let mut body = Vec::with_capacity(8 + instrs.len());
    if ctx.extra_i64_locals == 0 {
        push_u32_leb(&mut body, 0);
    } else {
        push_u32_leb(&mut body, 1); // 1 local declaration group
        push_u32_leb(&mut body, ctx.extra_i64_locals);
        body.push(WASM_TYPE_I64);
    }
    body.extend_from_slice(&instrs);
    Ok(body)
}

fn prealloc_stmt_locals(stmts: &[Stmt], ctx: &mut FnCodegenCtx<'_>) {
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                prealloc_expr_locals(value, ctx);
                if !ctx.local_map.contains_key(name) {
                    ctx.alloc_local(Some(name));
                }
            }
            Stmt::Assign { value, .. } => prealloc_expr_locals(value, ctx),
            Stmt::While { cond, body, .. } => {
                prealloc_expr_locals(cond, ctx);
                prealloc_stmt_locals(body, ctx);
            }
            Stmt::Expr(e) => prealloc_expr_locals(e, ctx),
        }
    }
}

fn prealloc_expr_locals(expr: &Expr, ctx: &mut FnCodegenCtx<'_>) {
    match expr {
        Expr::Block { stmts, tail } => {
            prealloc_stmt_locals(stmts, ctx);
            prealloc_expr_locals(tail, ctx);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            prealloc_expr_locals(cond, ctx);
            prealloc_expr_locals(then_branch, ctx);
            prealloc_expr_locals(else_branch, ctx);
        }
        Expr::Unary { expr: inner, .. }
        | Expr::MemLoad { index: inner }
        | Expr::SecretWrap(inner)
        | Expr::Reveal { expr: inner, .. } => prealloc_expr_locals(inner, ctx),
        Expr::MemStore { index, value } => {
            prealloc_expr_locals(index, ctx);
            prealloc_expr_locals(value, ctx);
        }
        Expr::Binary { lhs, rhs, .. } => {
            prealloc_expr_locals(lhs, ctx);
            prealloc_expr_locals(rhs, ctx);
        }
        Expr::Call { args, .. } | Expr::Perform { args, .. } => {
            for a in args {
                prealloc_expr_locals(a, ctx);
            }
        }
        Expr::IntLit(_) | Expr::BoolLit(_) | Expr::StrLit(_) | Expr::UnitLit | Expr::Var(_) => {}
    }
}

fn emit_stmts(
    stmts: &[Stmt],
    ctx: &mut FnCodegenCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), AxiomDiagnostic> {
    for s in stmts {
        match s {
            Stmt::Let { name, value, .. } | Stmt::Assign { name, value } => {
                emit_expr(value, ctx, out)?;
                let lidx = *ctx.local_map.get(name).ok_or_else(|| {
                    AxiomDiagnostic::new(
                        "E020_UNKNOWN_VAR",
                        "strata-wasm",
                        format!("Missing local slot for '{name}'"),
                        Span::zero(),
                        "Internal local allocation mismatch",
                    )
                })?;
                out.push(0x21); // local.set
                push_u32_leb(out, lidx);
            }
            Stmt::While {
                cond,
                max_iters,
                body,
            } => {
                let fuel_local = ctx.alloc_local(None);
                // fuel = max_iters
                out.push(0x42); // i64.const
                push_i64_leb(out, *max_iters as i64);
                out.push(0x21); // local.set $fuel
                push_u32_leb(out, fuel_local);

                out.push(0x02); // block $exit
                out.push(WASM_BLOCK_VOID);
                out.push(0x03); // loop $cont
                out.push(WASM_BLOCK_VOID);

                // if fuel <= 0: br 1 ($exit)
                out.push(0x20); // local.get $fuel
                push_u32_leb(out, fuel_local);
                out.push(0x42); // i64.const 0
                push_i64_leb(out, 0);
                out.push(0x57); // i64.le_s -> i32
                out.push(0x0D); // br_if 1
                push_u32_leb(out, 1);

                // if !cond: br 1 ($exit)
                emit_expr(cond, ctx, out)?;
                out.push(0x50); // i64.eqz -> i32 (1 when cond == 0)
                out.push(0x0D); // br_if 1
                push_u32_leb(out, 1);

                // fuel = fuel - 1
                out.push(0x20);
                push_u32_leb(out, fuel_local);
                out.push(0x42);
                push_i64_leb(out, 1);
                out.push(0x7D); // i64.sub
                out.push(0x21);
                push_u32_leb(out, fuel_local);

                emit_stmts(body, ctx, out)?;

                out.push(0x0C); // br 0 ($cont)
                push_u32_leb(out, 0);

                out.push(0x0B); // end loop
                out.push(0x0B); // end block
            }
            Stmt::Expr(e) => {
                emit_expr(e, ctx, out)?;
                out.push(0x1A); // drop unused expression result
            }
        }
    }
    Ok(())
}

fn emit_safe_mem_addr_i32(
    index_expr: &Expr,
    ctx: &mut FnCodegenCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), AxiomDiagnostic> {
    emit_expr(index_expr, ctx, out)?;
    out.push(0xA7); // i32.wrap_i64
    out.push(0x41); // i32.const 0x1FFFFF (2,097,151 words = 8 MiB)
    push_i32_leb(out, 0x001F_FFFF);
    out.push(0x71); // i32.and
    out.push(0x41); // i32.const 2
    push_i32_leb(out, 2);
    out.push(0x74); // i32.shl (word_idx * 4)
    Ok(())
}

fn emit_expr(
    expr: &Expr,
    ctx: &mut FnCodegenCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), AxiomDiagnostic> {
    match expr {
        Expr::IntLit(v) => {
            out.push(0x42); // i64.const
            push_i64_leb(out, *v);
        }
        Expr::BoolLit(b) => {
            out.push(0x42);
            push_i64_leb(out, if *b { 1 } else { 0 });
        }
        Expr::UnitLit => {
            out.push(0x42);
            push_i64_leb(out, 0);
        }
        Expr::StrLit(s) => {
            let word_addr = ctx.str_pool.intern(s);
            out.push(0x42); // i64.const <word_addr>
            push_i64_leb(out, word_addr);
        }
        Expr::SecretWrap(_) | Expr::Reveal { .. } => {
            return Err(AxiomDiagnostic::new(
                "E085_STRATA_UNSUPPORTED_EXPR",
                "strata-wasm",
                "Secret expressions belong to Axiom.Flux, not low-level Axiom.Strata kernels",
                Span::zero(),
                "Use i64, bool, str, unit, and linear memory inside Strata kernels",
            ));
        }
        Expr::Var(name) => {
            let lidx = *ctx.local_map.get(name).ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E020_UNKNOWN_VAR",
                    "strata-wasm",
                    format!("Unknown local '{name}'"),
                    Span::zero(),
                    "Ensure variable is bound",
                )
            })?;
            out.push(0x20); // local.get
            push_u32_leb(out, lidx);
        }
        Expr::Unary { op, expr: inner } => match op {
            UnaryOp::Neg => {
                out.push(0x42);
                push_i64_leb(out, 0);
                emit_expr(inner, ctx, out)?;
                out.push(0x7D); // 0 - inner
            }
            UnaryOp::Not => {
                emit_expr(inner, ctx, out)?;
                out.push(0x50); // i64.eqz -> i32 (0 or 1)
                out.push(0xAD); // i64.extend_i32_u -> i64
            }
        },
        Expr::Binary { op, lhs, rhs } => match op {
            BinOp::And => {
                emit_expr(lhs, ctx, out)?;
                out.push(0xA7); // i32.wrap_i64
                out.push(0x04); // if (result i64)
                out.push(WASM_BLOCK_I64);
                emit_expr(rhs, ctx, out)?;
                out.push(0x05); // else
                out.push(0x42);
                push_i64_leb(out, 0);
                out.push(0x0B); // end
            }
            BinOp::Or => {
                emit_expr(lhs, ctx, out)?;
                out.push(0xA7); // i32.wrap_i64
                out.push(0x04); // if (result i64)
                out.push(WASM_BLOCK_I64);
                out.push(0x42);
                push_i64_leb(out, 1);
                out.push(0x05); // else
                emit_expr(rhs, ctx, out)?;
                out.push(0x0B); // end
            }
            _ => {
                emit_expr(lhs, ctx, out)?;
                emit_expr(rhs, ctx, out)?;
                match op {
                    BinOp::Add => out.push(0x7C),
                    BinOp::Sub => out.push(0x7D),
                    BinOp::Mul => out.push(0x7E),
                    BinOp::Div => out.push(0x7F),
                    BinOp::Mod => out.push(0x81),
                    BinOp::BitAnd => out.push(0x83),
                    BinOp::BitOr => out.push(0x84),
                    BinOp::BitXor => out.push(0x85),
                    BinOp::Shl => out.push(0x86),
                    BinOp::Shr => out.push(0x87),
                    BinOp::Eq => {
                        out.push(0x51);
                        out.push(0xAC);
                    }
                    BinOp::Ne => {
                        out.push(0x52);
                        out.push(0xAC);
                    }
                    BinOp::Lt => {
                        out.push(0x53);
                        out.push(0xAC);
                    }
                    BinOp::Gt => {
                        out.push(0x55);
                        out.push(0xAC);
                    }
                    BinOp::Le => {
                        out.push(0x57);
                        out.push(0xAC);
                    }
                    BinOp::Ge => {
                        out.push(0x59);
                        out.push(0xAC);
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                }
            }
        },
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            emit_expr(cond, ctx, out)?;
            out.push(0xA7); // i32.wrap_i64
            out.push(0x04); // if (result i64)
            out.push(WASM_BLOCK_I64);
            emit_expr(then_branch, ctx, out)?;
            out.push(0x05); // else
            emit_expr(else_branch, ctx, out)?;
            out.push(0x0B); // end
        }
        Expr::MemLoad { index } => {
            emit_safe_mem_addr_i32(index, ctx, out)?;
            out.push(0x34); // i64.load32_s
            out.push(0x02); // align = 4 bytes
            out.push(0x00); // offset = 0
        }
        Expr::MemStore { index, value } => {
            emit_safe_mem_addr_i32(index, ctx, out)?;
            emit_expr(value, ctx, out)?;
            out.push(0x3E); // i64.store32
            out.push(0x02); // align = 4 bytes
            out.push(0x00); // offset = 0
            // Push unit value `0_i64`
            out.push(0x42);
            push_i64_leb(out, 0);
        }
        Expr::Perform { effect, op, args } => {
            if effect == "Mem" && op == "store_str" && args.len() == 2 {
                // Zero-copy O(1) static string binding: stores word address of `str` into `mem[slot]`!
                emit_safe_mem_addr_i32(&args[0], ctx, out)?;
                emit_expr(&args[1], ctx, out)?;
                out.push(0x3E); // i64.store32
                out.push(0x02);
                out.push(0x00);
                out.push(0x42);
                push_i64_leb(out, 0);
            } else {
                for a in args {
                    emit_expr(a, ctx, out)?;
                    out.push(0x1A); // drop
                }
                out.push(0x42);
                push_i64_leb(out, 0);
            }
        }
        Expr::Call { target, args } => {
            for a in args {
                emit_expr(a, ctx, out)?;
            }
            let fidx = match target {
                TargetRef::SelfRec => ctx.self_idx,
                TargetRef::Hash(h) => *ctx.hash_to_func_idx.get(h).ok_or_else(|| {
                    AxiomDiagnostic::new(
                        "E032_NODE_NOT_FOUND",
                        "strata-wasm",
                        format!("Callee #{h} missing in subgraph"),
                        Span::zero(),
                        "Ingest callee first",
                    )
                })?,
                TargetRef::Named(n) => {
                    return Err(AxiomDiagnostic::new(
                        "E055_UNRESOLVED_CALL",
                        "strata-wasm",
                        format!("Unresolved call '{n}'"),
                        Span::zero(),
                        "Canonicalize before emitting Wasm",
                    ))
                }
            };
            out.push(0x10); // call
            push_u32_leb(out, fidx);
        }
        Expr::Block { stmts, tail } => {
            emit_stmts(stmts, ctx, out)?;
            emit_expr(tail, ctx, out)?;
        }
    }
    Ok(())
}

fn intern_sig(entries: &mut Vec<Vec<u8>>, sig: Vec<u8>) -> u32 {
    if let Some(pos) = entries.iter().position(|e| e == &sig) {
        pos as u32
    } else {
        let idx = entries.len() as u32;
        entries.push(sig);
        idx
    }
}

fn push_section(wasm: &mut Vec<u8>, id: u8, payload: &[u8]) {
    wasm.push(id);
    push_u32_leb(wasm, payload.len() as u32);
    wasm.extend_from_slice(payload);
}

fn push_wasm_str(out: &mut Vec<u8>, s: &str) {
    push_u32_leb(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

fn push_u32_leb(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

fn push_i32_leb(out: &mut Vec<u8>, mut val: i32) {
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        let done = (val == 0 && (byte & 0x40) == 0) || (val == -1 && (byte & 0x40) != 0);
        if done {
            out.push(byte);
            break;
        } else {
            out.push(byte | 0x80);
        }
    }
}

fn push_i64_leb(out: &mut Vec<u8>, mut val: i64) {
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        let done = (val == 0 && (byte & 0x40) == 0) || (val == -1 && (byte & 0x40) != 0);
        if done {
            out.push(byte);
            break;
        } else {
            out.push(byte | 0x80);
        }
    }
}
