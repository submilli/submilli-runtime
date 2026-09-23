//! `GET /v1/capabilities` — the semantic-security capability catalog: every
//! stdlib capability the runtime can gate (from
//! [`interpreter::stdlib::capabilities`], including the templated
//! `mcp.<server>` entry — clients detect templates by the `<`), plus the
//! capabilities each installed package provides.

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::{Value, json};
use submilli_build::PackageStore;

use crate::app::AppState;

pub async fn list(State(state): State<AppState>) -> impl IntoResponse {
    crate::metrics::capabilities();
    Json(capabilities_json(state.package_store()))
}

/// `{ groups: [{ source, kind: "stdlib" | "package", capabilities: [...] }] }`.
/// Each capability carries `filter_fields` (names, the uniform quick view) and
/// `fields` (`{ name: { type, description } }`, the full payload). Package
/// capabilities have no `example_filter` (their schema has none). Package
/// groups also carry `requires` — the capability rules the package itself
/// needs under its own caller block (add-package tooling must insert them);
/// packages with neither provides nor requires are omitted.
fn capabilities_json(store: &PackageStore) -> Value {
    let mut groups: Vec<Value> = interpreter::stdlib::capabilities::catalog()
        .iter()
        .map(|group| {
            let capabilities: Vec<Value> = group
                .capabilities
                .iter()
                .map(|cap| {
                    let fields: Value = cap
                        .filter_fields
                        .iter()
                        .map(|f| {
                            (
                                f.name.to_string(),
                                json!({ "type": f.ty, "description": f.doc }),
                            )
                        })
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                    json!({
                        "name": cap.name,
                        "summary": cap.summary,
                        "filter_fields": cap.field_names().collect::<Vec<_>>(),
                        "fields": fields,
                        "example_filter": cap.example_filter,
                        "main_denial": cap.main_denial,
                    })
                })
                .collect();
            json!({ "source": group.module, "kind": "stdlib", "capabilities": capabilities })
        })
        .collect();

    for name in store.available_packages() {
        let Ok(artifact) = store.load(&name) else {
            continue;
        };
        if artifact.capabilities.provides.is_empty() && artifact.capabilities.requires.is_empty() {
            continue;
        }
        let capabilities: Vec<Value> = artifact
            .capabilities
            .provides
            .iter()
            .map(|cap| {
                let fields: Value = cap
                    .fields
                    .iter()
                    .map(|(name, f)| {
                        (
                            name.clone(),
                            json!({ "type": f.ty, "description": f.description }),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>()
                    .into();
                json!({
                    "name": cap.name,
                    "summary": cap.description.clone().unwrap_or_default(),
                    "filter_fields": cap.fields.keys().collect::<Vec<_>>(),
                    "fields": fields,
                    "example_filter": Value::Null,
                })
            })
            .collect();
        let requires: Vec<Value> = artifact
            .capabilities
            .requires
            .iter()
            .map(|req| json!({ "capability": req.capability, "filter": req.filter }))
            .collect();
        groups.push(json!({
            "source": name,
            "kind": "package",
            "capabilities": capabilities,
            "requires": requires,
        }));
    }

    json!({ "groups": groups })
}
