#![deny(clippy::todo, clippy::unimplemented, dead_code, unused_must_use)]

pub mod ast;
pub mod canon;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod store;
pub mod typecheck;
pub mod verify;
