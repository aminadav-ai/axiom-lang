use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn zero() -> Self {
        Self {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticData {
    pub code: String,
    pub phase: String,
    pub message: String,
    pub span: Span,
    pub repair_hint: String,
}

/// Pointer-sized (8-byte) machine-readable diagnostic for fast Result passing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AxiomDiagnostic(Box<DiagnosticData>);

impl Deref for AxiomDiagnostic {
    type Target = DiagnosticData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AxiomDiagnostic {
    pub fn new(
        code: impl Into<String>,
        phase: impl Into<String>,
        message: impl Into<String>,
        span: Span,
        repair_hint: impl Into<String>,
    ) -> Self {
        Self(Box::new(DiagnosticData {
            code: code.into(),
            phase: phase.into(),
            message: message.into(),
            span,
            repair_hint: repair_hint.into(),
        }))
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| {
            format!(
                "{{\"code\":\"{}\",\"message\":\"{}\"}}",
                self.code, self.message
            )
        })
    }
}

impl fmt::Display for AxiomDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] ({}:{}): {} | hint: {}",
            self.code, self.span.line, self.span.col, self.message, self.repair_hint
        )
    }
}

impl std::error::Error for AxiomDiagnostic {}
