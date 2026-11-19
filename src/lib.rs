#![deny(clippy::todo, clippy::unimplemented, dead_code, unused_must_use)]

pub mod ast;
pub mod canon;
pub mod codegen;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod showcase;
pub mod store;
pub mod typecheck;
pub mod verify;
pub mod wasm;
