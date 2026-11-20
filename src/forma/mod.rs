// Copyright (c) 2026 Axiom Language Contributors
// License: BUSL-1.1

//! `Axiom.Forma` — The fourth architectural layer of Axiom:
//! declarative spatial-interactive UI with Z3-verified geometry and zero-overhead
//! lowering into `Axiom.Flux` / `Axiom.Strata` 1-Draw-Call GPU primitive streams.

pub mod ast;
pub mod lower;
pub mod parser;
pub mod verify_ui;

pub use ast::{Align, FormaDef, FormaNode, FormaNodeKind, FormaProps};
pub use lower::{forma_runtime_helpers, lower_forma_def};
pub use verify_ui::{verify_forma, verify_module_formas, FormaVerifyReport, FORMA_MAX_PRIMS};
