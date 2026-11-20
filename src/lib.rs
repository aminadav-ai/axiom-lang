#![deny(clippy::todo, clippy::unimplemented, dead_code, unused_must_use)]

pub mod abi;
pub mod ast;
pub mod canon;
pub mod codegen;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod showcase;
pub mod store;
pub mod strata_elf;
pub mod strata_wasm;
pub mod typecheck;
pub mod verify;
pub mod wasm;
