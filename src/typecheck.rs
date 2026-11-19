use crate::ast::{BinOp, Expr, FnDef, Stmt, TargetRef, Type, UnaryOp};
use crate::canon::CanonicalNode;
use crate::diag::{AxiomDiagnostic, Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DirectSecretSink {
    pub secret_type: String,
    pub domain: String,
    pub operation: String,
}

struct TypeEnv {
    frames: Vec<BTreeMap<String, Type>>,
}

impl TypeEnv {
    fn from_fn(f: &FnDef) -> Self {
        let mut root = BTreeMap::new();
        for p in &f.params {
            root.insert(p.name.clone(), p.ty.clone());
        }
        Self { frames: vec![root] }
    }

    fn push(&mut self) {
        self.frames.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn bind(&mut self, name: String, ty: Type) {
        if let Some(top) = self.frames.last_mut() {
            top.insert(name, ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for frame in self.frames.iter().rev() {
            if let Some(t) = frame.get(name) {
                return Some(t.clone());
            }
        }
        None
    }
}

pub fn typecheck_node<F>(
    raw_name: &str,
    canon_node: &CanonicalNode,
    load_callee: &F,
) -> Result<Vec<DirectSecretSink>, AxiomDiagnostic>
where
    F: Fn(&str) -> Result<CanonicalNode, AxiomDiagnostic>,
{
    let f = &canon_node.ast;
    let mut env = TypeEnv::from_fn(f);
    let mut sinks = Vec::new();

    // 1. Typecheck preconditions (`req`) — must be bool and free of Secret<T> inspection
    for req in &f.requires {
        let ty = check_expr(req, raw_name, canon_node, &mut env, load_callee, &mut sinks, true)?;
        if ty != Type::Bool {
            return Err(AxiomDiagnostic::new(
                "E060_TYPE_MISMATCH",
                "typecheck",
                format!(
                    "Precondition ('req') in '{raw_name}' must have type 'bool', found '{ty}'"
                ),
                Span::zero(),
                "Write boolean expressions in 'req' clauses (e.g. req amount > 0)",
            ));
        }
    }

    // 2. Typecheck postconditions (`ens`) — bind $result, must be bool and free of Secret<T>
    env.push();
    env.bind("$result".to_string(), f.ret_type.clone());
    for ens in &f.ensures {
        let ty = check_expr(ens, raw_name, canon_node, &mut env, load_callee, &mut sinks, true)?;
        if ty != Type::Bool {
            return Err(AxiomDiagnostic::new(
                "E060_TYPE_MISMATCH",
                "typecheck",
                format!(
                    "Postcondition ('ens') in '{raw_name}' must have type 'bool', found '{ty}'"
                ),
                Span::zero(),
                "Write boolean expressions in 'ens' clauses (e.g. ens result >= 0)",
            ));
        }
    }
    env.pop();

    // 3. Typecheck function body and match return type
    let body_ty = check_expr(
        &f.body,
        raw_name,
        canon_node,
        &mut env,
        load_callee,
        &mut sinks,
        false,
    )?;
    if body_ty != f.ret_type {
        return Err(AxiomDiagnostic::new(
            "E060_TYPE_MISMATCH",
            "typecheck",
            format!(
                "Function '{raw_name}' declares return type '{}', but body evaluates to '{body_ty}'",
                f.ret_type
            ),
            Span::zero(),
            format!(
                "Change return expression or update signature of '{raw_name}' to '-> {body_ty}'"
            ),
        ));
    }

    sinks.sort();
    sinks.dedup();
    Ok(sinks)
}

fn check_expr<F>(
    expr: &Expr,
    fn_name: &str,
    self_node: &CanonicalNode,
    env: &mut TypeEnv,
    load_callee: &F,
    sinks: &mut Vec<DirectSecretSink>,
    in_contract: bool,
) -> Result<Type, AxiomDiagnostic>
where
    F: Fn(&str) -> Result<CanonicalNode, AxiomDiagnostic>,
{
    match expr {
        Expr::IntLit(_) => Ok(Type::I64),
        Expr::BoolLit(_) => Ok(Type::Bool),
        Expr::StrLit(_) => Ok(Type::Str),
        Expr::UnitLit => Ok(Type::Unit),
        Expr::Var(name) => {
            let ty = env.lookup(name).ok_or_else(|| {
                AxiomDiagnostic::new(
                    "E020_UNKNOWN_VAR",
                    "typecheck",
                    format!("Unbound symbol '{name}' in '{fn_name}'"),
                    Span::zero(),
                    "Ensure variable is declared in parameters or a preceding 'let'",
                )
            })?;
            if in_contract && type_contains_secret(&ty) {
                return Err(AxiomDiagnostic::new(
                    "E062_SECRET_IN_CONTRACT",
                    "typecheck",
                    format!(
                        "Contract of '{fn_name}' references '{ty}'; contracts cannot inspect Secret<T> values"
                    ),
                    Span::zero(),
                    "Keep Secret<T> opaque in public contracts to prevent specification side channels",
                ));
            }
            Ok(ty)
        }
        Expr::SecretWrap(inner) => {
            if in_contract {
                return Err(AxiomDiagnostic::new(
                    "E062_SECRET_IN_CONTRACT",
                    "typecheck",
                    format!("'secret(...)' cannot appear inside a contract in '{fn_name}'"),
                    Span::zero(),
                    "Wrap secrets inside executable function bodies, not contracts",
                ));
            }
            let inner_ty = check_expr(
                inner,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            if type_contains_secret(&inner_ty) {
                return Err(AxiomDiagnostic::new(
                    "E060_TYPE_MISMATCH",
                    "typecheck",
                    format!("Cannot double-wrap '{inner_ty}' inside Secret<...> in '{fn_name}'"),
                    Span::zero(),
                    "Value is already wrapped as Secret<T>",
                ));
            }
            Ok(Type::Secret(Box::new(inner_ty)))
        }
        Expr::Reveal { domain, .. } => Err(AxiomDiagnostic::new(
            "E064_REVEAL_OUTSIDE_NET_SINK",
            "typecheck",
            format!(
                "Illegal 'reveal(\"{domain}\", ...)' in '{fn_name}': revealed secrets cannot be stored in variables, returned, or used in expressions"
            ),
            Span::zero(),
            format!(
                "Pass 'reveal(\"{domain}\", secret_val)' directly into 'do Net.<op>(...)' inside a function pinned to Net(\"{domain}\")"
            ),
        )),
        Expr::Unary { op, expr: inner } => {
            let t = check_expr(
                inner,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            if type_contains_secret(&t) {
                return Err(AxiomDiagnostic::new(
                    "E062_SECRET_OPACITY_VIOLATION",
                    "typecheck",
                    format!(
                        "Cannot apply unary operator '{}' to opaque '{t}' in '{fn_name}'",
                        op.as_str()
                    ),
                    Span::zero(),
                    "Secret<T> values are opaque to prevent side-channel leaks; pass them intact or reveal at an authorized Net(\"domain\") sink",
                ));
            }
            match (op, &t) {
                (UnaryOp::Neg, Type::I64) => Ok(Type::I64),
                (UnaryOp::Not, Type::Bool) => Ok(Type::Bool),
                _ => Err(AxiomDiagnostic::new(
                    "E060_TYPE_MISMATCH",
                    "typecheck",
                    format!(
                        "Unary operator '{}' cannot be applied to type '{t}' in '{fn_name}'",
                        op.as_str()
                    ),
                    Span::zero(),
                    "Use '-' on i64 and '!' on bool",
                )),
            }
        }
        Expr::Binary { op, lhs, rhs } => {
            let lt = check_expr(
                lhs,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            let rt = check_expr(
                rhs,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            if type_contains_secret(&lt) || type_contains_secret(&rt) {
                return Err(AxiomDiagnostic::new(
                    "E062_SECRET_OPACITY_VIOLATION",
                    "typecheck",
                    format!(
                        "Cannot apply operator '{}' to Secret<T> ('{lt}' vs '{rt}') in '{fn_name}'",
                        op.as_str()
                    ),
                    Span::zero(),
                    "Comparing or computing directly on Secret<T> is forbidden to block timing and control-flow side-channel attacks",
                ));
            }
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    if lt == Type::I64 && rt == Type::I64 {
                        Ok(Type::I64)
                    } else {
                        Err(AxiomDiagnostic::new(
                            "E060_TYPE_MISMATCH",
                            "typecheck",
                            format!(
                                "Arithmetic operator '{}' expects (i64, i64), found ({lt}, {rt}) in '{fn_name}'",
                                op.as_str()
                            ),
                            Span::zero(),
                            "Ensure both operands of arithmetic operators have type i64",
                        ))
                    }
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    if lt == Type::I64 && rt == Type::I64 {
                        Ok(Type::Bool)
                    } else {
                        Err(AxiomDiagnostic::new(
                            "E060_TYPE_MISMATCH",
                            "typecheck",
                            format!(
                                "Comparison operator '{}' expects (i64, i64), found ({lt}, {rt}) in '{fn_name}'",
                                op.as_str()
                            ),
                            Span::zero(),
                            "Ensure both operands of ordering comparisons have type i64",
                        ))
                    }
                }
                BinOp::And | BinOp::Or => {
                    if lt == Type::Bool && rt == Type::Bool {
                        Ok(Type::Bool)
                    } else {
                        Err(AxiomDiagnostic::new(
                            "E060_TYPE_MISMATCH",
                            "typecheck",
                            format!(
                                "Logical operator '{}' expects (bool, bool), found ({lt}, {rt}) in '{fn_name}'",
                                op.as_str()
                            ),
                            Span::zero(),
                            "Ensure both operands of '&&' / '||' have type bool",
                        ))
                    }
                }
                BinOp::Eq | BinOp::Ne => {
                    if lt == rt {
                        Ok(Type::Bool)
                    } else {
                        Err(AxiomDiagnostic::new(
                            "E060_TYPE_MISMATCH",
                            "typecheck",
                            format!(
                                "Equality operator '{}' requires identical types, found ({lt}, {rt}) in '{fn_name}'",
                                op.as_str()
                            ),
                            Span::zero(),
                            "Compare values of the same non-secret type",
                        ))
                    }
                }
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let ct = check_expr(
                cond,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            if ct != Type::Bool {
                return Err(AxiomDiagnostic::new(
                    "E060_TYPE_MISMATCH",
                    "typecheck",
                    format!("'if' condition in '{fn_name}' must be 'bool', found '{ct}'"),
                    Span::zero(),
                    "Provide a boolean condition for 'if'",
                ));
            }
            let tt = check_expr(
                then_branch,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            let et = check_expr(
                else_branch,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            if tt != et {
                return Err(AxiomDiagnostic::new(
                    "E060_TYPE_MISMATCH",
                    "typecheck",
                    format!(
                        "'if' branches in '{fn_name}' have incompatible types: '{tt}' vs '{et}'"
                    ),
                    Span::zero(),
                    "Ensure both 'if' and 'else' branches return the exact same type",
                ));
            }
            Ok(tt)
        }
        Expr::Perform { effect, op, args } => {
            if in_contract {
                return Err(AxiomDiagnostic::new(
                    "E066_EFFECT_IN_CONTRACT",
                    "typecheck",
                    format!(
                        "Effectful operation 'do {effect}.{op}' is forbidden inside contracts of '{fn_name}'"
                    ),
                    Span::zero(),
                    "Contracts ('req'/'ens') must be pure mathematical specifications",
                ));
            }

            for arg in args {
                if let Expr::Reveal {
                    domain,
                    expr: inner_secret,
                } = arg
                {
                    if effect != "Net" {
                        return Err(AxiomDiagnostic::new(
                            "E064_REVEAL_OUTSIDE_NET_SINK",
                            "typecheck",
                            format!(
                                "Function '{fn_name}' attempts to pass revealed secret for '{domain}' into non-network sink '{effect}.{op}'"
                            ),
                            Span::zero(),
                            "Secrets can never be written to Fs, Audit, or Clock sinks; only domain-pinned Net(\"...\") sinks are allowed",
                        ));
                    }

                    let sec_ty = check_expr(
                        inner_secret,
                        fn_name,
                        self_node,
                        env,
                        load_callee,
                        sinks,
                        false,
                    )?;
                    let Type::Secret(inner_t) = sec_ty else {
                        return Err(AxiomDiagnostic::new(
                            "E060_TYPE_MISMATCH",
                            "typecheck",
                            format!(
                                "'reveal(\"{domain}\", ...)' in '{fn_name}' expects a Secret<T> argument, found '{sec_ty}'"
                            ),
                            Span::zero(),
                            "Pass a value of type Secret<T> to reveal(...)",
                        ));
                    };

                    // Verify strict single-domain Net("domain") pinning on enclosing function!
                    verify_net_domain_pinning(fn_name, &self_node.ast, domain)?;

                    sinks.push(DirectSecretSink {
                        secret_type: format!("Secret<{inner_t}>"),
                        domain: domain.clone(),
                        operation: format!("Net.{op}"),
                    });
                } else {
                    let arg_ty = check_expr(
                        arg,
                        fn_name,
                        self_node,
                        env,
                        load_callee,
                        sinks,
                        false,
                    )?;
                    if type_contains_secret(&arg_ty) {
                        return Err(AxiomDiagnostic::new(
                            "E063_RAW_SECRET_SINK_LEAK",
                            "typecheck",
                            format!(
                                "Security violation in '{fn_name}': raw '{arg_ty}' passed directly to sink '{effect}.{op}'"
                            ),
                            Span::zero(),
                            "Raw Secret<T> values cannot be written to logs, files, or network sockets; use explicit domain-pinned reveal(\"domain\", secret_val) inside a pinned Net(\"domain\") node",
                        ));
                    }
                }
            }
            Ok(Type::Unit)
        }
        Expr::Call { target, args } => {
            let callee_node: CanonicalNode;
            let (callee_ast, callee_label): (&FnDef, String) = match target {
                TargetRef::SelfRec => (&self_node.ast, fn_name.to_string()),
                TargetRef::Hash(h) => {
                    callee_node = load_callee(h)?;
                    let short = callee_node.short_hash.clone();
                    (&callee_node.ast, format!("#{short}"))
                }
                TargetRef::Named(n) => {
                    return Err(AxiomDiagnostic::new(
                        "E055_UNRESOLVED_CALL",
                        "typecheck",
                        format!("Unresolved symbolic call '{n}' in '{fn_name}'"),
                        Span::zero(),
                        "Canonicalize function calls before typechecking",
                    ));
                }
            };

            if args.len() != callee_ast.params.len() {
                return Err(AxiomDiagnostic::new(
                    "E056_ARITY_MISMATCH",
                    "typecheck",
                    format!(
                        "Call to {callee_label} in '{fn_name}' passed {} argument(s), expected {}",
                        args.len(),
                        callee_ast.params.len()
                    ),
                    Span::zero(),
                    "Match the exact parameter count of the called function",
                ));
            }

            for (i, (arg_expr, param)) in args.iter().zip(callee_ast.params.iter()).enumerate() {
                let arg_ty = check_expr(
                    arg_expr,
                    fn_name,
                    self_node,
                    env,
                    load_callee,
                    sinks,
                    in_contract,
                )?;
                if arg_ty != param.ty {
                    return Err(AxiomDiagnostic::new(
                        "E061_ARG_TYPE_MISMATCH",
                        "typecheck",
                        format!(
                            "Argument #{} in call to {callee_label} inside '{fn_name}' has type '{arg_ty}', expected '{}'",
                            i + 1,
                            param.ty
                        ),
                        Span::zero(),
                        format!(
                            "Pass a value of type '{}' for parameter #{}",
                            param.ty,
                            i + 1
                        ),
                    ));
                }
            }

            Ok(callee_ast.ret_type.clone())
        }
        Expr::Block { stmts, tail } => {
            env.push();
            for stmt in stmts {
                match stmt {
                    Stmt::Let { name, ty, value } => {
                        let val_ty = check_expr(
                            value,
                            fn_name,
                            self_node,
                            env,
                            load_callee,
                            sinks,
                            in_contract,
                        )?;
                        if let Some(decl_ty) = ty {
                            if &val_ty != decl_ty {
                                env.pop();
                                return Err(AxiomDiagnostic::new(
                                    "E060_TYPE_MISMATCH",
                                    "typecheck",
                                    format!(
                                        "Let binding '{name}' in '{fn_name}' declared type '{decl_ty}', but initializer has type '{val_ty}'"
                                    ),
                                    Span::zero(),
                                    format!(
                                        "Change annotation to '{val_ty}' or fix initializer expression"
                                    ),
                                ));
                            }
                        }
                        env.bind(name.clone(), val_ty);
                    }
                    Stmt::Expr(e) => {
                        let _ = check_expr(
                            e,
                            fn_name,
                            self_node,
                            env,
                            load_callee,
                            sinks,
                            in_contract,
                        )?;
                    }
                }
            }
            let tail_ty = check_expr(
                tail,
                fn_name,
                self_node,
                env,
                load_callee,
                sinks,
                in_contract,
            )?;
            env.pop();
            Ok(tail_ty)
        }
    }
}

fn verify_net_domain_pinning(
    fn_name: &str,
    ast: &FnDef,
    expected_domain: &str,
) -> Result<(), AxiomDiagnostic> {
    let net_caps: Vec<Option<&str>> = ast
        .effects
        .iter()
        .filter(|c| c.name == "Net")
        .map(|c| c.scope.as_deref())
        .collect();

    if net_caps.is_empty() {
        return Err(AxiomDiagnostic::new(
            "E021_UNDECLARED_EFFECT",
            "typecheck",
            format!(
                "Function '{fn_name}' reveals a secret for '{expected_domain}' without declaring Net(\"{expected_domain}\")"
            ),
            Span::zero(),
            format!("Add Net(\"{expected_domain}\") to '{fn_name}' capability signature"),
        ));
    }

    if net_caps.contains(&None) {
        return Err(AxiomDiagnostic::new(
            "E065_REVEAL_DOMAIN_MISMATCH",
            "typecheck",
            format!(
                "Function '{fn_name}' reveals Secret for '{expected_domain}', but holds an unpinned wildcard 'Net' capability"
            ),
            Span::zero(),
            format!(
                "Replace wildcard 'Net' with strictly pinned 'Net(\"{expected_domain}\")' so secrets cannot be routed to arbitrary hosts"
            ),
        ));
    }

    let scopes: Vec<&str> = net_caps.into_iter().flatten().collect();
    if scopes != [expected_domain] {
        return Err(AxiomDiagnostic::new(
            "E065_REVEAL_DOMAIN_MISMATCH",
            "typecheck",
            format!(
                "Function '{fn_name}' reveals Secret for '{expected_domain}', but holds Net capabilities for {scopes:?}"
            ),
            Span::zero(),
            format!(
                "Isolate secret transmission inside a dedicated node whose sole Net capability is Net(\"{expected_domain}\")"
            ),
        ));
    }

    Ok(())
}

pub fn type_contains_secret(ty: &Type) -> bool {
    match ty {
        Type::Secret(_) => true,
        Type::I64 | Type::Bool | Type::Str | Type::Unit => false,
    }
}
