// Copyright (c) 2026 Axiom Language Contributors
// License: BUSL-1.1

use axiom_lang::forma::verify_module_formas;
use axiom_lang::parser::{parse_module, parse_module_from_path};
use axiom_lang::store::GraphStore;
use axiom_lang::strata_wasm::build_strata_wasm_from_module_with_cache;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_workspace(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "axiom_sprint9_{tag}_{}_{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_forma_z3_proves_valid_composer_bar_layout() {
    let src = r#"
fn draw_input_text(_rx: i64, _ry: i64, _rw: i64, _rh: i64, _clip: i64) -[Mem]-> unit { () }
fn handle_input_wheel(_rx: i64, _ry: i64, _rw: i64, _rh: i64, _clip: i64) -[Mem]-> unit { () }
fn draw_attach_icon(_rx: i64, _ry: i64, _rw: i64, _rh: i64, _clip: i64) -[Mem]-> unit { () }
fn draw_model_label(_rx: i64, _ry: i64, _rw: i64, _rh: i64, _clip: i64) -[Mem]-> unit { () }
fn draw_send_icon(_rx: i64, _ry: i64, _rw: i64, _rh: i64, _clip: i64) -[Mem]-> unit { () }

forma ComposerBar(x: i64, y: i64, w: i64, h: i64, theme_bg: i64, border_col: i64)
  req w >= 280 && h >= 80
  -[Mem]-> i64
{
  Box(x: x, y: y, w: w, h: h, radius: 12, border: 1, bg: theme_bg, border_col: border_col, pad: 10) {
    Col(gap: 8, grow: 1) {
      Scroll(id: 3, grow: 1, clip: 3) {
        Slot(render: draw_input_text, on_wheel: handle_input_wheel)
      }
      Row(h: 34, gap: 8, align: center) {
        Button(id: 101, w: 32, h: 32, radius: 8, bg: theme_bg, hover_bg: border_col, on_click: 11) {
          Slot(render: draw_attach_icon)
        }
        Dropdown(id: 102, w: 170, h: 30, radius: 8, bg: theme_bg, hover_bg: border_col, on_click: 12) {
          Slot(render: draw_model_label)
        }
        Spacer(grow: 1)
        Button(id: 103, w: 34, h: 34, radius: 17, bg: theme_bg, hover_bg: border_col, on_click: 1) {
          Slot(render: draw_send_icon)
        }
      }
    }
  }
}
"#;

    let module = parse_module(src).expect("valid ComposerBar forma should parse");
    assert_eq!(module.formas.len(), 1);

    let forma_reports =
        verify_module_formas(&module).expect("Z3 should prove all geometry invariants");
    assert_eq!(forma_reports.len(), 1);
    assert_eq!(forma_reports[0].name, "ComposerBar");
    assert!(forma_reports[0].obligations_proved > 0);
    assert!(forma_reports[0].static_prim_bound < 80_000);

    let ws = temp_workspace("valid_composer");
    let store = GraphStore::open(&ws).unwrap();
    let fn_reports = store
        .verify_module_with_cache(&module, false)
        .expect("lowered ComposerBar should pass full Telos+Flux verification");
    assert!(fn_reports.iter().any(|r| r.alias == "ComposerBar"));
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_forma_z3_catches_row_overflow_with_counterexample() {
    // Row(w: 200, gap: 10) with three buttons of w: 80 => 80 + 10 + 80 + 10 + 80 = 260 > 200.
    let src = r#"
forma BrokenRow(bg_col: i64)
  -[Mem]-> i64
{
  Row(x: 0, y: 0, w: 200, h: 40, gap: 10) {
    Button(id: 1, w: 80, h: 32, bg: bg_col, on_click: 1)
    Button(id: 2, w: 80, h: 32, bg: bg_col, on_click: 2)
    Button(id: 3, w: 80, h: 32, bg: bg_col, on_click: 3)
  }
}
"#;

    let module = parse_module(src).expect("syntactically valid forma should parse");
    let err = verify_module_formas(&module).expect_err("Z3 must catch Row overflow");
    assert_eq!(err.code, "E090_FORMA_LAYOUT_VIOLATION");
    assert_eq!(err.phase, "forma-verify");
    assert!(
        err.message.contains("Bounds overflow"),
        "expected bounds overflow message, got: {}",
        err.message
    );
    assert!(
        !err.counterexample.is_empty(),
        "Z3 must provide a concrete counterexample"
    );

    // Also ensure GraphStore::verify_module_with_cache blocks it at compile time.
    let ws = temp_workspace("row_overflow");
    let store = GraphStore::open(&ws).unwrap();
    let store_err = store
        .verify_module_with_cache(&module, false)
        .expect_err("GraphStore must reject overflowing forma");
    assert_eq!(store_err.code, "E090_FORMA_LAYOUT_VIOLATION");
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_forma_z3_catches_button_collapse_non_degeneracy() {
    // When w == 200: pad=10 leaves iw=180; two fixed 90px spacers leave 0px for the flex Button!
    let src = r#"
forma CollapsingButton(w: i64, bg_col: i64)
  req w >= 200
  -[Mem]-> i64
{
  Box(x: 0, y: 0, w: w, h: 48, pad: 10) {
    Row(gap: 0) {
      Spacer(w: 90)
      Button(id: 1, grow: 1, h: 28, bg: bg_col, on_click: 1)
      Spacer(w: 90)
    }
  }
}
"#;

    let module = parse_module(src).expect("syntactically valid forma should parse");
    let err = verify_module_formas(&module)
        .expect_err("Z3 must catch Button collapsing to w <= 0 at w == 200");
    assert_eq!(err.code, "E090_FORMA_LAYOUT_VIOLATION");
    assert!(
        err.message.contains("Non-degeneracy violated"),
        "expected non-degeneracy violation, got: {}",
        err.message
    );
    assert_eq!(
        err.counterexample.get("w").map(String::as_str),
        Some("200"),
        "expected Z3 counterexample w = 200, got: {:?}",
        err.counterexample
    );
}

#[test]
fn test_forma_demo_compiles_to_axw_and_passes_axiom_web_headless_check() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let demo_path = manifest_dir.join("examples/forma_ui_demo.ax");
    let module = parse_module_from_path(&demo_path).expect("forma_ui_demo.ax should parse");
    assert_eq!(module.formas.len(), 1);

    let ws = temp_workspace("forma_demo_axw");
    let out_axw = ws.join("forma_ui_demo.axw");
    let mut store = GraphStore::open(&ws).unwrap();

    let artifact =
        build_strata_wasm_from_module_with_cache(&mut store, &module, &out_axw, false)
            .expect("forma_ui_demo.ax should verify via Z3 and compile to .axw");
    assert!(artifact.wasm_size_bytes > 0);
    assert!(artifact.exported_functions.contains(&"step_ui".to_string()));
    assert!(artifact
        .exported_functions
        .contains(&"init_app".to_string()));

    let axiom_web_bin = Path::new("/home/aminadav/Projects/axiom-web/target/release/axiom-web");
    assert!(
        axiom_web_bin.is_file(),
        "Expected axiom-web release binary at {}",
        axiom_web_bin.display()
    );

    let output = Command::new(axiom_web_bin)
        .arg(&out_axw)
        .arg("--headless-check")
        .output()
        .expect("failed to execute axiom-web --headless-check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "axiom-web --headless-check failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("HEADLESS_OK"),
        "expected HEADLESS_OK in output, got:\n{stdout}"
    );

    // Parse `Prims=<N>` and verify real GPU primitives were emitted (> 0).
    let prims: usize = stdout
        .split('|')
        .find_map(|part| part.trim().strip_prefix("Prims="))
        .and_then(|num| num.trim().parse().ok())
        .unwrap_or(0);
    assert!(
        prims > 0,
        "expected Prims > 0 from Forma 1-Draw-Call stream, got stdout:\n{stdout}"
    );

    let _ = fs::remove_dir_all(&ws);
}
