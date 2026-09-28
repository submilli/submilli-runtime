//! Typed failures shared by compiler phases and compilation entry points.

use crate::{Diagnostic, FileId, Severity, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerStage {
    Parse,
    Infer,
    Codegen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilerFailure {
    Limit {
        stage: CompilerStage,
        span: Option<Span>,
        message: String,
        help: Vec<String>,
    },
    Internal {
        stage: CompilerStage,
        span: Option<Span>,
        message: String,
    },
}

impl std::fmt::Display for CompilerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (category, stage, message) = match self {
            Self::Limit { stage, message, .. } => ("compiler limit", stage, message),
            Self::Internal { stage, message, .. } => ("internal compiler failure", stage, message),
        };
        write!(f, "{category} during {stage:?}: {message}")
    }
}

impl std::error::Error for CompilerFailure {}

/// Diagnostics accumulated before a fatal phase failure are retained alongside
/// its typed cause. Source errors have no fatal cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub diagnostics: Vec<Diagnostic>,
    pub fatal: Option<CompilerFailure>,
}

impl CompileError {
    pub fn with_prior_diagnostics(mut self, diagnostics: &[Diagnostic]) -> Self {
        self.diagnostics.splice(0..0, diagnostics.iter().cloned());
        self
    }

    /// Explicit compatibility adapter for APIs that historically returned only
    /// diagnostics. Source-less failures retain a virtual compiler location.
    pub fn into_diagnostics(mut self, _file: FileId) -> Vec<Diagnostic> {
        if let Some(fatal) = self.fatal {
            let span = match &fatal {
                CompilerFailure::Limit { span, .. } | CompilerFailure::Internal { span, .. } => {
                    *span
                }
            };
            let (message, help) = match &fatal {
                CompilerFailure::Limit { message, help, .. } => (message.clone(), help.clone()),
                CompilerFailure::Internal { .. } => (
                    fatal.to_string(),
                    vec!["report this compiler error with the source program".into()],
                ),
            };
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: span.unwrap_or(Span::at(FileId::COMPILER)),
                message,
                help,
                notes: Vec::new(),
            });
        }
        self.diagnostics
    }
}

impl From<Vec<Diagnostic>> for CompileError {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            fatal: None,
        }
    }
}

impl From<CompilerFailure> for CompileError {
    fn from(fatal: CompilerFailure) -> Self {
        Self {
            diagnostics: Vec::new(),
            fatal: Some(fatal),
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(fatal) = &self.fatal {
            return fatal.fmt(f);
        }
        write!(
            f,
            "compilation failed with {} diagnostics",
            self.diagnostics.len()
        )
    }
}

impl std::error::Error for CompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.fatal
            .as_ref()
            .map(|error| error as &dyn std::error::Error)
    }
}
