use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    CompileError,
    Timeout,
    FuelExhausted,
    MemoryExhausted,
    StackExhausted,
    Cancelled,
    /// A capability denial the runtime threw escaped the program. A denial the
    /// program constructed itself is a `RuntimeError`.
    PermissionDenied,
    RuntimeError,
    BlueprintNotFound,
    PackageResolution,
    /// The request itself is malformed for this blueprint — e.g. a required
    /// session variable was not supplied, or an undeclared one was.
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticNote {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticPayload {
    pub severity: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
}

/// Who was refused what, and by which layer: `policy` (the operator's
/// blueprint), `invariant` (refused by the runtime ahead of any policy), or
/// `read_only` (a write into a read-only volume).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenialDetails {
    pub caller: String,
    pub capability: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticPayload>,
    /// Present exactly when `kind` is `permission_denied`; its fields sit
    /// beside `kind` in the JSON.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub denial: Option<DenialDetails>,
}
