use crate::ast::{BinOp, Expr, FnDef, Param, Stmt, TargetRef, Type, UnaryOp};
use crate::canon::CanonicalNode;
use crate::diag::{AxiomDiagnostic, Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub alias: String,
    pub short_hash: String,
    pub obligations_checked: usize,
    pub elapsed_ms: u128,
    pub status: String,
}

#[derive(Debug, Clone)]
enum ObligationKind {
    Postcondition { index: usize, display: String },
    CallPrecondition { callee_desc: String, req_index: usize },
    NonZeroDivisor,
}

#[derive(Debug, Clone)]
struct ProofObligation {
    guard_smt: String,
    goal_smt: String,
    kind: ObligationKind,
}

struct SmtBuilder<'a, F>
where
    F: Fn(&str) -> Result<CanonicalNode, AxiomDiagnostic>,
{
    self_node: &'a CanonicalNode,
    load_callee: &'a F,
    decls: Vec<String>,
    assumptions: Vec<String>,
    obligations: Vec<ProofObligation>,
    fresh_counter: usize,
}

impl<'a, F> SmtBuilder<'a, F>
where
    F: Fn(&str) -> Result<CanonicalNode, AxiomDiagnostic>,
{
    fn new(self_node: &'a CanonicalNode, load_callee: &'a F) -> Self {
        Self {
            self_node,
            load_callee,
            decls: Vec::new(),
            assumptions: Vec::new(),
            obligations: Vec::new(),
            fresh_counter: 0,
        }
    }

    fn fresh_sym(&mut self, prefix: &str, ty: &Type) -> String {
        let sym = format!("{prefix}_{}", self.fresh_counter);
        self.fresh_counter += 1;
        self.decls
            .push(format!("(declare-const {sym} {})", smt_sort(ty)));
        sym
    }

    fn encode_expr(
        &mut self,
        expr: &Expr,
        env: &BTreeMap<String, String>,
        guard: &str,
        emit_checks: bool,
    ) -> Result<String, AxiomDiagnostic> {
        match expr {
            Expr::IntLit(v) => {
                if *v < 0 {
                    Ok(format!("(- {})", v.unsigned_abs()))
                } else {
                    Ok(v.to_string())
                }
            }
            Expr::BoolLit(b) => Ok(if *b { "true" } else { "false" }.to_string()),
            Expr::StrLit(s) => Ok(format!("\"{}\"", smt_escape_str(s))),
            Expr::UnitLit => Ok("true".to_string()),
            Expr::Var(name) => env.get(name).cloned().ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E054_SMT_UNBOUND_VAR",
                    "verify",
                    format!("Unbound canonical symbol '{name}' during Z3 encoding"),
                    Span::zero(),
                    "Ensure all variables in contracts and body are bound parameters or locals",
                )
            }),
            Expr::SecretWrap(inner) | Expr::Reveal { expr: inner, .. } => {
                self.encode_expr(inner, env, guard, emit_checks)
            }
            Expr::Unary { op, expr: inner } => {
                let sub = self.encode_expr(inner, env, guard, emit_checks)?;
                match op {
                    UnaryOp::Neg => Ok(format!("(- {sub})")),
                    UnaryOp::Not => Ok(format!("(not {sub})")),
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let l = self.encode_expr(lhs, env, guard, emit_checks)?;
                match op {
                    BinOp::And => {
                        let rhs_guard = conjoin_guard(guard, &l);
                        let r = self.encode_expr(rhs, env, &rhs_guard, emit_checks)?;
                        Ok(format!("(and {l} {r})"))
                    }
                    BinOp::Or => {
                        let not_l = format!("(not {l})");
                        let rhs_guard = conjoin_guard(guard, &not_l);
                        let r = self.encode_expr(rhs, env, &rhs_guard, emit_checks)?;
                        Ok(format!("(or {l} {r})"))
                    }
                    BinOp::Div | BinOp::Mod => {
                        let r = self.encode_expr(rhs, env, guard, emit_checks)?;
                        if emit_checks {
                            self.obligations.push(ProofObligation {
                                guard_smt: guard.to_string(),
                                goal_smt: format!("(distinct {r} 0)"),
                                kind: ObligationKind::NonZeroDivisor,
                            });
                        }
                        let fn_name = if matches!(op, BinOp::Div) {
                            "ax_div"
                        } else {
                            "ax_mod"
                        };
                        Ok(format!("({fn_name} {l} {r})"))
                    }
                    other => {
                        let r = self.encode_expr(rhs, env, guard, emit_checks)?;
                        let smt_op = match other {
                            BinOp::Add => "+",
                            BinOp::Sub => "-",
                            BinOp::Mul => "*",
                            BinOp::Eq => "=",
                            BinOp::Ne => "distinct",
                            BinOp::Lt => "<",
                            BinOp::Le => "<=",
                            BinOp::Gt => ">",
                            BinOp::Ge => ">=",
                            BinOp::Div | BinOp::Mod | BinOp::And | BinOp::Or => unreachable!(),
                        };
                        Ok(format!("({smt_op} {l} {r})"))
                    }
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.encode_expr(cond, env, guard, emit_checks)?;
                let then_guard = conjoin_guard(guard, &c);
                let not_c = format!("(not {c})");
                let else_guard = conjoin_guard(guard, &not_c);
                let t = self.encode_expr(then_branch, env, &then_guard, emit_checks)?;
                let e = self.encode_expr(else_branch, env, &else_guard, emit_checks)?;
                Ok(format!("(ite {c} {t} {e})"))
            }
            Expr::Perform { args, .. } => {
                for a in args {
                    let _ = self.encode_expr(a, env, guard, emit_checks)?;
                }
                Ok("true".to_string())
            }
            Expr::Call { target, args } => {
                let mut encoded_args = Vec::with_capacity(args.len());
                for a in args {
                    encoded_args.push(self.encode_expr(a, env, guard, emit_checks)?);
                }

                let callee_node: CanonicalNode;
                let (callee_ast, callee_label): (&FnDef, String) = match target {
                    TargetRef::SelfRec => (&self.self_node.ast, "$self".to_string()),
                    TargetRef::Hash(h) => {
                        callee_node = (self.load_callee)(h)?;
                        let short = callee_node.short_hash.clone();
                        (&callee_node.ast, format!("#{short}"))
                    }
                    TargetRef::Named(n) => {
                        return Err(AxiomDiagnostic::new(
                            "E055_UNRESOLVED_CALL",
                            "verify",
                            format!("Unresolved symbolic call '{n}' reached Z3 verifier"),
                            Span::zero(),
                            "Canonicalize function references to hashes before verification",
                        ));
                    }
                };

                if encoded_args.len() != callee_ast.params.len() {
                    return Err(AxiomDiagnostic::new(
                        "E056_ARITY_MISMATCH",
                        "verify",
                        format!(
                            "Call to {callee_label} passed {} args, expected {}",
                            encoded_args.len(),
                            callee_ast.params.len()
                        ),
                        Span::zero(),
                        "Pass the exact number of arguments declared by the callee signature",
                    ));
                }

                let mut callee_env = BTreeMap::new();
                for (i, arg_smt) in encoded_args.iter().enumerate() {
                    callee_env.insert(format!("p{i}"), arg_smt.clone());
                }

                // 1. Modular call-site obligation: prove callee's preconditions hold under `guard`!
                if emit_checks {
                    for (req_idx, req_expr) in callee_ast.requires.iter().enumerate() {
                        let req_smt = self.encode_expr(req_expr, &callee_env, guard, false)?;
                        self.obligations.push(ProofObligation {
                            guard_smt: guard.to_string(),
                            goal_smt: req_smt,
                            kind: ObligationKind::CallPrecondition {
                                callee_desc: callee_label.clone(),
                                req_index: req_idx + 1,
                            },
                        });
                    }
                }

                // 2. Introduce fresh result symbol & assume callee's postconditions (or pure body if no ens)
                let call_sym = self.fresh_sym("call_ret", &callee_ast.ret_type);
                if !callee_ast.ensures.is_empty() {
                    callee_env.insert("$result".to_string(), call_sym.clone());
                    for ens_expr in &callee_ast.ensures {
                        let ens_smt = self.encode_expr(ens_expr, &callee_env, guard, false)?;
                        self.assumptions
                            .push(format!("(assert (=> {guard} {ens_smt}))"));
                    }
                } else if !matches!(target, TargetRef::SelfRec) && callee_ast.effects.is_empty() {
                    // Pure helper without explicit `ens`: inline its mathematical summary transparently
                    let body_smt = self.encode_expr(&callee_ast.body, &callee_env, guard, false)?;
                    self.assumptions
                        .push(format!("(assert (=> {guard} (= {call_sym} {body_smt})))"));
                }

                Ok(call_sym)
            }
            Expr::Block { stmts, tail } => {
                let mut local_env = env.clone();
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { name, value, .. } => {
                            let v_smt = self.encode_expr(value, &local_env, guard, emit_checks)?;
                            local_env.insert(name.clone(), v_smt);
                        }
                        Stmt::Expr(e) => {
                            let _ = self.encode_expr(e, &local_env, guard, emit_checks)?;
                        }
                    }
                }
                self.encode_expr(tail, &local_env, guard, emit_checks)
            }
        }
    }
}

pub fn verify_node<F>(
    raw_fn: &FnDef,
    canon_node: &CanonicalNode,
    load_callee: &F,
) -> Result<VerificationReport, AxiomDiagnostic>
where
    F: Fn(&str) -> Result<CanonicalNode, AxiomDiagnostic>,
{
    let start = Instant::now();
    let z3_bin = find_z3_binary()?;
    let canon_fn = &canon_node.ast;

    let mut base_env = BTreeMap::new();
    let mut param_decls = Vec::with_capacity(canon_fn.params.len());
    for (i, p) in canon_fn.params.iter().enumerate() {
        let cname = format!("p{i}");
        param_decls.push(format!("(declare-const {cname} {})", smt_sort(&p.ty)));
        base_env.insert(cname, format!("p{i}"));
    }

    let mut builder = SmtBuilder::new(canon_node, load_callee);

    // Encode preconditions
    let mut req_smts = Vec::with_capacity(canon_fn.requires.len());
    for req in &canon_fn.requires {
        req_smts.push(builder.encode_expr(req, &base_env, "true", false)?);
    }

    // 1. Vacuous precondition guard: ensure `req` clauses are mutually satisfiable!
    if !req_smts.is_empty() {
        let mut vac_script = smt_prelude(&param_decls, &builder.decls);
        for r in &req_smts {
            vac_script.push_str(&format!("(assert {r})\n"));
        }
        vac_script.push_str("(check-sat)\n");
        let vac_out = run_z3(&z3_bin, &vac_script)?;
        if vac_out.lines().next().map(str::trim) == Some("unsat") {
            return Err(AxiomDiagnostic::new(
                "E052_VACUOUS_PRECONDITION",
                "verify",
                format!(
                    "Preconditions ('req') of function '{}' are contradictory (never satisfiable)",
                    raw_fn.name
                ),
                Span::zero(),
                "Remove contradictory 'req' constraints; vacuous contracts are rejected to prevent trivial proof cheating",
            ));
        }
    }

    // Encode function body -> $result
    let body_smt = builder.encode_expr(&canon_fn.body, &base_env, "true", true)?;
    let result_sym = "$result";

    // Encode postconditions (`ens`) as proof obligations
    let mut ens_env = base_env.clone();
    ens_env.insert("$result".to_string(), result_sym.to_string());
    for (i, (canon_ens, raw_ens)) in canon_fn
        .ensures
        .iter()
        .zip(raw_fn.ensures.iter())
        .enumerate()
    {
        let ens_smt = builder.encode_expr(canon_ens, &ens_env, "true", false)?;
        let pretty_raw = crate::canon::format_expr_human(raw_ens);
        builder.obligations.push(ProofObligation {
            guard_smt: "true".to_string(),
            goal_smt: ens_smt,
            kind: ObligationKind::Postcondition {
                index: i + 1,
                display: pretty_raw,
            },
        });
    }

    let total_obligations = builder.obligations.len();
    if total_obligations == 0 {
        return Ok(VerificationReport {
            alias: raw_fn.name.clone(),
            short_hash: canon_node.short_hash.clone(),
            obligations_checked: 0,
            elapsed_ms: start.elapsed().as_millis(),
            status: "verified (0 obligations)".to_string(),
        });
    }

    // Build base SMT-LIB2 script shared across obligations
    let mut base_script = smt_prelude(&param_decls, &builder.decls);
    base_script.push_str(&format!(
        "(declare-const {result_sym} {})\n",
        smt_sort(&canon_fn.ret_type)
    ));
    for r in &req_smts {
        base_script.push_str(&format!("(assert {r})\n"));
    }
    for a in &builder.assumptions {
        base_script.push_str(a);
        base_script.push('\n');
    }
    base_script.push_str(&format!("(assert (= {result_sym} {body_smt}))\n"));

    let mut query_vars: Vec<String> = (0..canon_fn.params.len())
        .map(|i| format!("p{i}"))
        .collect();
    query_vars.push(result_sym.to_string());
    let get_val_cmd = format!("(get-value ({}))\n", query_vars.join(" "));

    for ob in &builder.obligations {
        let mut script = base_script.clone();
        script.push_str(&format!(
            "(assert (and {} (not {})))\n(check-sat)\n",
            ob.guard_smt, ob.goal_smt
        ));
        script.push_str(&get_val_cmd);

        let out = run_z3(&z3_bin, &script)?;
        let first_line = out.lines().next().unwrap_or("").trim();

        match first_line {
            "unsat" => {
                // Obligation mathematically proved for ALL inputs!
            }
            "sat" => {
                let cex = extract_counterexample(&out, &raw_fn.params);
                return Err(build_violation_diagnostic(&raw_fn.name, &ob.kind, cex));
            }
            other => {
                return Err(AxiomDiagnostic::new(
                    "E057_SOLVER_INCONCLUSIVE",
                    "verify",
                    format!(
                        "Z3 returned '{other}' while verifying '{}' (non-linear or timeout)",
                        raw_fn.name
                    ),
                    Span::zero(),
                    "Simplify non-linear arithmetic in contract or split into smaller verified lemmas",
                ));
            }
        }
    }

    Ok(VerificationReport {
        alias: raw_fn.name.clone(),
        short_hash: canon_node.short_hash.clone(),
        obligations_checked: total_obligations,
        elapsed_ms: start.elapsed().as_millis(),
        status: format!("proved ({total_obligations} obligations)"),
    })
}

fn build_violation_diagnostic(
    fn_name: &str,
    kind: &ObligationKind,
    cex: BTreeMap<String, String>,
) -> AxiomDiagnostic {
    match kind {
        ObligationKind::Postcondition { index, display } => AxiomDiagnostic::new(
            "E050_CONTRACT_VIOLATION",
            "verify",
            format!(
                "Postcondition #{index} failed for function '{fn_name}': ens {display}"
            ),
            Span::zero(),
            format!(
                "Stubs and partial implementations are rejected. Update '{fn_name}' body so 'ens {display}' holds for the counterexample inputs"
            ),
        )
        .with_counterexample(cex),
        ObligationKind::CallPrecondition {
            callee_desc,
            req_index,
        } => AxiomDiagnostic::new(
            "E051_CALL_PRECONDITION_VIOLATED",
            "verify",
            format!(
                "Function '{fn_name}' may call {callee_desc} violating its precondition #{req_index}"
            ),
            Span::zero(),
            format!(
                "Guard the call to {callee_desc} with an 'if' check or strengthen '{fn_name}' preconditions ('req')"
            ),
        )
        .with_counterexample(cex),
        ObligationKind::NonZeroDivisor => AxiomDiagnostic::new(
            "E053_POSSIBLE_DIV_BY_ZERO",
            "verify",
            format!("Possible division or modulo by zero in function '{fn_name}'"),
            Span::zero(),
            "Ensure divisor is provably non-zero via 'req divisor != 0' or an explicit 'if' branch",
        )
        .with_counterexample(cex),
    }
}

fn smt_prelude(param_decls: &[String], extra_decls: &[String]) -> String {
    let mut s = String::new();
    s.push_str("(set-option :produce-models true)\n");
    // Truncated-toward-zero integer division & remainder matching Rust `i64` `/` and `%`
    s.push_str(
        "(define-fun ax_div ((a Int) (b Int)) Int (ite (>= a 0) (ite (>= b 0) (div a b) (- (div a (- b)))) (ite (>= b 0) (- (div (- a) b)) (div (- a) (- b)))))\n",
    );
    s.push_str("(define-fun ax_mod ((a Int) (b Int)) Int (- a (* (ax_div a b) b)))\n");
    for d in param_decls {
        s.push_str(d);
        s.push('\n');
    }
    for d in extra_decls {
        s.push_str(d);
        s.push('\n');
    }
    s
}

fn smt_sort(ty: &Type) -> &'static str {
    match ty {
        Type::I64 => "Int",
        Type::Bool | Type::Unit => "Bool",
        Type::Str => "String",
        Type::Secret(inner) => smt_sort(inner),
    }
}

fn smt_escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '"' {
            out.push_str("\"\"");
        } else {
            out.push(ch);
        }
    }
    out
}

fn conjoin_guard(old_guard: &str, cond: &str) -> String {
    if old_guard == "true" {
        cond.to_string()
    } else {
        format!("(and {old_guard} {cond})")
    }
}

fn find_z3_binary() -> Result<PathBuf, AxiomDiagnostic> {
    if let Ok(custom) = env::var("AXIOM_Z3_BIN") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Some(home) = env::var_os("HOME") {
        let local = PathBuf::from(home).join(".local/bin/z3");
        if local.is_file() {
            return Ok(local);
        }
    }
    for candidate in ["/usr/bin/z3", "/usr/local/bin/z3"] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(AxiomDiagnostic::new(
        "E058_Z3_NOT_FOUND",
        "verify",
        "Z3 SMT solver binary not found",
        Span::zero(),
        "Install Z3 in ~/.local/bin/z3 or /usr/bin/z3, or set AXIOM_Z3_BIN",
    ))
}

fn run_z3(z3_bin: &PathBuf, script: &str) -> Result<String, AxiomDiagnostic> {
    let mut child = Command::new(z3_bin)
        .args(["-smt2", "-in", "-t:3000"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AxiomDiagnostic::new(
                "E058_Z3_EXEC_ERROR",
                "verify",
                format!("Failed to launch Z3 solver: {e}"),
                Span::zero(),
                "Verify Z3 binary permissions",
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(script.as_bytes());
    }

    let output = child.wait_with_output().map_err(|e| {
        AxiomDiagnostic::new(
            "E058_Z3_EXEC_ERROR",
            "verify",
            format!("Failed waiting on Z3 solver: {e}"),
            Span::zero(),
            "Verify Z3 process health",
        )
    })?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn extract_counterexample(z3_out: &str, raw_params: &[Param]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let rest: String = z3_out.lines().skip(1).collect::<Vec<_>>().join(" ");
    let pairs = parse_sexp_pairs(&rest);

    for (k, v) in pairs {
        let clean_val = normalize_smt_value(&v);
        if let Some(idx_str) = k.strip_prefix('p') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(p) = raw_params.get(idx) {
                    map.insert(p.name.clone(), clean_val);
                    continue;
                }
            }
        }
        if k == "$result" {
            map.insert("result".to_string(), clean_val);
        } else {
            map.insert(k, clean_val);
        }
    }
    map
}

fn normalize_smt_value(val: &str) -> String {
    let trimmed = val.trim();
    if let Some(inner) = trimmed.strip_prefix("(- ").and_then(|s| s.strip_suffix(')')) {
        return format!("-{}", inner.trim());
    }
    trimmed.to_string()
}

fn parse_sexp_pairs(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    // Skip outer '('
    while i < n && bytes[i] != b'(' {
        i += 1;
    }
    i += 1;

    while i < n {
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n || bytes[i] == b')' {
            break;
        }
        if bytes[i] == b'(' {
            i += 1;
            while i < n && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let key_start = i;
            while i < n && !bytes[i].is_ascii_whitespace() && bytes[i] != b')' {
                i += 1;
            }
            let key = input[key_start..i].to_string();
            while i < n && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let val_start = i;
            let mut depth = 0usize;
            let mut in_str = false;
            while i < n {
                let b = bytes[i];
                if b == b'"' {
                    in_str = !in_str;
                } else if !in_str {
                    if b == b'(' {
                        depth += 1;
                    } else if b == b')' {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                }
                i += 1;
            }
            let val = input[val_start..i].trim().to_string();
            if i < n && bytes[i] == b')' {
                i += 1;
            }
            if !key.is_empty() {
                out.push((key, val));
            }
        } else {
            i += 1;
        }
    }
    out
}
