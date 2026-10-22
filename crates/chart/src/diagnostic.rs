use std::fmt::Display;

use crate::Chart;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

/// A parser message owned by one invocation, never shared through global logging state.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub format: &'static str,
    pub line: Option<usize>,
    pub message: String,
}

impl Diagnostic {
    pub fn information_loss(format: &'static str, line: usize, detail: impl Display) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            format,
            line: Some(line),
            message: format!(
                "{format}: unsupported notation was omitted; information was lost: {detail}"
            ),
        }
    }
}

/// Warnings are retained even if a subsequent record causes parsing to fail.
#[derive(Debug)]
pub struct ParseReport<E> {
    pub result: Result<Chart, E>,
    pub diagnostics: Vec<Diagnostic>,
}

impl<E> ParseReport<E> {
    pub fn map_err<F>(self, map: impl FnOnce(E) -> F) -> ParseReport<F> {
        ParseReport {
            result: self.result.map_err(map),
            diagnostics: self.diagnostics,
        }
    }

    /// Preserves the legacy parser API's stderr reporting.
    pub fn into_result(self) -> Result<Chart, E> {
        for diagnostic in self.diagnostics {
            eprintln!("{}", diagnostic.message);
        }
        self.result
    }
}
