use crate::ast::{EffectCap, Expr, FnDef, Module, Stmt, TargetRef, Type};
use crate::canon::{canonicalize_fn, CanonicalNode};
use crate::diag::{AxiomDiagnostic, Span};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestEntry {
    pub alias: String,
    pub hash: String,
    pub short_hash: String,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeView {
    pub node: CanonicalNode,
    pub aliases: Vec<String>,
    pub neighbor_signatures: Vec<NeighborSig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborSig {
    pub hash: String,
    pub short_hash: String,
    pub aliases: Vec<String>,
    pub params: Vec<String>,
    pub effects: Vec<String>,
    pub ret_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub effect: String,
    pub witness_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPassport {
    pub root_hash: String,
    pub short_hash: String,
    pub aliases: Vec<String>,
    pub total_reachable_nodes: usize,
    pub is_pure: bool,
    pub uses_secret_types: bool,
    pub capabilities: Vec<CapabilityGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummaryNode {
    pub hash: String,
    pub short_hash: String,
    pub aliases: Vec<String>,
    pub direct_deps: Vec<String>,
    pub direct_effects: Vec<String>,
}

pub struct GraphStore {
    nodes_dir: PathBuf,
    aliases_file: PathBuf,
    aliases: BTreeMap<String, String>,
}

impl GraphStore {
    pub fn open(workspace_root: &Path) -> Result<Self, AxiomDiagnostic> {
        let axiom_dir = workspace_root.join(".axiom");
        let nodes_dir = axiom_dir.join("nodes");
        let aliases_file = axiom_dir.join("aliases.json");

        fs::create_dir_all(&nodes_dir).map_err(|e| {
            AxiomDiagnostic::new(
                "E030_STORE_IO",
                "store",
                format!("Failed to initialize .axiom/nodes directory: {e}"),
                Span::zero(),
                "Check write permissions on workspace directory",
            )
        })?;

        let aliases = if aliases_file.is_file() {
            let content = fs::read_to_string(&aliases_file).map_err(|e| {
                AxiomDiagnostic::new(
                    "E030_STORE_IO",
                    "store",
                    format!("Failed to read aliases.json: {e}"),
                    Span::zero(),
                    "Ensure .axiom/aliases.json is readable UTF-8 JSON",
                )
            })?;
            serde_json::from_str(&content).map_err(|e| {
                AxiomDiagnostic::new(
                    "E031_STORE_CORRUPT",
                    "store",
                    format!("Corrupted aliases.json: {e}"),
                    Span::zero(),
                    "Repair or recreate .axiom/aliases.json",
                )
            })?
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            nodes_dir,
            aliases_file,
            aliases,
        })
    }

    fn save_aliases(&self) -> Result<(), AxiomDiagnostic> {
        let json = serde_json::to_string_pretty(&self.aliases).map_err(|e| {
            AxiomDiagnostic::new(
                "E030_STORE_IO",
                "store",
                format!("Failed to serialize aliases: {e}"),
                Span::zero(),
                "Internal serialization error",
            )
        })?;
        fs::write(&self.aliases_file, json).map_err(|e| {
            AxiomDiagnostic::new(
                "E030_STORE_IO",
                "store",
                format!("Failed to write aliases.json: {e}"),
                Span::zero(),
                "Check disk space and permissions",
            )
        })
    }

    pub fn load_node(&self, full_hash: &str) -> Result<CanonicalNode, AxiomDiagnostic> {
        let path = self.nodes_dir.join(format!("{full_hash}.json"));
        let raw = fs::read_to_string(&path).map_err(|_| {
            AxiomDiagnostic::new(
                "E032_NODE_NOT_FOUND",
                "store",
                format!("Node #{full_hash} does not exist in store"),
                Span::zero(),
                "Ingest the missing node first or check hash spelling",
            )
        })?;
        serde_json::from_str(&raw).map_err(|e| {
            AxiomDiagnostic::new(
                "E031_STORE_CORRUPT",
                "store",
                format!("Corrupted node file #{full_hash}: {e}"),
                Span::zero(),
                "Re-ingest source definitions into .axiom/nodes",
            )
        })
    }

    pub fn resolve_hash_prefix(&self, prefix: &str) -> Result<String, AxiomDiagnostic> {
        let clean = prefix.trim_start_matches('#').to_ascii_lowercase();
        if clean.len() == 64 && self.nodes_dir.join(format!("{clean}.json")).is_file() {
            return Ok(clean);
        }

        let mut matches = Vec::new();
        let entries = fs::read_dir(&self.nodes_dir).map_err(|e| {
            AxiomDiagnostic::new(
                "E030_STORE_IO",
                "store",
                format!("Cannot read .axiom/nodes: {e}"),
                Span::zero(),
                "Ensure .axiom/nodes directory exists",
            )
        })?;

        for entry in entries.flatten() {
            let fname = entry.file_name();
            let s = fname.to_string_lossy();
            if let Some(stem) = s.strip_suffix(".json") {
                if stem.starts_with(&clean) {
                    matches.push(stem.to_string());
                }
            }
        }

        match matches.len() {
            0 => Err(AxiomDiagnostic::new(
                "E032_NODE_NOT_FOUND",
                "resolve",
                format!("No node hash matching '#{clean}' found in store"),
                Span::zero(),
                "Run 'axiom graph' to list available node hashes",
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(AxiomDiagnostic::new(
                "E033_AMBIGUOUS_HASH_PREFIX",
                "resolve",
                format!("Hash prefix '#{clean}' matches multiple nodes: {matches:?}"),
                Span::zero(),
                "Provide a longer hash prefix (e.g. 12+ hex chars)",
            )),
        }
    }

    pub fn resolve_ref(&self, name_or_hash: &str) -> Result<String, AxiomDiagnostic> {
        if let Some(stripped) = name_or_hash.strip_prefix('#') {
            return self.resolve_hash_prefix(stripped);
        }
        if let Some(full) = self.aliases.get(name_or_hash) {
            return Ok(full.clone());
        }
        // Also allow bare hash prefix if valid hex >= 6 chars
        if name_or_hash.len() >= 6 && name_or_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(full) = self.resolve_hash_prefix(name_or_hash) {
                return Ok(full);
            }
        }
        Err(AxiomDiagnostic::new(
            "E022_UNKNOWN_FUNCTION",
            "resolve",
            format!("Unknown function alias or hash '{name_or_hash}'"),
            Span::zero(),
            "Define the target function or ingest it into the .axiom store first",
        ))
    }

    pub fn aliases_for_hash(&self, full_hash: &str) -> Vec<String> {
        self.aliases
            .iter()
            .filter_map(|(alias, h)| {
                if h == full_hash {
                    Some(alias.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn rename_alias(&mut self, old_name: &str, new_name: &str) -> Result<String, AxiomDiagnostic> {
        let hash = self.aliases.remove(old_name).ok_or_else(|| {
            AxiomDiagnostic::new(
                "E022_UNKNOWN_FUNCTION",
                "store",
                format!("Alias '{old_name}' does not exist"),
                Span::zero(),
                "Run 'axiom graph' to inspect existing aliases",
            )
        })?;
        self.aliases.insert(new_name.to_string(), hash.clone());
        self.save_aliases()?;
        Ok(hash)
    }

    pub fn ingest_module(&mut self, module: &Module) -> Result<Vec<IngestEntry>, AxiomDiagnostic> {
        let order = topo_sort_functions(&module.functions)?;
        let mut results = Vec::with_capacity(order.len());

        for idx in order {
            let raw_fn = &module.functions[idx];
            let node = canonicalize_fn(
                raw_fn,
                &|callee| {
                    self.aliases.get(callee).cloned().ok_or_else(|| {
                        AxiomDiagnostic::new(
                            "E022_UNKNOWN_FUNCTION",
                            "resolve",
                            format!(
                                "Function '{}' calls unknown function '{callee}'",
                                raw_fn.name
                            ),
                            Span::zero(),
                            "Define the callee in the same input or ingest it into .axiom first",
                        )
                    })
                },
                &|prefix| self.resolve_hash_prefix(prefix),
            )?;

            let node_file = self.nodes_dir.join(format!("{}.json", node.hash));
            let deduplicated = node_file.is_file();
            if !deduplicated {
                let json = serde_json::to_string_pretty(&node).map_err(|e| {
                    AxiomDiagnostic::new(
                        "E030_STORE_IO",
                        "store",
                        format!("Failed to serialize node: {e}"),
                        Span::zero(),
                        "Internal JSON serialization error",
                    )
                })?;
                fs::write(&node_file, json).map_err(|e| {
                    AxiomDiagnostic::new(
                        "E030_STORE_IO",
                        "store",
                        format!("Failed to write node file: {e}"),
                        Span::zero(),
                        "Check write permissions in .axiom/nodes",
                    )
                })?;
            }

            self.aliases.insert(raw_fn.name.clone(), node.hash.clone());
            results.push(IngestEntry {
                alias: raw_fn.name.clone(),
                hash: node.hash,
                short_hash: node.short_hash,
                deduplicated,
            });
        }

        self.save_aliases()?;
        Ok(results)
    }

    pub fn show_node(&self, name_or_hash: &str) -> Result<NodeView, AxiomDiagnostic> {
        let full_hash = self.resolve_ref(name_or_hash)?;
        let node = self.load_node(&full_hash)?;
        let aliases = self.aliases_for_hash(&full_hash);

        let mut neighbor_signatures = Vec::with_capacity(node.direct_deps.len());
        for dep_hash in &node.direct_deps {
            let dep = self.load_node(dep_hash)?;
            neighbor_signatures.push(NeighborSig {
                hash: dep.hash.clone(),
                short_hash: dep.short_hash.clone(),
                aliases: self.aliases_for_hash(&dep.hash),
                params: dep
                    .ast
                    .params
                    .iter()
                    .map(|p| format!("{}:{}", p.name, p.ty))
                    .collect(),
                effects: dep.direct_effects.iter().map(|e| e.to_string()).collect(),
                ret_type: dep.ast.ret_type.to_string(),
            });
        }

        Ok(NodeView {
            node,
            aliases,
            neighbor_signatures,
        })
    }

    pub fn list_graph(&self) -> Result<Vec<GraphSummaryNode>, AxiomDiagnostic> {
        let mut nodes = Vec::new();
        let entries = fs::read_dir(&self.nodes_dir).map_err(|e| {
            AxiomDiagnostic::new(
                "E030_STORE_IO",
                "store",
                format!("Failed to read .axiom/nodes: {e}"),
                Span::zero(),
                "Check directory permissions",
            )
        })?;

        for entry in entries.flatten() {
            let fname = entry.file_name();
            let s = fname.to_string_lossy();
            if let Some(hash) = s.strip_suffix(".json") {
                let node = self.load_node(hash)?;
                nodes.push(GraphSummaryNode {
                    hash: node.hash.clone(),
                    short_hash: node.short_hash,
                    aliases: self.aliases_for_hash(&node.hash),
                    direct_deps: node.direct_deps,
                    direct_effects: node.direct_effects.iter().map(|e| e.to_string()).collect(),
                });
            }
        }
        nodes.sort_by(|a, b| a.hash.cmp(&b.hash));
        Ok(nodes)
    }

    pub fn compute_passport(&self, name_or_hash: &str) -> Result<SecurityPassport, AxiomDiagnostic> {
        let root_hash = self.resolve_ref(name_or_hash)?;
        let root_node = self.load_node(&root_hash)?;
        let root_aliases = self.aliases_for_hash(&root_hash);

        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut queue: VecDeque<(String, Vec<String>)> = VecDeque::new();
        let mut effect_witness: BTreeMap<EffectCap, Vec<String>> = BTreeMap::new();
        let mut uses_secret = fn_uses_secret(&root_node.ast);

        let root_label = format_node_label(&root_node, &root_aliases);
        visited.insert(root_hash.clone());
        queue.push_back((root_hash.clone(), vec![root_label]));

        while let Some((curr_hash, path)) = queue.pop_front() {
            let curr = self.load_node(&curr_hash)?;
            if fn_uses_secret(&curr.ast) {
                uses_secret = true;
            }
            for eff in &curr.direct_effects {
                effect_witness
                    .entry(eff.clone())
                    .or_insert_with(|| path.clone());
            }
            for dep_hash in &curr.direct_deps {
                if visited.insert(dep_hash.clone()) {
                    let dep_node = self.load_node(dep_hash)?;
                    let dep_aliases = self.aliases_for_hash(dep_hash);
                    let mut next_path = path.clone();
                    next_path.push(format_node_label(&dep_node, &dep_aliases));
                    queue.push_back((dep_hash.clone(), next_path));
                }
            }
        }

        let capabilities: Vec<CapabilityGrant> = effect_witness
            .into_iter()
            .map(|(eff, chain)| CapabilityGrant {
                effect: eff.to_string(),
                witness_chain: chain,
            })
            .collect();

        Ok(SecurityPassport {
            root_hash,
            short_hash: root_node.short_hash,
            aliases: root_aliases,
            total_reachable_nodes: visited.len(),
            is_pure: capabilities.is_empty(),
            uses_secret_types: uses_secret,
            capabilities,
        })
    }
}

fn format_node_label(node: &CanonicalNode, aliases: &[String]) -> String {
    if let Some(first) = aliases.first() {
        format!("{first} (#{})", node.short_hash)
    } else {
        format!("#{}", node.short_hash)
    }
}

fn type_has_secret(ty: &Type) -> bool {
    match ty {
        Type::Secret(_) => true,
        Type::I64 | Type::Bool | Type::Str | Type::Unit => false,
    }
}

fn fn_uses_secret(f: &FnDef) -> bool {
    f.params.iter().any(|p| type_has_secret(&p.ty))
        || type_has_secret(&f.ret_type)
        || expr_uses_secret(&f.body)
}

fn expr_uses_secret(e: &Expr) -> bool {
    match e {
        Expr::SecretWrap(_) | Expr::Reveal { .. } => true,
        Expr::Unary { expr: inner, .. } => expr_uses_secret(inner),
        Expr::Binary { lhs, rhs, .. } => expr_uses_secret(lhs) || expr_uses_secret(rhs),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => expr_uses_secret(cond) || expr_uses_secret(then_branch) || expr_uses_secret(else_branch),
        Expr::Call { args, .. } | Expr::Perform { args, .. } => args.iter().any(expr_uses_secret),
        Expr::Block { stmts, tail } => {
            stmts.iter().any(|s| match s {
                Stmt::Let { ty, value, .. } => {
                    ty.as_ref().is_some_and(type_has_secret) || expr_uses_secret(value)
                }
                Stmt::Expr(inner) => expr_uses_secret(inner),
            }) || expr_uses_secret(tail)
        }
        Expr::IntLit(_) | Expr::BoolLit(_) | Expr::StrLit(_) | Expr::UnitLit | Expr::Var(_) => {
            false
        }
    }
}

fn collect_named_calls(expr: &Expr, self_name: &str, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Call { target, args } => {
            if let TargetRef::Named(name) = target {
                if name != self_name {
                    out.insert(name.clone());
                }
            }
            for a in args {
                collect_named_calls(a, self_name, out);
            }
        }
        Expr::Perform { args, .. } => {
            for a in args {
                collect_named_calls(a, self_name, out);
            }
        }
        Expr::Unary { expr: inner, .. }
        | Expr::SecretWrap(inner)
        | Expr::Reveal { expr: inner, .. } => collect_named_calls(inner, self_name, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_named_calls(lhs, self_name, out);
            collect_named_calls(rhs, self_name, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_named_calls(cond, self_name, out);
            collect_named_calls(then_branch, self_name, out);
            collect_named_calls(else_branch, self_name, out);
        }
        Expr::Block { stmts, tail } => {
            for s in stmts {
                match s {
                    Stmt::Let { value, .. } => collect_named_calls(value, self_name, out),
                    Stmt::Expr(e) => collect_named_calls(e, self_name, out),
                }
            }
            collect_named_calls(tail, self_name, out);
        }
        Expr::IntLit(_) | Expr::BoolLit(_) | Expr::StrLit(_) | Expr::UnitLit | Expr::Var(_) => {}
    }
}

fn topo_sort_functions(fns: &[FnDef]) -> Result<Vec<usize>, AxiomDiagnostic> {
    let mut name_to_idx: BTreeMap<String, usize> = BTreeMap::new();
    for (i, f) in fns.iter().enumerate() {
        if name_to_idx.insert(f.name.clone(), i).is_some() {
            return Err(AxiomDiagnostic::new(
                "E024_DUPLICATE_FN_NAME",
                "resolve",
                format!("Duplicate function name '{}' in the same module", f.name),
                Span::zero(),
                "Give each function in a batch a distinct alias or ingest versions separately",
            ));
        }
    }

    let n = fns.len();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree: Vec<usize> = vec![0; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, f) in fns.iter().enumerate() {
        let mut called = BTreeSet::new();
        for r in &f.requires {
            collect_named_calls(r, &f.name, &mut called);
        }
        for e in &f.ensures {
            collect_named_calls(e, &f.name, &mut called);
        }
        collect_named_calls(&f.body, &f.name, &mut called);

        for cname in called {
            if let Some(&dep_idx) = name_to_idx.get(&cname) {
                deps[i].push(dep_idx);
                dependents[dep_idx].push(i);
                indegree[i] += 1;
            }
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in indegree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(u) = queue.pop_front() {
        order.push(u);
        for &v in &dependents[u] {
            indegree[v] -= 1;
            if indegree[v] == 0 {
                queue.push_back(v);
            }
        }
    }

    if order.len() != n {
        return Err(AxiomDiagnostic::new(
            "E023_MUTUAL_RECURSION",
            "resolve",
            "Mutual recursion cycle detected across functions in module",
            Span::zero(),
            "Refactor mutually recursive functions into a single recursive node using self-calls",
        ));
    }

    Ok(order)
}
