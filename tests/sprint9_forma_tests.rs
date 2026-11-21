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

    let wasm_bytes = fs::read(&out_axw).expect("compiled .axw should exist");
    assert!(
        wasm_bytes.starts_with(b"\0asm"),
        "expected valid WebAssembly binary header"
    );

    let axiom_web_bin = std::env::var_os("AXIOM_WEB_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            manifest_dir
                .parent()
                .map(|p| p.join("axiom-web/target/release/axiom-web"))
        })
        .filter(|p| p.is_file());

    if let Some(axiom_web_bin) = axiom_web_bin {
        let output = Command::new(&axiom_web_bin)
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
    } else {
        eprintln!(
            "Skipping axiom-web --headless-check (sibling axiom-web binary not present in this environment)"
        );
    }

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_forma2_fit_content_and_justify_space_between() {
    // Reproduces the Axiom Studio Native sidebar footer ("Настройки" + "119.2 МБ")
    // using Forma 2.0 `w: fit`, `h: fit`, and `justify: space_between`.
    let src = r#"
forma SidebarFooter(sb_w: i64, bg_col: i64, text_col: i64, green_col: i64)
  req sb_w >= 280
  -[Mem]-> i64
{
  Row(x: 0, y: 0, w: sb_w, h: 42, pad_x: 12, justify: space_between, align: center, bg: bg_col) {
    Button(id: 1, w: fit, h: fit, pad_x: 8, pad_y: 6, radius: 6, bg: bg_col, on_click: 1) {
      Text("Настройки", size: 14, color: text_col)
    }
    Text("119.2 МБ", size: 13, color: green_col)
  }
}
"#;

    let module = parse_module(src).expect("Forma 2.0 SidebarFooter should parse");
    let reports = verify_module_formas(&module)
        .expect("Z3 should prove SidebarFooter with w:fit, h:fit, justify:space_between");
    assert_eq!(reports.len(), 1);
    assert!(reports[0].obligations_proved > 0);

    let ws = temp_workspace("forma2_sidebar_footer");
    let store = GraphStore::open(&ws).unwrap();
    let fn_reports = store
        .verify_module_with_cache(&module, false)
        .expect("lowered Forma 2.0 SidebarFooter should pass Telos+Flux verification");
    assert!(fn_reports.iter().any(|r| r.alias == "SidebarFooter"));
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_forma2_shrink_and_ellipsis_prevents_overflow() {
    // Reproduces the "Ход работы" bar with arbitrary dynamic summary text on the left
    // and fixed right label ("развернуть"). `shrink: 1` guarantees no overflow for ANY string.
    let src = r#"
forma TimelineHeader(bar_w: i64, summary: str, bg_col: i64, text_col: i64, accent_col: i64)
  req bar_w >= 240
  -[Mem]-> i64
{
  Row(x: 0, y: 0, w: bar_w, h: 32, pad_x: 12, gap: 8, align: center, bg: bg_col) {
    Text(summary, size: 12, color: text_col, shrink: 1, overflow: ellipsis)
    Spacer(grow: 1)
    Text("развернуть", size: 12, color: accent_col)
  }
}
"#;

    let module = parse_module(src).expect("Forma 2.0 TimelineHeader should parse");
    let reports = verify_module_formas(&module)
        .expect("Z3 must prove TimelineHeader with shrink:1 + overflow:ellipsis never overflows");
    assert_eq!(reports.len(), 1);
    assert!(reports[0].obligations_proved > 0);
}

#[test]
fn test_forma2_z3_catches_unbounded_dynamic_text_overflow() {
    // Without `shrink: 1` or `max_w`, an arbitrary dynamic string inside a fixed-width Row
    // CAN overflow the right edge — Z3 must catch this at compile time!
    let src = r#"
forma UnsafeTimelineHeader(bar_w: i64, summary: str, bg_col: i64, text_col: i64, accent_col: i64)
  req bar_w >= 240
  -[Mem]-> i64
{
  Row(x: 0, y: 0, w: bar_w, h: 32, pad_x: 12, gap: 8, align: center, bg: bg_col) {
    Text(summary, size: 12, color: text_col)
    Spacer(grow: 1)
    Text("развернуть", size: 12, color: accent_col)
  }
}
"#;

    let module = parse_module(src).expect("UnsafeTimelineHeader should parse");
    let err = verify_module_formas(&module)
        .expect_err("Z3 must catch unbounded dynamic Text overflow in Row");
    assert_eq!(err.code, "E090_FORMA_LAYOUT_VIOLATION");
    assert!(
        err.message.contains("Bounds overflow"),
        "expected bounds overflow message, got: {}",
        err.message
    );
}

#[test]
fn test_forma2_full_capsule_compiles_and_runs_headless() {
    let src = r#"
forma Forma2Demo(vw: i64, vh: i64, summary: str)
  req vw >= 320 && vh >= 240
  -[Mem]-> i64
{
  Col(x: 0, y: 0, w: vw, h: vh, pad: 16, gap: 12, bg: 4279309080) {
    Row(h: 36, pad_x: 12, gap: 8, align: center, radius: 8, bg: 4280296747) {
      Text(summary, size: 13, color: 4293849848, shrink: 1, overflow: ellipsis)
      Spacer(grow: 1)
      Text("развернуть", size: 13, color: 4284193791)
    }
    Box(grow: 1, pad: 12, radius: 8, bg: 4279769635) {
      Text("Длинный многострочный текст в Axiom.Forma 2.0 с автоматическим переносом строк внутри контейнера.", size: 14, color: 4293849848, grow: 1, overflow: wrap)
    }
    Row(h: 40, pad_x: 12, justify: space_between, align: center, radius: 8, bg: 4280296747) {
      Button(id: 1, w: fit, h: fit, pad_x: 10, pad_y: 6, radius: 6, bg: 4281415488, on_click: 1) {
        Text("Настройки", size: 14, color: 4294967295)
      }
      Text("119.2 МБ", size: 13, color: 4281856625)
    }
  }
}

export fn init_app() -[Mem]-> unit {
  ()
}

export fn step_ui(vw: i64, vh: i64, mx: i64, my: i64, m_down: i64, m_clicked: i64, wheel_dy: i64, key_char: i64, key_special: i64) -[Mem, Gpu]-> i64 {
  mem_store(12, 0);
  mem_store(48100, mx);
  mem_store(48101, my);
  mem_store(48102, m_clicked);
  mem_store(48103, wheel_dy);
  let w = if vw < 320 { 320 } else { vw };
  let h = if vh < 240 { 240 } else { vh };
  let _ = Forma2Demo(w, h, "Ход работы: 76 шагов (посл. 32) за 4 мин 30 с — очень длинный текст");
  mem_load(12)
}

export fn get_cmd_byte_offset() -[Mem]-> i64 { 65536 * 8 }
export fn get_clips_byte_offset() -[Mem]-> i64 { 48000 * 8 }
export fn get_prim_count() -[Mem]-> i64 { mem_load(12) }
export fn get_cursor_kind() -[Mem]-> i64 { 0 }
export fn axw_unicode_atlas() -[Mem]-> i64 { 1 }
"#;

    let module = parse_module(src).expect("Forma2Demo should parse");
    let ws = temp_workspace("forma2_full_capsule");
    let out_axw = ws.join("forma2_demo.axw");
    let mut store = GraphStore::open(&ws).unwrap();

    let artifact =
        build_strata_wasm_from_module_with_cache(&mut store, &module, &out_axw, false)
            .expect("Forma2Demo should verify via Z3 and compile to .axw");
    assert!(artifact.wasm_size_bytes > 0);

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let axiom_web_bin = std::env::var_os("AXIOM_WEB_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            manifest_dir
                .parent()
                .map(|p| p.join("axiom-web/target/release/axiom-web"))
        })
        .filter(|p| p.is_file());

    if let Some(axiom_web_bin) = axiom_web_bin {
        let output = Command::new(&axiom_web_bin)
            .arg(&out_axw)
            .arg("--headless-check")
            .output()
            .expect("failed to execute axiom-web --headless-check");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("HEADLESS_OK"),
            "Forma2Demo headless-check failed:\nstdout: {stdout}\nstderr: {stderr}"
        );
        let prims: usize = stdout
            .split('|')
            .find_map(|part| part.trim().strip_prefix("Prims="))
            .and_then(|num| num.trim().parse().ok())
            .unwrap_or(0);
        assert!(
            prims > 20,
            "expected > 20 GPU primitives from Forma2Demo text+rects, got {prims}\n{stdout}"
        );
    }

    let _ = fs::remove_dir_all(&ws);
}
