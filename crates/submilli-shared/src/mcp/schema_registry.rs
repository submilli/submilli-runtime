//! A curated, in-repo library of output schemas for popular public MCP servers.
//!
//! Most public MCP servers don't publish an `outputSchema`, so a tool's result
//! falls back to `unknown` unless a schema pack supplies a representable type (see
//! [`catalog`](super::catalog)). We can't make upstream servers
//! publish schemas, but we can vendor known ones and overlay them at discovery: a
//! server's *published* schema always wins, and a pack only fills the gap when the
//! server publishes nothing representable.
//!
//! A pack is matched to a blueprint server by the host of its `url` — not by the
//! operator's chosen `mcp:` name, which is arbitrary. The trade-off: only the
//! hosted remote endpoint matches; a self-hosted deployment on a custom host gets
//! no overlay.
//!
//! Schemas are stored as JSON Schema (the MCP `outputSchema` shape), so an overlaid
//! schema flows through the same `schema_to_type` path as a published one.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use serde_json::Value;

#[derive(Clone, Debug)]
pub enum SchemaPackError {
    InvalidJson {
        pack: &'static str,
        source: Arc<serde_json::Error>,
    },
    MissingTools {
        pack: &'static str,
    },
}

impl std::fmt::Display for SchemaPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { pack, .. } => write!(f, "schema pack '{pack}' is not valid JSON"),
            Self::MissingTools { pack } => write!(f, "schema pack '{pack}' has no tools object"),
        }
    }
}

impl std::error::Error for SchemaPackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson { source, .. } => Some(source.as_ref()),
            Self::MissingTools { .. } => None,
        }
    }
}

/// One server's curated output schemas, keyed by tool name. The JSON values mirror
/// the MCP `outputSchema` shape so they map through `schema_to_type` unchanged.
pub struct SchemaPack {
    id: &'static str,
    schemas: BTreeMap<String, Value>,
}

impl SchemaPack {
    /// The pack id, surfaced in tool docs so an agent knows the return is curated.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// The curated `outputSchema` for `tool`, if this pack covers it.
    pub fn output_schema(&self, tool: &str) -> Option<&Value> {
        self.schemas.get(tool)
    }

    /// Build a pack from canned schemas, for tests in sibling modules.
    #[cfg(test)]
    pub fn for_test(id: &'static str, schemas: BTreeMap<String, Value>) -> Self {
        Self { id, schemas }
    }

    /// Parse the compiled-in asset, retaining a typed failure for bad metadata.
    fn from_json(id: &'static str, raw: &str) -> Result<Self, SchemaPackError> {
        let doc: Value =
            serde_json::from_str(raw).map_err(|source| SchemaPackError::InvalidJson {
                pack: id,
                source: Arc::new(source),
            })?;
        let tools = doc
            .get("tools")
            .and_then(Value::as_object)
            .ok_or(SchemaPackError::MissingTools { pack: id })?;
        let schemas = tools
            .iter()
            .map(|(name, schema)| (name.clone(), schema.clone()))
            .collect();
        Ok(Self { id, schemas })
    }
}

static GITHUB_PACK: OnceLock<Result<SchemaPack, SchemaPackError>> = OnceLock::new();

/// Validate every built-in pack before serving requests or starting discovery.
/// Failed initialization is cached as a value, so repeated reads cannot poison it.
pub fn initialize_builtin_packs() -> Result<(), SchemaPackError> {
    github_pack().map(|_| ())
}

fn github_pack() -> Result<&'static SchemaPack, SchemaPackError> {
    initialized_pack(&GITHUB_PACK, "github", include_str!("schemas/github.json"))
}

fn initialized_pack<'a>(
    cell: &'a OnceLock<Result<SchemaPack, SchemaPackError>>,
    id: &'static str,
    raw: &str,
) -> Result<&'a SchemaPack, SchemaPackError> {
    cell.get_or_init(|| SchemaPack::from_json(id, raw))
        .as_ref()
        .map_err(Clone::clone)
}

/// Unknown hosts and malformed URLs have no overlay; broken built-in assets fail.
pub fn pack_for_url(url: &str) -> Result<Option<&'static SchemaPack>, SchemaPackError> {
    initialize_builtin_packs()?;
    let Ok(url) = url::Url::parse(url) else {
        return Ok(None);
    };
    let Some(host) = url.host_str() else {
        return Ok(None);
    };
    match host.to_ascii_lowercase().as_str() {
        "api.githubcopilot.com" => github_pack().map(Some),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::catalog::schema_to_type;
    use interpreter::Type;

    #[test]
    fn failed_initialization_is_cached_without_poisoning_and_keeps_its_cause() {
        use std::error::Error;
        let cell = OnceLock::new();
        let first = initialized_pack(&cell, "broken", "{").err().unwrap();
        let second = initialized_pack(&cell, "broken", r#"{"tools":{}}"#)
            .err()
            .unwrap();
        assert!(first.source().is_some());
        match (first, second) {
            (
                SchemaPackError::InvalidJson { source: a, .. },
                SchemaPackError::InvalidJson { source: b, .. },
            ) => assert!(Arc::ptr_eq(&a, &b)),
            errors => panic!("unexpected errors: {errors:?}"),
        }
        let healthy = OnceLock::new();
        assert_eq!(
            initialized_pack(&healthy, "healthy", r#"{"tools":{}}"#)
                .unwrap()
                .id(),
            "healthy"
        );
    }

    #[test]
    fn missing_and_non_object_tools_are_typed_failures() {
        for raw in ["{}", r#"{"tools":null}"#, r#"{"tools":[]}"#] {
            assert!(matches!(
                SchemaPack::from_json("broken", raw),
                Err(SchemaPackError::MissingTools { pack: "broken" })
            ));
        }
    }

    #[test]
    fn concurrent_failed_initialization_remains_readable() {
        let cell = OnceLock::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let cell = &cell;
                scope.spawn(move || {
                    assert!(matches!(
                        initialized_pack(cell, "broken", "{"),
                        Err(SchemaPackError::InvalidJson { .. })
                    ));
                });
            }
        });
        assert!(initialized_pack(&cell, "broken", "{").is_err());
    }

    #[test]
    fn github_pack_parses_and_covers_read_only_tools() {
        let pack = github_pack().unwrap();
        assert_eq!(pack.id(), "github");
        for tool in ["get_me", "list_issues", "get_label", "search_code"] {
            assert!(
                pack.output_schema(tool).is_some(),
                "github pack should cover `{tool}`"
            );
        }
    }

    #[test]
    fn every_pack_schema_is_representable_and_structured() {
        let pack = github_pack().unwrap();
        for (tool, schema) in &pack.schemas {
            let ty = schema_to_type(schema, 0)
                .unwrap_or_else(|| panic!("github tool `{tool}` schema is not representable"));
            assert!(
                !matches!(ty, Type::String),
                "github tool `{tool}` must map to a structured type, got a bare string"
            );
        }
    }

    #[test]
    fn pack_for_url_matches_github_host() {
        assert!(
            pack_for_url("https://api.githubcopilot.com/mcp/")
                .unwrap()
                .is_some()
        );
        // Case-insensitive host, port and path ignored.
        assert!(
            pack_for_url("https://API.GithubCopilot.com:443/mcp")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn pack_for_url_misses_unknown_and_malformed() {
        assert!(
            pack_for_url("https://mcp.linear.app/mcp")
                .unwrap()
                .is_none()
        );
        assert!(pack_for_url("not a url").unwrap().is_none());
    }

    /// Structurally validate `value` against `ty`, mirroring what `JSON.parse(...) as T`
    /// does at runtime: required fields must be present and well-typed, an absent
    /// optional field is fine, a present field of the wrong type fails, and extra
    /// fields are ignored (see `tests/fixtures/json/parse_object_lenient.subm` and
    /// `casts/as_nested_and_optional.subm`). Returns the failing path on mismatch.
    fn value_matches_type(ty: &Type, value: &Value, path: &str) -> Result<(), String> {
        let err = |want: &str| format!("{path}: expected {want}, got {value}");
        let ensure = |ok: bool, want: &str| if ok { Ok(()) } else { Err(err(want)) };
        match ty {
            Type::String => ensure(value.is_string(), "string"),
            Type::StringLiteral(lit) => match value {
                Value::String(s) if s == lit => Ok(()),
                _ => Err(format!("{path}: expected \"{lit}\", got {value}")),
            },
            Type::Number => ensure(value.is_number(), "number"),
            Type::Boolean => ensure(value.is_boolean(), "boolean"),
            Type::Null => ensure(value.is_null(), "null"),
            Type::Array(elem) => {
                let items = value.as_array().ok_or_else(|| err("array"))?;
                for (i, item) in items.iter().enumerate() {
                    value_matches_type(elem, item, &format!("{path}[{i}]"))?;
                }
                Ok(())
            }
            Type::Object { fields, .. } => {
                let obj = value.as_object().ok_or_else(|| err("object"))?;
                for (name, field) in fields {
                    match obj.get(name) {
                        Some(v) => value_matches_type(&field.ty, v, &format!("{path}.{name}"))?,
                        None if field.optional => {}
                        None => return Err(format!("{path}: missing required field `{name}`")),
                    }
                }
                Ok(())
            }
            Type::Union(members) => ensure(
                members
                    .iter()
                    .any(|m| value_matches_type(m, value, path).is_ok()),
                "a union member",
            ),
            other => Err(format!("{path}: validator does not handle {other:?}")),
        }
    }

    /// Recorded GitHub MCP server responses (the deployed hosted server returns the
    /// minimal shapes from `pkg/github/minimal_types.go`). The six `list_*` tools
    /// emit bare arrays via `json.Marshal([]Minimal*)`; `list_issues` and the
    /// `search_*` tools emit wrapper objects. The pack's declared type must parse
    /// each — this is the guard against re-vendoring wrapper shapes that don't match
    /// what the server emits (the SUB-571 regression).
    fn golden_server_responses() -> Vec<(&'static str, Value)> {
        use serde_json::json;
        let user = json!({ "login": "octocat", "id": 1 });
        vec![
            (
                "list_commits",
                json!([{
                    "sha": "abc123", "html_url": "https://github.com/o/r/commit/abc123",
                    "commit": { "message": "fix", "author": { "name": "A", "email": "a@x", "date": "2026-01-01T00:00:00Z" } },
                    "author": user, "committer": user
                }]),
            ),
            (
                "list_branches",
                json!([{ "name": "main", "sha": "abc123", "protected": false }]),
            ),
            ("list_tags", json!([{ "name": "v1.0.0", "sha": "abc123" }])),
            (
                "list_releases",
                json!([{
                    "id": 42, "tag_name": "v1.0.0", "name": "v1.0.0",
                    "html_url": "https://github.com/o/r/releases/tag/v1.0.0",
                    "prerelease": false, "draft": false, "author": user
                }]),
            ),
            (
                "list_pull_requests",
                json!([{
                    "number": 7, "title": "Add feature", "state": "open",
                    "draft": false, "merged": false, "html_url": "https://github.com/o/r/pull/7",
                    "user": user,
                    "head": { "ref": "feature", "sha": "head1" },
                    "base": { "ref": "main", "sha": "base1" }
                }]),
            ),
            (
                "list_issue_types",
                json!([{
                    "id": 1, "node_id": "IT_1", "name": "Bug",
                    "description": "A bug", "color": "red",
                    "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
                }]),
            ),
            (
                "list_issues",
                json!({
                    "issues": [{ "number": 3, "title": "Broken", "state": "open" }],
                    "totalCount": 1,
                    "pageInfo": { "hasNextPage": false, "hasPreviousPage": false }
                }),
            ),
            (
                "search_repositories",
                json!({
                    "total_count": 1, "incomplete_results": false,
                    "items": [{
                        "id": 1, "name": "r", "full_name": "o/r",
                        "html_url": "https://github.com/o/r",
                        "stargazers_count": 0, "forks_count": 0, "open_issues_count": 0,
                        "private": false, "fork": false, "archived": false
                    }]
                }),
            ),
            // search_issues / search_pull_requests marshal the *raw* github.Issue
            // search result: `labels`/`assignees` are object arrays and `milestone`
            // is an object — not the strings the unmerged PR declared.
            (
                "search_issues",
                json!({
                    "total_count": 1, "incomplete_results": false,
                    "items": [{
                        "number": 3, "title": "Broken", "state": "open", "user": user,
                        "labels": [{ "id": 1, "name": "bug", "color": "d73a4a", "default": true }],
                        "assignees": [user],
                        "milestone": { "number": 1, "title": "v1", "state": "open" }
                    }]
                }),
            ),
            (
                "search_pull_requests",
                json!({
                    "total_count": 1, "incomplete_results": false,
                    "items": [{
                        "number": 7, "title": "Add feature", "state": "open", "user": user,
                        "labels": [{ "id": 2, "name": "enhancement", "color": "a2eeef" }],
                        "assignees": [user],
                        "milestone": { "title": "v1" }
                    }]
                }),
            ),
        ]
    }

    #[test]
    fn declared_types_parse_recorded_server_responses() {
        let pack = github_pack().unwrap();
        for (tool, response) in golden_server_responses() {
            let schema = pack
                .output_schema(tool)
                .unwrap_or_else(|| panic!("github pack should cover `{tool}`"));
            let ty = schema_to_type(schema, 0)
                .unwrap_or_else(|| panic!("github tool `{tool}` schema is not representable"));
            value_matches_type(&ty, &response, "$").unwrap_or_else(|e| {
                panic!("github tool `{tool}` declared type rejects real server output — {e}")
            });
        }
    }

    #[test]
    fn list_endpoints_declare_bare_arrays_not_wrappers() {
        let pack = github_pack().unwrap();
        // These six tools marshal bare arrays on the wire; declaring an object
        // wrapper (the unmerged-PR shape that caused SUB-571) makes every typed
        // call trap at the JSON.parse boundary.
        for tool in [
            "list_commits",
            "list_branches",
            "list_tags",
            "list_releases",
            "list_pull_requests",
            "list_issue_types",
        ] {
            let ty = schema_to_type(pack.output_schema(tool).unwrap(), 0).unwrap();
            assert!(
                matches!(ty, Type::Array(_)),
                "github tool `{tool}` must declare a bare array, got {ty:?}"
            );
            // A wrapper object is exactly what the server does NOT send, so the
            // declared type must reject it — locks the fix against re-vendoring.
            let wrapper = serde_json::json!({ "items": [] });
            assert!(
                value_matches_type(&ty, &wrapper, "$").is_err(),
                "github tool `{tool}` should reject a wrapper object"
            );
        }
    }
}
