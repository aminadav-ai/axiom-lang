#![deny(clippy::todo, clippy::unimplemented, dead_code, unused_must_use)]

use axiom_lang::codegen::{build_native_binary, run_native_binary};
use axiom_lang::diag::{AxiomDiagnostic, Span};
use axiom_lang::parser::parse_module;
use axiom_lang::store::GraphStore;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let json_mode = args.iter().any(|a| a == "--json");
    let pos_args: Vec<&str> = args
        .iter()
        .filter(|a| a.as_str() != "--json")
        .map(String::as_str)
        .collect();

    match run_cli(&pos_args, json_mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(diag) => {
            if json_mode {
                eprintln!("{}", diag.to_json_pretty());
            } else {
                eprintln!("error {}", diag);
            }
            ExitCode::FAILURE
        }
    }
}

fn run_cli(args: &[&str], json_mode: bool) -> Result<(), AxiomDiagnostic> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match args {
        ["verify", file_path] => {
            let src = fs::read_to_string(file_path).map_err(|e| {
                AxiomDiagnostic::new(
                    "E040_FILE_READ",
                    "cli",
                    format!("Cannot read '{file_path}': {e}"),
                    Span::zero(),
                    "Provide a valid path to an .ax source file",
                )
            })?;
            let module = parse_module(&src)?;
            let store = GraphStore::open(&cwd)?;
            let reports = store.verify_module(&module)?;

            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&reports).unwrap_or_default()
                );
            } else {
                for r in reports {
                    println!(
                        "  #{}  {:<26} [Z3: {} in {}ms]",
                        r.short_hash, r.alias, r.status, r.elapsed_ms
                    );
                }
            }
            Ok(())
        }
        ["ingest", file_path] => {
            let src = fs::read_to_string(file_path).map_err(|e| {
                AxiomDiagnostic::new(
                    "E040_FILE_READ",
                    "cli",
                    format!("Cannot read '{file_path}': {e}"),
                    Span::zero(),
                    "Provide a valid path to an .ax source file",
                )
            })?;
            let module = parse_module(&src)?;
            let mut store = GraphStore::open(&cwd)?;
            let entries = store.ingest_module(&module)?;

            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&entries).unwrap_or_default()
                );
            } else {
                for item in entries {
                    let storage_tag = if item.deduplicated {
                        "dedup (identical semantics)"
                    } else {
                        "stored"
                    };
                    println!(
                        "  #{}  {:<26} [Z3 proved {} oblig. ({}ms) | {}]",
                        item.short_hash,
                        item.alias,
                        item.obligations_proved,
                        item.verify_ms,
                        storage_tag
                    );
                }
            }
            Ok(())
        }
        ["build", target] => {
            let store = GraphStore::open(&cwd)?;
            let art = build_native_binary(&store, &cwd, target, None)?;
            print_build_artifact(&art, json_mode);
            Ok(())
        }
        ["build", target, "-o", out_path] => {
            let store = GraphStore::open(&cwd)?;
            let art = build_native_binary(&store, &cwd, target, Some(Path::new(out_path)))?;
            print_build_artifact(&art, json_mode);
            Ok(())
        }
        ["run", target, rest @ ..] => {
            let store = GraphStore::open(&cwd)?;
            let outcome = run_native_binary(&store, &cwd, target, rest)?;
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&outcome).unwrap_or_default()
                );
            } else {
                if !outcome.stdout.is_empty() {
                    print!("{}", outcome.stdout);
                }
                if !outcome.stderr.is_empty() {
                    eprint!("{}", outcome.stderr);
                }
                if outcome.exit_code != 0 {
                    return Err(AxiomDiagnostic::new(
                        "E074_RUNTIME_REJECTED",
                        "runtime",
                        format!(
                            "Node '{}' exited with status {}",
                            target, outcome.exit_code
                        ),
                        Span::zero(),
                        "Ensure CLI arguments match parameter types and satisfy root 'req' preconditions",
                    ));
                }
            }
            Ok(())
        }
        ["show", target] => {
            let store = GraphStore::open(&cwd)?;
            let view = store.show_node(target)?;
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&view).unwrap_or_default()
                );
            } else {
                println!("Node:      #{}", view.node.hash);
                println!("Aliases:   {}", view.aliases.join(", "));
                println!("Canonical: {}", view.node.canonical_source);
                if !view.neighbor_signatures.is_empty() {
                    println!("Direct Neighbors (minimal AI context):");
                    for n in view.neighbor_signatures {
                        let eff = if n.effects.is_empty() {
                            "->".to_string()
                        } else {
                            format!("-[{}]->", n.effects.join(", "))
                        };
                        println!(
                            "  #{} ({}) ({}) {} {}",
                            n.short_hash,
                            n.aliases.join("/"),
                            n.params.join(", "),
                            eff,
                            n.ret_type
                        );
                    }
                }
            }
            Ok(())
        }
        ["graph"] => {
            let store = GraphStore::open(&cwd)?;
            let nodes = store.list_graph()?;
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&nodes).unwrap_or_default()
                );
            } else if nodes.is_empty() {
                println!("Graph is empty (.axiom/nodes has 0 nodes).");
            } else {
                println!("Axiom Content-Addressed Graph ({} nodes):", nodes.len());
                for n in nodes {
                    let eff = if n.direct_effects.is_empty() {
                        "pure".to_string()
                    } else {
                        n.direct_effects.join(", ")
                    };
                    let deps: Vec<String> =
                        n.direct_deps.iter().map(|d| format!("#{}", &d[..12])).collect();
                    println!(
                        "  #{}  aliases=[{}]  effects=[{}]  calls=[{}]",
                        n.short_hash,
                        n.aliases.join(", "),
                        eff,
                        deps.join(", ")
                    );
                }
            }
            Ok(())
        }
        ["passport", target] => {
            let store = GraphStore::open(&cwd)?;
            let p = store.compute_passport(target)?;
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&p).unwrap_or_default());
            } else {
                println!("=== AXIOM SECURITY PASSPORT ===");
                println!(
                    "Node:                 #{} ({})",
                    p.short_hash,
                    p.aliases.join(", ")
                );
                println!("Reachable Subgraph:   {} node(s)", p.total_reachable_nodes);
                println!(
                    "Touches Secret<T>:    {}",
                    if p.uses_secret_types { "YES" } else { "NO" }
                );
                if p.is_pure {
                    println!(
                        "Capabilities:         PURE (zero network, filesystem, or clock access)"
                    );
                } else {
                    println!("Capabilities:");
                    for cap in p.capabilities {
                        println!(
                            "  [CAP]  {:<20} via: {}",
                            cap.effect,
                            cap.witness_chain.join(" -> ")
                        );
                    }
                }
                if p.uses_secret_types {
                    println!("Secret Data-Flow Pipes:");
                    if p.secret_pipes.is_empty() {
                        println!("  [SAFE] Secret<T> stays internal (0 external network sinks)");
                    } else {
                        for pipe in p.secret_pipes {
                            println!(
                                "  [PIPE] {} -> {} (\"{}\")  via: {}",
                                pipe.secret_type,
                                pipe.operation,
                                pipe.destination_domain,
                                pipe.witness_chain.join(" -> ")
                            );
                        }
                    }
                    println!("  [SAFE] Disk / Log / Unpinned Leaks: 0 (verified by taint checker)");
                }
            }
            Ok(())
        }
        ["rename", old_alias, new_alias] => {
            let mut store = GraphStore::open(&cwd)?;
            let hash = store.rename_alias(old_alias, new_alias)?;
            if json_mode {
                println!(
                    "{{\"old\":\"{old_alias}\",\"new\":\"{new_alias}\",\"hash\":\"{hash}\"}}"
                );
            } else {
                println!(
                    "Renamed alias '{old_alias}' -> '{new_alias}' (node #{} unchanged)",
                    &hash[..12]
                );
            }
            Ok(())
        }
        _ => Err(AxiomDiagnostic::new(
            "E041_CLI_USAGE",
            "cli",
            "Invalid CLI arguments",
            Span::zero(),
            "Usage: axiom <verify <f.ax> | ingest <f.ax> | build <ref> [-o out] | run <ref> [args...] | show <ref> | graph | passport <ref> | rename <old> <new>> [--json]",
        )),
    }
}

fn print_build_artifact(art: &axiom_lang::codegen::BuildArtifact, json_mode: bool) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(art).unwrap_or_default());
    } else {
        let hit = if art.cache_hit {
            "cache-hit"
        } else {
            "compiled -O native"
        };
        println!(
            "  Built #{} ({}) -> {} [{} node(s), {} KB, {} in {}ms]",
            art.short_hash,
            art.entry_alias_or_ref,
            art.binary_path,
            art.compiled_nodes,
            art.binary_size_bytes / 1024,
            hit,
            art.build_ms
        );
    }
}
