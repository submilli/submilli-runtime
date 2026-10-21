//! The real packages under the runtime's blueprint policy, with an in-process
//! Linear transport. No sockets, credentials, or live Linear mutations.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use interpreter::runtime::{
    LinkedPackageModule, SecretProvider, StoreData, Vfs, install_package_modules_async,
    install_runtime_host_functions, install_runtime_store_bound, install_tenant_limits,
};
use interpreter::stdlib::http::transport::{
    DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse,
};
use interpreter::{
    CompiledPackage, FileId, ModulePath, PackageSourceModule, RuntimeConfig, compile_package,
    compile_script, dispatch_main_async,
};
use serde_json::{Value, json};
use submilli_shared::PolicyCheck;
use wasmtime::{Linker, Module};

const LINEAR: &str = include_str!("../../../packages/linear/src/lib.ts");
const AGENT: &str = include_str!("../../../packages/linear-agent/src/lib.ts");
const CALLS: &[(&str, &str)] = &[
    (
        "thought",
        "thought(\"s\", \"Working\", { ephemeral: true })",
    ),
    (
        "action",
        "action(\"s\", \"Searching\", \"issues\", { result: \"\" })",
    ),
    ("ask", "ask(\"s\", \"Which repo?\")"),
    ("respond", "respond(\"s\", \"Done\")"),
    ("fail", "fail(\"s\", \"Failed\")"),
    (
        "askChoice",
        "askChoice(\"s\", \"Choose\", [{ label: \"Backend\", value: \"backend\" }])",
    ),
    (
        "askAccountConnection",
        "askAccountConnection(\"s\", \"Connect\", \"https://example.com/auth\", { userId: \"u\" })",
    ),
    ("getSession", "getSession(\"s\")"),
    (
        "listActivities",
        "listActivities(\"s\", { first: 10, after: \"cursor\" })",
    ),
    ("setExternalLinks", "setExternalLinks(\"s\", [])"),
    (
        "addExternalLinks",
        "addExternalLinks(\"s\", [{ label: \"PR\", url: \"https://example.com/pr\" }])",
    ),
    (
        "removeExternalLinks",
        "removeExternalLinks(\"s\", [\"https://example.com/pr\"])",
    ),
];

#[tokio::test]
async fn linear_agent_checks_both_callers_and_teams_in_the_native_runtime() {
    let packages = compile_packages();
    check_rejected_calls(&packages);
    let calls = CALLS
        .iter()
        .map(|(_, call)| format!("{call};"))
        .collect::<String>();
    let allowed = run(&packages, &calls, "", "team", true).await;
    assert!(allowed.result.is_ok(), "{:?}", allowed.result);
    assert_eq!(allowed.requests.len(), CALLS.len() * 2);
    let mutation = allowed
        .requests
        .iter()
        .find(|r| {
            r["query"]
                .as_str()
                .unwrap()
                .contains("agentActivityCreate(")
        })
        .unwrap();
    assert_eq!(mutation["variables"]["input"]["ephemeral"], true);

    // The wrapper gates main before even the metadata lookup, for every helper.
    let denied_main = denial_body("caller=main", "linear-agent.");
    let outcome = run(&packages, &denied_main, "main", "team", true).await;
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert!(outcome.requests.is_empty());

    // Allowed main intent does not grant the wrapper permission to use Linear.
    let denied_dependency = denial_body("@submilli/linear-agent", "linear.app/");
    let outcome = run(&packages, &denied_dependency, "dependency", "team", true).await;
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert_eq!(outcome.requests.len(), CALLS.len());
    assert!(outcome.requests.iter().all(is_team_lookup));

    // A real team filter on the dependency grant blocks reads and mutations.
    let outcome = run(&packages, &denied_dependency, "", "other-team", true).await;
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert_eq!(outcome.requests.len(), CALLS.len());
    assert!(outcome.requests.iter().all(is_team_lookup));

    // Credential/HTTP grants remain attributed to linear, never the wrapper.
    for (mode, capability) in [("secret", "secrets.get"), ("http", "http.post")] {
        let body = denial_body("@submilli/linear", capability);
        let outcome = run(&packages, &body, mode, "team", true).await;
        assert!(outcome.result.is_ok(), "{:?}", outcome.result);
        assert!(outcome.requests.is_empty());
    }

    let failures = CALLS.iter().filter(|(name, _)| !matches!(*name, "getSession" | "listActivities"))
        .map(|(_, call)| format!("{{ let failed = false; try {{ {call}; }} catch (e) {{ failed = e.message.includes(\"did not succeed\"); }} assert(failed, \"mutation failure is propagated\"); }}"))
        .collect::<Vec<_>>().join("\n");
    let outcome = run(&packages, &failures, "", "team", false).await;
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
}

fn compile_packages() -> Vec<CompiledPackage> {
    let linear = compile_package(
        "@submilli/linear",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: LINEAR,
        }],
        &[],
    )
    .unwrap_or_else(|errors| panic!("linear: {errors:#?}"));
    let agent = compile_package(
        "@submilli/linear-agent",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: AGENT,
        }],
        &[&linear.declaration],
    )
    .unwrap_or_else(|errors| panic!("linear-agent: {errors:#?}"));
    let schema = submilli_build::derive_capability_schema(
        &agent.declaration,
        &[&linear.declaration],
        &agent.required_capabilities,
    );
    assert_eq!(schema.provides.len(), CALLS.len());
    for (name, _) in CALLS {
        let capability = schema
            .provides
            .iter()
            .find(|c| c.name == format!("linear-agent.{name}"))
            .unwrap();
        assert!(capability.fields.contains_key("sessionId"));
    }
    let required: Vec<_> = schema
        .requires
        .iter()
        .map(|r| r.capability.as_str())
        .collect();
    assert_eq!(
        required,
        [
            "linear.app/createAgentActivity",
            "linear.app/getAgentSession",
            "linear.app/listAgentActivities",
            "linear.app/updateAgentSession"
        ]
    );
    vec![linear, agent]
}

fn check_rejected_calls(packages: &[CompiledPackage]) {
    let declarations: Vec<_> = packages.iter().map(|p| &p.declaration).collect();
    for call in [
        "respond(\"s\", \"b\", { ephemeral: true })",
        "thought(\"s\", \"b\", { signal: \"auth\" })",
        "ask(\"s\", \"b\", { ephemeral: true })",
        "fail(\"s\", \"b\", { signal: \"select\" })",
        "askChoice(\"s\", \"b\", [{ value: \"v\" }])",
        "askAccountConnection(\"s\", \"b\", \"https://example.com\", { ephemeral: true })",
    ] {
        let source = script(call);
        assert!(
            compile_script(&source, "invalid.ts", FileId(0), &declarations, &[]).is_err(),
            "must reject {call}"
        );
    }
}

fn script(body: &str) -> String {
    let imports = CALLS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "import {{ {imports} }} from \"@submilli/linear-agent\";\nfunction main(): void {{ {body} }}"
    )
}

fn denial_body(caller: &str, capability: &str) -> String {
    CALLS.iter().map(|(_, call)| format!(
        "{{ let refused = false; try {{ {call}; }} catch (e) {{ refused = e.name === \"PermissionDeniedError\" && e.message.includes(\"{caller}\") && e.message.includes(\"{capability}\"); }} assert(refused, \"expected denial\"); }}"
    )).collect::<Vec<_>>().join("\n")
}

struct Outcome {
    result: Result<Option<String>, String>,
    requests: Vec<Value>,
}

async fn run(
    packages: &[CompiledPackage],
    body: &str,
    deny: &str,
    team: &str,
    success: bool,
) -> Outcome {
    let declarations: Vec<_> = packages.iter().map(|p| &p.declaration).collect();
    let source = script(body);
    let script = compile_script(&source, "main.ts", FileId(0), &declarations, &[])
        .unwrap_or_else(|errors| panic!("script: {errors:#?}"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine_async().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut data = StoreData::with_vfs(Vfs::tempdir().unwrap());
    data.install_type_info(script.type_info.clone());
    data.http_client = Arc::new(LinearTransport {
        requests: requests.clone(),
        team: team.into(),
        success,
    });
    data.secret_provider = Arc::new(Token);
    data.security_check = Arc::new(PolicyCheck::new(Arc::new(blueprint(deny))));
    let mut store = cfg.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(&engine);
    install_runtime_host_functions(&mut linker).unwrap();
    install_runtime_store_bound(&mut linker, &mut store).unwrap();
    let modules: Vec<_> = packages
        .iter()
        .map(|p| Module::new(&engine, &p.wasm).unwrap())
        .collect();
    let linked: Vec<_> = packages
        .iter()
        .zip(&modules)
        .map(|(p, module)| LinkedPackageModule {
            module,
            declaration: &p.declaration,
            type_info: &p.type_info,
        })
        .collect();
    install_package_modules_async(&mut linker, &mut store, &linked)
        .await
        .unwrap();
    let module = Module::new(&engine, &script.wasm).unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let result = dispatch_main_async(&mut store, &instance)
        .await
        .map_err(|e| format!("{e:#}"));
    let requests = requests.lock().unwrap().clone();
    Outcome { result, requests }
}

fn blueprint(deny: &str) -> submilli_blueprint::Blueprint {
    let mut rules = String::from(
        "kind: blueprint\nname: linear-agent-test\npackages: ['@submilli/linear-agent']\ndefault: deny\npermissions:\n  main:\n",
    );
    for (name, _) in CALLS {
        rules.push_str(&format!("    - {{ capability: linear-agent.{name}, action: {}, filter: 'sessionId == \"s\"' }}\n", if deny == "main" { "deny" } else { "allow" }));
    }
    rules.push_str("  '@submilli/linear-agent':\n");
    for cap in [
        "createAgentActivity",
        "getAgentSession",
        "listAgentActivities",
        "updateAgentSession",
    ] {
        rules.push_str(&format!(
            "    - {{ capability: linear.app/{cap}, action: {}, filter: 'teamId == \"team\"' }}\n",
            if deny == "dependency" {
                "deny"
            } else {
                "allow"
            }
        ));
    }
    rules.push_str("  '@submilli/linear':\n");
    for (cap, filter, mode) in [
        ("secrets.get", "name == \"LINEAR_API_KEY\"", "secret"),
        (
            "http.post",
            "host == \"api.linear.app\" and path == \"/graphql\"",
            "http",
        ),
    ] {
        rules.push_str(&format!(
            "    - {{ capability: {cap}, action: {}, filter: '{filter}' }}\n",
            if deny == mode { "deny" } else { "allow" }
        ));
    }
    submilli_blueprint::parse(&rules).unwrap()
}

struct Token;
impl SecretProvider for Token {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>> {
        assert_eq!(name, "LINEAR_API_KEY");
        Box::pin(async { Ok(Some("oauth-test-token".into())) })
    }
}

struct LinearTransport {
    requests: Arc<Mutex<Vec<Value>>>,
    team: String,
    success: bool,
}

#[async_trait::async_trait]
impl HttpClient for LinearTransport {
    async fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        assert_eq!(request.url, "https://api.linear.app/graphql");
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                    && value == "Bearer oauth-test-token")
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        self.requests.lock().unwrap().push(body.clone());
        let data = self.response(&body);
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: serde_json::to_vec(&json!({ "data": data })).unwrap(),
            final_url: request.url.clone(),
        })
    }

    async fn download(
        &self,
        _: &HttpRequest,
        _: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        panic!("agent sessions must not download files")
    }
}

impl LinearTransport {
    fn response(&self, body: &Value) -> Value {
        if is_team_lookup(body) {
            return json!({ "agentSession": { "issue": { "team": { "id": self.team } }, "comment": null } });
        }
        let session = json!({ "id": "s", "status": "active", "url": null, "summary": null,
            "issue": { "id": "i" }, "comment": null, "externalUrls": [], "plan": null });
        let query = body["query"].as_str().unwrap();
        if query.contains("agentActivityCreate(") {
            let input = &body["variables"]["input"];
            return json!({ "agentActivityCreate": { "success": self.success, "agentActivity": {
                "id": "a", "createdAt": "2026-01-01T00:00:00Z", "content": input["content"],
                "ephemeral": input.get("ephemeral").unwrap_or(&Value::Bool(false)),
                "signal": input["signal"], "signalMetadata": input["signalMetadata"] } } });
        }
        if query.contains("agentSessionUpdate(") {
            return json!({ "agentSessionUpdate": { "success": self.success, "agentSession": session } });
        }
        if query.contains("activities(") {
            return json!({ "agentSession": { "activities": { "nodes": [{ "id": "prompt", "createdAt": "t", "ephemeral": false, "content": { "type": "prompt", "body": "Stop" }, "signal": "stop", "signalMetadata": null }], "pageInfo": { "hasNextPage": false, "endCursor": null } } } });
        }
        json!({ "agentSession": session })
    }
}

fn is_team_lookup(body: &Value) -> bool {
    body["query"].as_str().unwrap().contains("team { id }")
}

#[test]
fn linear_agent_discovery_and_blueprint_scaffolding_include_the_dependency() {
    let home = tempfile::tempdir().unwrap();
    cli(
        home.path(),
        &["build", "publish-local", "-p", "@submilli/linear-agent"],
    );
    let store = submilli_build::PackageStore::new(home.path().join("packages"));
    let closure = store.load_closure(["@submilli/linear-agent"]).unwrap();
    let names: Vec<_> = closure
        .iter()
        .map(|p| p.metadata.package_name.as_str())
        .collect();
    assert_eq!(names, ["@submilli/linear", "@submilli/linear-agent"]);

    let path = home.path().join("blueprint.yaml");
    std::fs::write(&path, "kind: blueprint\nname: linear-agent-discovery\ndefault: deny\nsecrets:\n  LINEAR_API_KEY:\n    harness:\n      required: true\n").unwrap();
    cli(
        home.path(),
        &[
            "blueprint",
            "add-package",
            "@submilli/linear-agent",
            "--all-capabilities",
            "--blueprint",
            path.to_str().unwrap(),
        ],
    );
    let blueprint = submilli_blueprint::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        blueprint
            .packages
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["@submilli/linear-agent"]
    );
    assert_eq!(blueprint.permissions["main"].len(), CALLS.len());
    assert_eq!(blueprint.permissions["@submilli/linear-agent"].len(), 4);
    assert!(
        blueprint.permissions["@submilli/linear-agent"]
            .iter()
            .all(|r| r.capability.starts_with("linear.app/"))
    );
    assert_eq!(blueprint.permissions["@submilli/linear"].len(), 2);
    cli(
        home.path(),
        &[
            "blueprint",
            "lint",
            "--deny-warnings",
            path.to_str().unwrap(),
        ],
    );
}

fn cli(home: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(args)
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
