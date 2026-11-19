use crate::ast::{Expr, FnDef, Param, Stmt, TargetRef, Type};
use crate::canon::CanonicalNode;
use crate::codegen::collect_reachable_subgraph;
use crate::diag::{AxiomDiagnostic, Span};
use crate::store::GraphStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmArtifact {
    pub entry_alias_or_ref: String,
    pub root_hash: String,
    pub short_hash: String,
    pub compiled_nodes: usize,
    pub wasm_path: String,
    pub wasm_size_bytes: u64,
    pub cache_hit: bool,
    pub build_ms: u128,
}

pub fn emit_wasm_rust_program(
    store: &GraphStore,
    target_ref: &str,
) -> Result<(String, CanonicalNode, usize), AxiomDiagnostic> {
    let root_hash = store.resolve_ref(target_ref)?;
    let root_node = store.load_node(&root_hash)?;
    let passport = store.compute_passport(target_ref)?;
    let passport_json = serde_json::to_string(&passport).unwrap_or_else(|_| "{}".to_string());

    let nodes = collect_reachable_subgraph(store, &root_hash)?;
    let node_count = nodes.len();

    let mut aliases_map: BTreeMap<String, String> = BTreeMap::new();
    for n in &nodes {
        let a = store.aliases_for_hash(&n.hash);
        let label = a.first().cloned().unwrap_or_else(|| format!("#{}", n.short_hash));
        aliases_map.insert(n.hash.clone(), label);
    }

    let mut rs = String::with_capacity(8192);
    rs.push_str("#![allow(unused_parens, unused_variables, dead_code, clippy::all)]\n");
    rs.push_str("use std::cell::RefCell;\nuse std::fmt;\n\n");

    rs.push_str(&format!(
        "const EMBEDDED_PASSPORT_JSON: &str = {:?};\n\n",
        passport_json
    ));

    // Embedded lightweight runtime + deterministic FlightRecorder + JSON serializer (zero external crates!)
    rs.push_str(
        r#"#[repr(transparent)]
#[derive(Clone)]
struct AxiomSecret<T>(T);

impl<T> AxiomSecret<T> {
    #[inline(always)]
    fn new(val: T) -> Self {
        Self(val)
    }

    #[inline(always)]
    fn expose_for_pinned_domain(&self, _domain: &'static str) -> &T {
        &self.0
    }
}

impl<T> fmt::Display for AxiomSecret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED SECRET]")
    }
}

trait AxiomFormat {
    fn axiom_display(&self) -> String;
}
impl AxiomFormat for i64 {
    fn axiom_display(&self) -> String { self.to_string() }
}
impl AxiomFormat for bool {
    fn axiom_display(&self) -> String { self.to_string() }
}
impl AxiomFormat for String {
    fn axiom_display(&self) -> String { self.clone() }
}
impl AxiomFormat for () {
    fn axiom_display(&self) -> String { "()".to_string() }
}
impl<T> AxiomFormat for AxiomSecret<T> {
    fn axiom_display(&self) -> String { "[REDACTED SECRET]".to_string() }
}

fn json_esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[derive(Clone)]
struct CallRecord {
    depth: usize,
    node: &'static str,
    alias: &'static str,
    args: Vec<String>,
    ret: String,
}

#[derive(Clone)]
struct EffectRecord {
    step: usize,
    node: &'static str,
    effect: &'static str,
    op: &'static str,
    args: Vec<String>,
}

#[derive(Clone)]
struct TimelineFrame {
    frame_id: usize,
    inputs: Vec<String>,
    result: String,
    calls: Vec<CallRecord>,
    effects: Vec<EffectRecord>,
}

impl TimelineFrame {
    fn to_json(&self, total_frames: usize) -> String {
        let inputs_j: Vec<String> = self.inputs.iter().map(|s| json_esc(s)).collect();
        let calls_j: Vec<String> = self.calls.iter().map(|c| {
            let aj: Vec<String> = c.args.iter().map(|a| json_esc(a)).collect();
            format!(
                "{{\"depth\":{},\"node\":{},\"alias\":{},\"args\":[{}],\"return\":{}}}",
                c.depth,
                json_esc(c.node),
                json_esc(c.alias),
                aj.join(","),
                json_esc(&c.ret)
            )
        }).collect();
        let effs_j: Vec<String> = self.effects.iter().map(|e| {
            let aj: Vec<String> = e.args.iter().map(|a| json_esc(a)).collect();
            format!(
                "{{\"step\":{},\"node\":{},\"effect\":{},\"op\":{},\"args\":[{}]}}",
                e.step,
                json_esc(e.node),
                json_esc(e.effect),
                json_esc(e.op),
                aj.join(",")
            )
        }).collect();

        format!(
            "{{\"status\":\"ok\",\"frame_id\":{},\"total_frames\":{},\"inputs\":[{}],\"result\":{},\"calls\":[{}],\"effects\":[{}]}}",
            self.frame_id,
            total_frames,
            inputs_j.join(","),
            json_esc(&self.result),
            calls_j.join(","),
            effs_j.join(",")
        )
    }
}

struct ExecTrace {
    depth: usize,
    calls: Vec<CallRecord>,
    effects: Vec<EffectRecord>,
}

impl ExecTrace {
    fn new() -> Self {
        Self {
            depth: 0,
            calls: Vec::new(),
            effects: Vec::new(),
        }
    }

    fn perform(&mut self, node: &'static str, effect: &'static str, op: &'static str, args: Vec<String>) {
        let step = self.effects.len();
        self.effects.push(EffectRecord {
            step,
            node,
            effect,
            op,
            args,
        });
    }
}

struct WasmHostState {
    input_buf: Vec<u8>,
    output_buf: Vec<u8>,
    frames: Vec<TimelineFrame>,
    cursor: usize,
}

impl WasmHostState {
    fn new() -> Self {
        Self {
            input_buf: Vec::new(),
            output_buf: Vec::new(),
            frames: Vec::new(),
            cursor: 0,
        }
    }

    fn set_output(&mut self, s: String) {
        self.output_buf = s.into_bytes();
    }

    fn set_error(&mut self, kind: &str, msg: &str) {
        let j = format!(
            "{{\"status\":\"error\",\"kind\":{},\"message\":{}}}",
            json_esc(kind),
            json_esc(msg)
        );
        self.set_output(j);
    }
}

thread_local! {
    static HOST: RefCell<WasmHostState> = RefCell::new(WasmHostState::new());
}

#[no_mangle]
pub extern "C" fn axiom_prepare_input(len: usize) -> *mut u8 {
    HOST.with(|h| {
        let mut st = h.borrow_mut();
        st.input_buf.clear();
        st.input_buf.resize(len, 0u8);
        st.input_buf.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn axiom_out_ptr() -> *const u8 {
    HOST.with(|h| h.borrow().output_buf.as_ptr())
}

#[no_mangle]
pub extern "C" fn axiom_out_len() -> usize {
    HOST.with(|h| h.borrow().output_buf.len())
}

#[no_mangle]
pub extern "C" fn axiom_frame_count() -> usize {
    HOST.with(|h| h.borrow().frames.len())
}

#[no_mangle]
pub extern "C" fn axiom_reset_timeline() {
    HOST.with(|h| {
        let mut st = h.borrow_mut();
        st.frames.clear();
        st.cursor = 0;
        st.set_output("{\"status\":\"ok\",\"total_frames\":0}".to_string());
    });
}

#[no_mangle]
pub extern "C" fn axiom_scrub_frame(frame_idx: usize) -> i32 {
    HOST.with(|h| {
        let mut st = h.borrow_mut();
        let total = st.frames.len();
        if frame_idx >= total {
            st.set_error("bounds", &format!("Frame index {} out of range (total {})", frame_idx, total));
            return 1;
        }
        st.cursor = frame_idx;
        let json = st.frames[frame_idx].to_json(total);
        st.set_output(json);
        0
    })
}

#[no_mangle]
pub extern "C" fn axiom_load_passport() -> usize {
    HOST.with(|h| {
        let mut st = h.borrow_mut();
        st.set_output(EMBEDDED_PASSPORT_JSON.to_string());
        st.output_buf.len()
    })
}

"#,
    );

    // Emit instrumented verified nodes for causal call-tree & effect recording
    for node in &nodes {
        let alias = aliases_map
            .get(&node.hash)
            .map(String::as_str)
            .unwrap_or("_");
        emit_wasm_node_fn(node, alias, &mut rs);
    }

    // Emit `axiom_step()` Wasm export for root entrypoint
    emit_wasm_step_export(&root_node, target_ref, &mut rs);

    Ok((rs, root_node, node_count))
}

fn rust_fn_ident(hash: &str) -> String {
    format!("ax_{}", &hash[..16])
}

fn rust_type(ty: &Type) -> String {
    match ty {
        Type::I64 => "i64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "String".to_string(),
        Type::Unit => "()".to_string(),
        Type::Secret(inner) => format!("AxiomSecret<{}>", rust_type(inner)),
    }
}

fn emit_wasm_node_fn(node: &CanonicalNode, alias: &str, out: &mut String) {
    let f: &FnDef = &node.ast;
    let fn_id = rust_fn_ident(&node.hash);
    let short = &node.short_hash;

    out.push_str("fn ");
    out.push_str(&fn_id);
    out.push_str("(tr: &mut ExecTrace");
    for p in &f.params {
        out.push_str(", ");
        out.push_str(&p.name);
        out.push_str(": ");
        out.push_str(&rust_type(&p.ty));
    }
    out.push_str(") -> ");
    out.push_str(&rust_type(&f.ret_type));
    out.push_str(" {\n");
    out.push_str("    let __call_depth = tr.depth;\n");
    out.push_str("    let __call_idx = tr.calls.len();\n");
    out.push_str("    tr.calls.push(CallRecord {\n");
    out.push_str("        depth: __call_depth,\n");
    out.push_str(&format!("        node: {short:?},\n"));
    out.push_str(&format!("        alias: {alias:?},\n"));
    out.push_str("        args: vec![");
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{}.axiom_display()", p.name));
    }
    out.push_str("],\n        ret: String::new(),\n    });\n");
    out.push_str("    tr.depth += 1;\n");
    out.push_str("    let __ret = ");
    emit_wasm_expr(&f.body, &node.hash, short, out);
    out.push_str(";\n");
    out.push_str("    tr.depth -= 1;\n");
    out.push_str("    tr.calls[__call_idx].ret = __ret.axiom_display();\n");
    out.push_str("    __ret\n}\n\n");
}

fn emit_wasm_expr(expr: &Expr, self_hash: &str, short_hash: &str, out: &mut String) {
    match expr {
        Expr::IntLit(v) => out.push_str(&format!("{v}_i64")),
        Expr::BoolLit(b) => out.push_str(if *b { "true" } else { "false" }),
        Expr::StrLit(s) => out.push_str(&format!("String::from({s:?})")),
        Expr::UnitLit => out.push_str("()"),
        Expr::Var(v) => out.push_str(&format!("{v}.clone()")),
        Expr::SecretWrap(inner) => {
            out.push_str("AxiomSecret::new(");
            emit_wasm_expr(inner, self_hash, short_hash, out);
            out.push(')');
        }
        Expr::Reveal { domain, expr: inner } => {
            out.push_str("{ let _ = (");
            emit_wasm_expr(inner, self_hash, short_hash, out);
            out.push_str(&format!(
                ").expose_for_pinned_domain({domain:?}); String::from(\"[PINNED SECRET -> {domain}]\") }}"
            ));
        }
        Expr::Unary { op, expr: inner } => {
            out.push('(');
            out.push_str(op.as_str());
            emit_wasm_expr(inner, self_hash, short_hash, out);
            out.push(')');
        }
        Expr::Binary { op, lhs, rhs } => {
            out.push('(');
            emit_wasm_expr(lhs, self_hash, short_hash, out);
            out.push(' ');
            out.push_str(op.as_str());
            out.push(' ');
            emit_wasm_expr(rhs, self_hash, short_hash, out);
            out.push(')');
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if ");
            emit_wasm_expr(cond, self_hash, short_hash, out);
            out.push_str(" { ");
            emit_wasm_expr(then_branch, self_hash, short_hash, out);
            out.push_str(" } else { ");
            emit_wasm_expr(else_branch, self_hash, short_hash, out);
            out.push_str(" }");
        }
        Expr::Call { target, args } => {
            let callee_id = match target {
                TargetRef::SelfRec => rust_fn_ident(self_hash),
                TargetRef::Hash(h) => rust_fn_ident(h),
                TargetRef::Named(n) => format!("ax_unresolved_{n}"),
            };
            out.push_str(&callee_id);
            out.push_str("(tr");
            for a in args {
                out.push_str(", ");
                emit_wasm_expr(a, self_hash, short_hash, out);
            }
            out.push(')');
        }
        Expr::Perform { effect, op, args } => {
            out.push_str(&format!(
                "{{ tr.perform({short_hash:?}, {effect:?}, {op:?}, vec!["
            ));
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if matches!(a, Expr::Reveal { .. }) {
                    emit_wasm_expr(a, self_hash, short_hash, out);
                } else {
                    out.push('(');
                    emit_wasm_expr(a, self_hash, short_hash, out);
                    out.push_str(").axiom_display()");
                }
            }
            out.push_str("]); () }");
        }
        Expr::Block { stmts, tail } => {
            out.push_str("{\n");
            for s in stmts {
                match s {
                    Stmt::Let { name, ty, value } => {
                        out.push_str("        let ");
                        out.push_str(name);
                        if let Some(t) = ty {
                            out.push_str(": ");
                            out.push_str(&rust_type(t));
                        }
                        out.push_str(" = ");
                        emit_wasm_expr(value, self_hash, short_hash, out);
                        out.push_str(";\n");
                    }
                    Stmt::Expr(e) => {
                        out.push_str("        let _ = ");
                        emit_wasm_expr(e, self_hash, short_hash, out);
                        out.push_str(";\n");
                    }
                }
            }
            out.push_str("        ");
            emit_wasm_expr(tail, self_hash, short_hash, out);
            out.push_str("\n    }");
        }
    }
}

fn emit_wasm_arg_parser(idx: usize, param: &Param, out: &mut String) {
    let pname = &param.name;
    out.push_str(&format!("        let {pname}: {} = ", rust_type(&param.ty)));
    emit_wasm_parse_val(&format!("lines[{idx}]"), &param.ty, idx + 1, out);
    out.push_str(";\n");
}

fn emit_wasm_parse_val(src_expr: &str, ty: &Type, arg_num: usize, out: &mut String) {
    match ty {
        Type::I64 => out.push_str(&format!(
            "match ({src_expr}).trim().parse::<i64>() {{ Ok(v) => v, Err(_) => {{ st.set_error(\"arg_parse\", &format!(\"Invalid i64 for argument #{arg_num}: '{{}}'\", {src_expr})); return 2; }} }}"
        )),
        Type::Bool => out.push_str(&format!(
            "match ({src_expr}).trim().parse::<bool>() {{ Ok(v) => v, Err(_) => {{ st.set_error(\"arg_parse\", &format!(\"Invalid bool for argument #{arg_num}: '{{}}'\", {src_expr})); return 2; }} }}"
        )),
        Type::Str => out.push_str(&format!("({src_expr}).to_string()")),
        Type::Unit => out.push_str("()"),
        Type::Secret(inner) => {
            out.push_str("AxiomSecret::new(");
            emit_wasm_parse_val(src_expr, inner, arg_num, out);
            out.push(')');
        }
    }
}

fn emit_wasm_step_export(root: &CanonicalNode, target_label: &str, out: &mut String) {
    let f = &root.ast;
    let arity = f.params.len();
    let fn_id = rust_fn_ident(&root.hash);

    out.push_str("#[no_mangle]\npub extern \"C\" fn axiom_step() -> i32 {\n");
    out.push_str("    HOST.with(|h| {\n");
    out.push_str("        let mut st = h.borrow_mut();\n");
    out.push_str("        let raw_in = String::from_utf8_lossy(&st.input_buf).to_string();\n");
    if arity == 0 {
        out.push_str("        let lines: Vec<&str> = Vec::new();\n");
    } else {
        out.push_str("        let lines: Vec<&str> = if raw_in.is_empty() { Vec::new() } else { raw_in.split('\\n').collect() };\n");
    }
    out.push_str(&format!(
        "        if lines.len() != {arity} {{\n            st.set_error(\"arity\", &format!(\"Expected {arity} argument(s) for '{target_label}', got {{}}\", lines.len()));\n            return 2;\n        }}\n"
    ));

    for (i, p) in f.params.iter().enumerate() {
        emit_wasm_arg_parser(i, p, out);
    }

    // Boundary precondition check (`req`) before recording a valid timeline step
    for (i, req_expr) in f.requires.iter().enumerate() {
        let req_desc = crate::canon::format_expr_human(req_expr);
        out.push_str("        if !(");
        emit_wasm_expr(req_expr, &root.hash, &root.short_hash, out);
        out.push_str(&format!(
            ") {{\n            st.set_error(\"precondition\", &format!(\"Boundary Precondition Violation: req #{} ({{}}) failed\", {req_desc:?}));\n            return 2;\n        }}\n",
            i + 1
        ));
    }

    out.push_str("        let redacted_inputs: Vec<String> = vec![");
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{}.axiom_display()", p.name));
    }
    out.push_str("];\n");

    out.push_str("        let mut tr = ExecTrace::new();\n");
    out.push_str(&format!("        let res = {fn_id}(&mut tr"));
    for p in &f.params {
        out.push_str(", ");
        out.push_str(&p.name);
    }
    out.push_str(");\n");

    out.push_str("        let frame_id = st.frames.len();\n");
    out.push_str("        let frame = TimelineFrame {\n");
    out.push_str("            frame_id,\n");
    out.push_str("            inputs: redacted_inputs,\n");
    out.push_str("            result: res.axiom_display(),\n");
    out.push_str("            calls: tr.calls,\n");
    out.push_str("            effects: tr.effects,\n");
    out.push_str("        };\n");
    out.push_str("        st.frames.push(frame);\n");
    out.push_str("        st.cursor = frame_id;\n");
    out.push_str("        let total = st.frames.len();\n");
    out.push_str("        let json = st.frames[frame_id].to_json(total);\n");
    out.push_str("        st.set_output(json);\n");
    out.push_str("        0\n");
    out.push_str("    })\n}\n");
}

pub fn build_wasm_module(
    store: &GraphStore,
    workspace_root: &Path,
    target_ref: &str,
    explicit_out: Option<&Path>,
) -> Result<WasmArtifact, AxiomDiagnostic> {
    let t0 = Instant::now();
    let (rust_src, root_node, compiled_nodes) = emit_wasm_rust_program(store, target_ref)?;

    let cache_dir = workspace_root.join(".axiom").join("wasm");
    fs::create_dir_all(&cache_dir).map_err(|e| {
        AxiomDiagnostic::new(
            "E080_WASM_IO",
            "wasm",
            format!("Cannot create .axiom/wasm directory: {e}"),
            Span::zero(),
            "Check write permissions in workspace",
        )
    })?;

    let cached_wasm = cache_dir.join(format!("{}.wasm", root_node.hash));
    let cache_hit = cached_wasm.is_file();

    if !cache_hit {
        let src_path = cache_dir.join(format!("{}_wasm.rs", root_node.short_hash));
        fs::write(&src_path, &rust_src).map_err(|e| {
            AxiomDiagnostic::new(
                "E080_WASM_IO",
                "wasm",
                format!("Failed to write Wasm intermediate source: {e}"),
                Span::zero(),
                "Check disk space",
            )
        })?;

        let rustc = find_rustc_binary();
        let output = Command::new(&rustc)
            .args([
                "--edition=2021",
                "--target=wasm32-unknown-unknown",
                "--crate-type=cdylib",
                "-C",
                "opt-level=z",
                "-C",
                "lto=yes",
                "-C",
                "strip=symbols",
                "-C",
                "panic=abort",
                src_path.to_str().unwrap_or_default(),
                "-o",
                cached_wasm.to_str().unwrap_or_default(),
            ])
            .output()
            .map_err(|e| {
                AxiomDiagnostic::new(
                    "E081_WASM_RUSTC_EXEC",
                    "wasm",
                    format!("Failed to invoke rustc for wasm32: {e}"),
                    Span::zero(),
                    "Ensure wasm32-unknown-unknown target is installed via 'rustup target add wasm32-unknown-unknown'",
                )
            })?;

        let _ = fs::remove_file(&src_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AxiomDiagnostic::new(
                "E082_WASM_COMPILE_FAILED",
                "wasm",
                format!("WebAssembly compilation failed: {}", stderr.trim()),
                Span::zero(),
                "Inspect Wasm target installation and generated types",
            ));
        }
    }

    let final_path = if let Some(out_p) = explicit_out {
        if let Some(parent) = out_p.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        fs::copy(&cached_wasm, out_p).map_err(|e| {
            AxiomDiagnostic::new(
                "E080_WASM_IO",
                "wasm",
                format!("Failed to copy .wasm to '{}': {e}", out_p.display()),
                Span::zero(),
                "Check output path permissions",
            )
        })?;
        out_p.to_path_buf()
    } else {
        cached_wasm
    };

    let size = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

    Ok(WasmArtifact {
        entry_alias_or_ref: target_ref.to_string(),
        root_hash: root_node.hash,
        short_hash: root_node.short_hash,
        compiled_nodes,
        wasm_path: final_path.display().to_string(),
        wasm_size_bytes: size,
        cache_hit,
        build_ms: t0.elapsed().as_millis(),
    })
}

fn find_rustc_binary() -> PathBuf {
    if let Ok(custom) = env::var("RUSTC") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return p;
        }
    }
    if let Some(home) = env::var_os("HOME") {
        let cargo_rustc = PathBuf::from(home).join(".cargo/bin/rustc");
        if cargo_rustc.is_file() {
            return cargo_rustc;
        }
    }
    PathBuf::from("rustc")
}
