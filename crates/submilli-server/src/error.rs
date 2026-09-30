use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    CompileError,
    Timeout,
    FuelExhausted,
    MemoryExhausted,
    RuntimeError,
    BlueprintNotFound,
    PackageResolution,
    /// The request itself is malformed for this blueprint — e.g. a required
    /// session variable was not supplied, or an undeclared one was.
    InvalidRequest,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticNote {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticPayload {
    pub severity: &'static str,
    pub line: u32,
    pub column: u32,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticPayload>,
}
