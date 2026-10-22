use std::ffi::{CString, c_char};

use chart::{Diagnostic, DiagnosticSeverity};

#[repr(u32)]
pub enum CcDiagnosticSeverity {
    Warning,
    Error,
}

#[repr(C)]
pub struct CcDiagnostic {
    pub severity: CcDiagnosticSeverity,
    pub format: *const c_char,
    pub message: *const c_char,
    /// One-based source line, or zero if unknown.
    pub line: usize,
}

#[repr(C)]
#[derive(Default)]
pub struct CcDiagnosticsView {
    pub diagnostics: *const CcDiagnostic,
    pub count: usize,
}

pub(super) struct OwnedDiagnostics {
    entries: Vec<CcDiagnostic>,
    _strings: Vec<(CString, CString)>,
}

impl OwnedDiagnostics {
    pub fn new(diagnostics: Vec<Diagnostic>) -> Self {
        let strings: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    CString::new(diagnostic.format).expect("static format name"),
                    CString::new(diagnostic.message.replace('\0', "\\0")).expect("no embedded NUL"),
                )
            })
            .collect();
        let entries = diagnostics
            .iter()
            .zip(&strings)
            .map(|(diagnostic, (format, message))| CcDiagnostic {
                severity: match diagnostic.severity {
                    DiagnosticSeverity::Warning => CcDiagnosticSeverity::Warning,
                    DiagnosticSeverity::Error => CcDiagnosticSeverity::Error,
                },
                format: format.as_ptr(),
                message: message.as_ptr(),
                line: diagnostic.line.unwrap_or(0),
            })
            .collect();
        Self {
            entries,
            _strings: strings,
        }
    }

    pub fn view(&self) -> CcDiagnosticsView {
        CcDiagnosticsView {
            diagnostics: self.entries.as_ptr(),
            count: self.entries.len(),
        }
    }
}
