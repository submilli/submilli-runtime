//! The application tools a harness declares for a session: the catalog behind
//! the session's `@tools` package. No mapping or transport here.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// At most this many tools per session.
pub const MAX_TOOLS: usize = 256;
/// At most this many bytes of catalog JSON per session.
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;

/// One tool as the harness declares it: the shape of an MCP `tools/list` entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool's single `args` object.
    pub input_schema: serde_json::Value,
    /// JSON Schema for the result. Without one, the tool returns `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

/// A session's tool catalog, fixed when the session is created.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTools(Arc<[DeclaredTool]>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsRuleError {
    TooMany,
    TooLarge,
    Malformed(String),
}

impl std::fmt::Display for ToolsRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany => write!(f, "a session may declare at most {MAX_TOOLS} tools"),
            Self::TooLarge => write!(
                f,
                "the tools catalog may be at most {MAX_CATALOG_BYTES} bytes of JSON"
            ),
            Self::Malformed(detail) => write!(f, "the stored tools catalog is malformed: {detail}"),
        }
    }
}

impl std::error::Error for ToolsRuleError {}

impl SessionTools {
    /// `None` for an empty catalog: a session that declares no tools has none.
    pub fn new(tools: Vec<DeclaredTool>) -> Result<Option<Self>, ToolsRuleError> {
        if tools.is_empty() {
            return Ok(None);
        }
        if tools.len() > MAX_TOOLS {
            return Err(ToolsRuleError::TooMany);
        }
        let tools = Self(tools.into());
        if tools.to_json().len() > MAX_CATALOG_BYTES {
            return Err(ToolsRuleError::TooLarge);
        }
        Ok(Some(tools))
    }

    pub fn tools(&self) -> &[DeclaredTool] {
        &self.0
    }

    /// The stored form: the declared JSON array.
    pub fn to_json(&self) -> String {
        // Strings and JSON values only, so serializing cannot fail; the
        // fallback exists for totality.
        serde_json::to_string(&*self.0).unwrap_or_else(|_| "[]".to_owned())
    }

    pub fn from_json(json: &str) -> Result<Option<Self>, ToolsRuleError> {
        let tools: Vec<DeclaredTool> = serde_json::from_str(json)
            .map_err(|error| ToolsRuleError::Malformed(error.to_string()))?;
        Ok((!tools.is_empty()).then(|| Self(tools.into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> DeclaredTool {
        DeclaredTool {
            name: name.to_owned(),
            description: None,
            input_schema: serde_json::json!({ "type": "object" }),
            output_schema: None,
        }
    }

    #[test]
    fn the_catalog_round_trips_and_an_empty_one_is_none() {
        assert_eq!(SessionTools::new(Vec::new()), Ok(None));
        let tools = SessionTools::new(vec![tool("a"), tool("b")])
            .unwrap()
            .unwrap();
        assert_eq!(SessionTools::from_json(&tools.to_json()), Ok(Some(tools)));
        assert_eq!(SessionTools::from_json("[]"), Ok(None));
    }

    #[test]
    fn oversized_catalogs_are_refused() {
        let many = (0..=MAX_TOOLS).map(|i| tool(&format!("t{i}"))).collect();
        assert_eq!(SessionTools::new(many), Err(ToolsRuleError::TooMany));
        let mut large = tool("big");
        large.description = Some("x".repeat(MAX_CATALOG_BYTES));
        assert_eq!(
            SessionTools::new(vec![large]),
            Err(ToolsRuleError::TooLarge)
        );
    }
}
