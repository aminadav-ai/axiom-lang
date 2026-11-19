#![deny(clippy::todo, clippy::unimplemented, dead_code, unused_must_use)]

use axiom_lang::diag::{AxiomDiagnostic, Span};
use axiom_lang::parser::parse_module;
use axiom_lang::store::GraphStore;
use std::env;
use std::fs;
use std::path::PathBuf;
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
                    let tag = if item.deduplicated {
                        "dedup (identical semantics)"
                    } else {
                        "stored"
                    };
                    println!("  #{}  {:<22} [{}]", item.short_hash, item.alias, tag);
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
                println!("=== ПАСПОРТ БЕЗОПАСНОСТИ AXIOM ===");
                println!(
                    "Узел:        #{} ({})",
                    p.short_hash,
                    p.aliases.join(", ")
                );
                println!("Узлов в графе вызова: {}", p.total_reachable_nodes);
                println!(
                    "Работа с Secret<T>:   {}",
                    if p.uses_secret_types { "ДА" } else { "НЕТ" }
                );
                if p.is_pure {
                    println!("Права / Эффекты:      🚫 ЧИСТАЯ ФУНКЦИЯ (сеть, диск и часы физически отсутствуют)");
                } else {
                    println!("Права / Эффекты:");
                    for cap in p.capabilities {
                        println!(
                            "  ⚠️  {:<18} путь: {}",
                            cap.effect,
                            cap.witness_chain.join(" -> ")
                        );
                    }
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
            "Usage: axiom <ingest <file.ax> | show <ref> | graph | passport <ref> | rename <old> <new>> [--json]",
        )),
    }
}
