//! The eight MCP tools: execute, the last run, package and built-in docs, and
//! the two file tools.
//!
//! Copied from `submilli-server`'s `mcp/server.rs`: the argument structs, whose
//! doc comments are the input schemas' descriptions, argument normalization, the
//! `invalid_arguments` result, the execution id in execute's results, the file
//! read window and list cap, and the file tools' one-decision runs. Tool
//! descriptions come from `submilli_shared::prompt`, except the two file tools',
//! which the server writes inline and are copied here. The host keeps no audit
//! and no event hub, so neither is here.

use std::borrow::Cow;
use std::sync::Arc;

use interpreter::runtime::fs::{ContainError, ContentPath, guest_normalize, resolve_content};
use interpreter::runtime::security::AuditDecision;
use interpreter::runtime::{CallSite, CheckOutcome, EntryPath, SecurityCheck, Vfs};
use interpreter::stdlib::fs::handles::kind_of;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::{Extension, ToolCallContext};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorCode, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};
use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, VarBindings, VfsConfig, required_harness_secrets,
};
use submilli_shared::PolicyCheck;
use submilli_shared::docs_scope::DocsScope;

use super::super::blueprints::AppliedBlueprint;
use super::super::run::{
    DenialDetails, ErrorKind, ExecuteError, FinishedRun, Recording, RunEntry, RunRequest, RunStart,
    VfsSource,
};
use super::super::sessions::LastRun;
use super::packages;
use super::{Caller, Shared};

const TOOL_NAME: &str = "submilli__typescript__execute";

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExecuteArgs {
    /// TypeScript-subset source with a `main()` entry point. Its return value
    /// is the tool result.
    code: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PackageDocsArgs {
    /// Package name, e.g. `submilli:crypto`.
    name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PackageSearchArgs {
    /// Substring matched against module names, descriptions, and exported
    /// symbols. Empty lists every available package.
    #[serde(default)]
    query: String,
}

/// Default and clamp for `files.read`: how many lines a single call returns, and
/// the per-line character cap that keeps one pathological line from dominating.
const READ_DEFAULT_LIMIT: u32 = 2000;
const READ_MAX_LIMIT: u32 = 5000;
const READ_MAX_LINE_CHARS: usize = 2000;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadFileArgs {
    /// VFS path to read, e.g. `"/workspace/search.md"`. Resolved under the session
    /// workspace root; `..` segments that escape it are rejected.
    path: String,
    /// 1-based line to start at. Defaults to 1.
    #[serde(default)]
    offset: Option<u32>,
    /// Maximum lines to return (clamped to 5000). Defaults to 2000.
    #[serde(default)]
    limit: Option<u32>,
}

/// One window of a file. `has_more` and `next_offset` drive pagination; lines
/// longer than the per-line cap are truncated and flagged by `truncated_lines`.
#[derive(Serialize)]
struct ReadFileOutput {
    path: String,
    content: String,
    line_start: u32,
    line_end: u32,
    returned_lines: u32,
    has_more: bool,
    next_offset: Option<u32>,
    truncated_lines: u32,
    bytes: u64,
}

/// Cap on entries a single `files.list` returns, so a directory with thousands of
/// files can't produce an unbounded result.
const LIST_MAX_ENTRIES: usize = 1000;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListFilesArgs {
    /// Directory to list, e.g. `"/workspace"`. Defaults to the workspace root.
    #[serde(default)]
    path: Option<String>,
    /// Recurse into subdirectories. Defaults to false (top level only).
    #[serde(default)]
    recursive: Option<bool>,
}

/// One VFS entry. `kind` is the same vocabulary `submilli:fs` reports from guest
/// code — `"file"`, `"directory"`, `"symlink"`, `"other"`. `bytes` is the size of
/// a regular file and 0 for everything else.
#[derive(Serialize)]
struct FileEntry {
    path: String,
    kind: &'static str,
    bytes: u64,
}

#[derive(Serialize)]
struct ListFilesOutput {
    path: String,
    entries: Vec<FileEntry>,
    count: usize,
    truncated: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct BuiltinsDocsArgs {
    /// Built-in names to look up, e.g. `["Array", "Map", "Temporal"]`. Each
    /// returns its declarations; unknown names come back with an `error` entry.
    names: Vec<String>,
}

/// The `{ result, console, error }` shape of `execute`.
#[derive(Serialize)]
struct ExecuteOutput {
    result: Option<String>,
    console: Vec<String>,
    error: Option<ExecuteError>,
}

/// The client's name from its `initialize`, for the run's record.
#[derive(Clone)]
struct McpClient(String);

/// The client's id for the tool call, for the run's record.
#[derive(Clone)]
struct McpToolCall(String);

/// The `mcp-session-id` header, present on every request in stateful mode.
fn session_header(parts: &axum::http::request::Parts) -> Option<String> {
    parts
        .headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn caller(parts: &axum::http::request::Parts) -> String {
    parts
        .extensions
        .get::<Caller>()
        .map_or_else(|| "unauthenticated".to_owned(), |caller| caller.0.clone())
}

/// Wrap a tool's output as its structured result.
fn structured_result(output: impl Serialize) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured(tool_value(output)?))
}

fn tool_value(output: impl Serialize) -> Result<serde_json::Value, ErrorData> {
    serde_json::to_value(output).map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

#[derive(Clone)]
pub(crate) struct SubmilliMcp {
    tool_router: ToolRouter<Self>,
    shared: Arc<Shared>,
    blueprint_name: String,
}

impl SubmilliMcp {
    pub(super) fn new(shared: Arc<Shared>, blueprint_name: String) -> Self {
        Self {
            tool_router: Self::tool_router(),
            shared,
            blueprint_name,
        }
    }

    /// The blueprint in force, for a tool that cannot work without one.
    async fn require_blueprint(&self) -> Result<AppliedBlueprint, ErrorData> {
        self.shared
            .blueprint(&self.blueprint_name)
            .await
            .ok_or_else(|| {
                ErrorData::invalid_request(
                    format!("unknown blueprint: {}", self.blueprint_name),
                    None,
                )
            })
    }

    /// The session's bound variables; none for a request with no open session.
    async fn variables(&self, session_id: Option<&str>) -> Result<Arc<VarBindings>, ErrorData> {
        let Some(id) = session_id else {
            return Ok(Arc::default());
        };
        let session = self
            .shared
            .sessions
            .get(id)
            .await
            .map_err(|error| ErrorData::internal_error(format!("{error:#}"), None))?;
        Ok(Arc::new(
            session.map(|session| session.variables).unwrap_or_default(),
        ))
    }

    /// Open the VFS a file tool reads. `per_session` resolves the session's
    /// directory (and keeps the session alive); every other mode gets a
    /// standalone VFS per call, so only an `ephemeral` root is a fresh temp
    /// directory each time.
    async fn acquire_vfs(
        &self,
        blueprint: &Blueprint,
        session_id: Option<&str>,
        variables: &VarBindings,
    ) -> Result<Vfs, ErrorData> {
        let session_root = if matches!(blueprint.vfs, VfsConfig::PerSession { .. }) {
            let sid = session_id
                .ok_or_else(|| ErrorData::invalid_request("missing mcp-session-id", None))?;
            let open = self
                .shared
                .sessions
                .touch(sid)
                .await
                .map_err(|error| ErrorData::internal_error(format!("{error:#}"), None))?;
            if !open {
                return Err(ErrorData::internal_error(
                    "unknown or expired session",
                    None,
                ));
            }
            Some(self.shared.sessions.session_dir(sid))
        } else {
            None
        };
        self.shared
            .volumes
            .build_vfs(blueprint, variables, session_root.as_deref())
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }
}

#[tool_router]
impl SubmilliMcp {
    #[tool(
        name = "submilli__typescript__execute",
        description = "Compile and run a TypeScript-subset program in the sandbox. \
            Provide a `main()` entry point; its return value is the result. \
            Returns { result, console, error }."
    )]
    async fn execute(
        &self,
        Parameters(args): Parameters<ExecuteArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let applied = self.require_blueprint().await?;
        let blueprint = Arc::clone(&applied.blueprint);
        // Stateful transport: every connection has a session id (rmcp rejects a
        // non-initialize request without one before we get here).
        let session_id = session_header(&parts).unwrap_or_default();
        let session = self
            .shared
            .sessions
            .get(&session_id)
            .await
            .map_err(|error| ErrorData::internal_error(format!("{error:#}"), None))?
            .ok_or_else(|| ErrorData::internal_error("unknown or expired session", None))?;
        let harness_secrets = match self.shared.sessions.secrets(&session_id) {
            Some(secrets) => secrets,
            None if required_harness_secrets(&blueprint.secrets).is_empty() => {
                Arc::new(HarnessSecretBindings::new())
            }
            None => {
                return Err(ErrorData::invalid_request(
                    "session_requires_secrets",
                    Some(serde_json::json!({
                        "required": required_harness_secrets(&blueprint.secrets),
                    })),
                ));
            }
        };
        let output = self
            .shared
            .run
            .run(RunRequest {
                session_id,
                code: args.code,
                label: caller(&parts),
                entry: RunEntry::Mcp,
                blueprint: applied,
                variables: Arc::new(session.variables),
                harness_secrets,
                client: parts.extensions.get::<McpClient>().map(|c| c.0.clone()),
                tool_call_id: parts.extensions.get::<McpToolCall>().map(|c| c.0.clone()),
                test_of: None,
                vfs: VfsSource::Session,
                finalizers: None,
                world: None,
            })
            .await;
        let response = output.response;
        let execution_id = response.execution_id.clone();
        if !output.dispatched {
            // Nothing ran: a protocol-level failure, as it always was for MCP.
            let message = response
                .error
                .map(|error| error.message)
                .unwrap_or_default();
            return with_execution_id(Err(ErrorData::internal_error(message, None)), &execution_id);
        }
        with_execution_id(
            structured_result(ExecuteOutput {
                result: response.result,
                console: response.console,
                error: response.error,
            }),
            &execution_id,
        )
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`.
    #[tool(name = "submilli__typescript__last_run")]
    async fn last_run(
        &self,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = session_header(&parts)
            .ok_or_else(|| ErrorData::invalid_request("no session for lastRun", None))?;
        match self.shared.sessions.last_run(&session_id).await {
            Ok(Some(LastRun::Kept { json, .. })) => {
                let value: serde_json::Value = serde_json::from_slice(&json)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::structured(value))
            }
            Ok(Some(LastRun::TooLarge { bytes, limit, .. })) => Err(ErrorData::invalid_request(
                format!(
                    "the last run's output ({bytes} bytes) is over the retained limit \
                     ({limit} bytes)"
                ),
                None,
            )),
            Ok(None) => Err(ErrorData::invalid_request(
                "no previous run in this session",
                None,
            )),
            Err(_) => Err(ErrorData::internal_error(
                "last-run storage unavailable",
                None,
            )),
        }
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`.
    #[tool(name = "submilli__typescript__packages__docs")]
    async fn packages_docs(
        &self,
        Parameters(args): Parameters<PackageDocsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let lookup = match self.shared.blueprint(&self.blueprint_name).await {
            Some(applied) => {
                self.shared
                    .run
                    .refresh_package_discovery(&applied.blueprint, Some(&args.name))
                    .await
                    .map_err(|message| ErrorData::internal_error(message, None))?;
                let catalog = self.catalog(&applied.blueprint).await?;
                let scope = self.docs_scope(&applied.blueprint);
                packages::lookup_with_blueprint(&args.name, &catalog, &scope)
            }
            None => packages::lookup(&args.name),
        };
        if let Some(markdown) = packages::docs_markdown(&lookup) {
            Ok(CallToolResult::success(vec![Content::text(markdown)]))
        } else {
            let value = packages::docs_json(&args.name, lookup);
            Ok(CallToolResult::structured(value))
        }
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`.
    #[tool(name = "submilli__typescript__packages__search")]
    async fn packages_search(
        &self,
        Parameters(args): Parameters<PackageSearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = match self.shared.blueprint(&self.blueprint_name).await {
            Some(applied) => {
                self.shared
                    .run
                    .refresh_package_discovery(&applied.blueprint, None)
                    .await
                    .map_err(|message| ErrorData::internal_error(message, None))?;
                let catalog = self.catalog(&applied.blueprint).await?;
                let scope = self.docs_scope(&applied.blueprint);
                packages::search_json_with_blueprint(&args.query, &catalog, &scope)
            }
            None => packages::search_json(&args.query),
        };
        Ok(CallToolResult::structured(value))
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`.
    #[tool(name = "submilli__typescript__builtins__docs")]
    async fn builtins_docs(
        &self,
        Parameters(args): Parameters<BuiltinsDocsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // The bound blueprint's `@mcp/*` names, so a package name asked of this
        // tool gets the correcting call rather than a bare unknown.
        let (mcp_packages, scope) = match self.shared.blueprint(&self.blueprint_name).await {
            Some(applied) => {
                let catalog = self.catalog(&applied.blueprint).await?;
                (
                    packages::mcp_package_names(&catalog),
                    Some(self.docs_scope(&applied.blueprint)),
                )
            }
            None => (Vec::new(), None),
        };
        Ok(CallToolResult::structured(packages::builtins_docs_json(
            &args.names,
            &mcp_packages,
            scope
                .as_ref()
                .unwrap_or_else(|| DocsScope::unscoped_shared()),
        )))
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`.
    #[tool(name = "submilli__typescript__builtins__list")]
    async fn builtins_list(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::structured(packages::builtins_list_json()))
    }

    #[tool(
        name = "submilli__files__read",
        description = "Read a file from the session workspace (VFS) in line windows \
            — the smart way to consume a large payload an `execute` run wrote to \
            disk (e.g. via jina `downloadSearch`/`downloadRead`) without pulling it \
            back through a single huge result. Args: `path`, optional `offset` \
            (1-based line, default 1) and `limit` (lines, default 2000). Returns \
            { content, line_start, line_end, has_more, next_offset, bytes }; pass \
            `next_offset` back to page on. Files survive across calls under a \
            per_session or named root and under a mounted volume; an \
            ephemeral root is emptied after every execute. Requires fs.read \
            as caller main with op readText. A denial or a missing file returns \
            { error: { kind, message } }."
    )]
    async fn read_file(
        &self,
        Parameters(args): Parameters<ReadFileArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let applied = self.require_blueprint().await?;
        let session_id = session_header(&parts);
        let variables = self.variables(session_id.as_deref()).await?;
        if let Err(refused) = self.authorize_file_call(
            &parts,
            &applied,
            Arc::clone(&variables),
            FileTool {
                name: "submilli__files__read",
                capability: "fs.read",
            },
            &args.path,
            None,
        ) {
            return file_tool_failure(refused);
        }
        let vfs = self
            .acquire_vfs(&applied.blueprint, session_id.as_deref(), &variables)
            .await?;

        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args
            .limit
            .unwrap_or(READ_DEFAULT_LIMIT)
            .clamp(1, READ_MAX_LIMIT);
        let read = resolve_content(&vfs, vfs.cwd(), &args.path)
            .and_then(|resolved| read_window(&args.path, &resolved, offset, limit));
        match read {
            Ok(output) => structured_result(output),
            Err(error) => file_tool_failure(FileToolError::file(&args.path, error)),
        }
    }

    #[tool(
        name = "submilli__files__list",
        description = "List files in the session workspace (VFS). Use it to discover \
            what an `execute` run wrote to disk before reading with `submilli__files__read`. \
            Args: optional `path` (directory, default the working directory) and \
            `recursive` (default false). Returns { entries: [{ path, kind, bytes }], \
            count, truncated }, capped at 1000 entries. Files survive across calls \
            under a per_session or named root and under a mounted volume; an \
            ephemeral root is emptied after every execute. Requires fs.list as \
            caller main with op list; policy checks the starting directory, \
            including recursive listings. A denial or a missing directory returns \
            { error: { kind, message } }."
    )]
    async fn list_files(
        &self,
        Parameters(args): Parameters<ListFilesArgs>,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let applied = self.require_blueprint().await?;
        let session_id = session_header(&parts);
        let variables = self.variables(session_id.as_deref()).await?;
        let dir = args.path.as_deref().unwrap_or(".");
        let recursive = args.recursive.unwrap_or(false);
        if let Err(refused) = self.authorize_file_call(
            &parts,
            &applied,
            Arc::clone(&variables),
            FileTool {
                name: "submilli__files__list",
                capability: "fs.list",
            },
            dir,
            Some(recursive),
        ) {
            return file_tool_failure(refused);
        }
        let vfs = self
            .acquire_vfs(&applied.blueprint, session_id.as_deref(), &variables)
            .await?;

        let listed = resolve_content(&vfs, vfs.cwd(), dir).and_then(|resolved| {
            list_entries(
                &resolved,
                &guest_dir_prefix(&guest_normalize(vfs.cwd(), dir)?),
                recursive,
            )
        });
        let (mut entries, truncated) = match listed {
            Ok(listed) => listed,
            Err(error) => return file_tool_failure(FileToolError::file(dir, error)),
        };
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        structured_result(ListFilesOutput {
            path: dir.to_string(),
            count: entries.len(),
            truncated,
            entries,
        })
    }
}

impl SubmilliMcp {
    async fn catalog(
        &self,
        blueprint: &Arc<Blueprint>,
    ) -> Result<Arc<submilli_shared::mcp::McpCatalog>, ErrorData> {
        self.shared
            .run
            .blueprint_mcp_catalog(blueprint)
            .await
            .map_err(|message| ErrorData::internal_error(message, None))
    }

    /// What `blueprint` lets programs see, from the packages installed now.
    /// The playground serves one author, so it builds this per call rather
    /// than caching it.
    fn docs_scope(&self, blueprint: &Blueprint) -> DocsScope {
        let artifacts = submilli_shared::docs_scope::load_declared(
            &blueprint.packages,
            &self.shared.package_store,
        );
        DocsScope::for_blueprint(blueprint, &artifacts)
    }

    /// File tools act as the session's main program, including on isolated VFSs.
    ///
    /// The path is checked lexically before the policy sees it. The policy refuses
    /// a path that cannot name anything in the VFS (`..` past the root, a NUL byte)
    /// too, but as a denial, which tells the model a rule stopped it rather than
    /// that the path is wrong.
    fn authorize_file_call(
        &self,
        parts: &axum::http::request::Parts,
        applied: &AppliedBlueprint,
        variables: Arc<VarBindings>,
        tool: FileTool<'_>,
        path: &str,
        recursive: Option<bool>,
    ) -> Result<(), FileToolError> {
        let capability = tool.capability;
        let blueprint = &applied.blueprint;
        let mut context = serde_json::Map::new();
        context.insert("path".into(), path.into());
        if let Some(recursive) = recursive {
            context.insert("recursive".into(), recursive.into());
        }
        let context = serde_json::Value::Object(context);
        let config = blueprint
            .vfs
            .resolve(&variables)
            .map_err(|error| FileToolError::denied(capability, &error.to_string()))?;
        guest_normalize(config.cwd(), path)
            .map_err(|error| FileToolError::file(path, error.into()))?;
        let recording = self.file_tool_recording(parts, applied, &variables, tool.name);
        let log = recording.as_ref().map(Recording::log);
        let policy: Arc<dyn SecurityCheck> = Arc::new(PolicyCheck::with_variables(
            Arc::clone(blueprint),
            variables,
        ));
        let policy = log.as_ref().map_or(policy.clone(), |log| log.wrap(policy));
        let call = policy.recorder().map(|recorder| {
            let marker = recorder.enter_host_call();
            (
                marker,
                recorder.begin_call("main", capability, Default::default()),
            )
        });
        let outcome = policy.check_with_cwd("main", capability, &context, config.cwd());
        let audit_context = policy.audit_context(capability, &context, config.cwd());
        let (allowed, rule, refusal) = match outcome {
            CheckOutcome::Allow { rule } => (true, rule, None),
            CheckOutcome::Deny { reason, rule } => (false, rule, Some(reason)),
            // A file tool cannot wait for an approver, so an ask is refused.
            CheckOutcome::Ask { rule, reason, .. } => (false, Some(rule.index), Some(reason)),
            _ => (false, None, Some("unrecognized policy outcome".to_string())),
        };
        if let (Some(recording), Some(log), Some((marker, ticket))) = (recording, log, call) {
            finish_file_tool_run(
                &recording,
                &log,
                &FileToolCall {
                    policy: policy.as_ref(),
                    tool,
                    context: &context,
                    audit_context: audit_context.as_ref(),
                    cwd: config.cwd(),
                    allowed,
                    rule,
                    refusal: refusal.as_deref(),
                    marker,
                    ticket,
                },
            );
        }
        match refusal {
            None => Ok(()),
            Some(reason) => Err(FileToolError::denied(capability, &reason)),
        }
    }

    /// A file tool's decision is a run of its own, when the recorder keeps it.
    fn file_tool_recording(
        &self,
        parts: &axum::http::request::Parts,
        applied: &AppliedBlueprint,
        variables: &Arc<VarBindings>,
        tool: &str,
    ) -> Option<Recording> {
        self.shared.run.record(RunStart {
            execution_id: uuid::Uuid::new_v4().to_string(),
            label: caller(parts),
            entry: RunEntry::McpFileTool { tool: tool.into() },
            test_of: None,
            client: parts.extensions.get::<McpClient>().map(|c| c.0.clone()),
            tool_call_id: parts.extensions.get::<McpToolCall>().map(|c| c.0.clone()),
            session_id: session_header(parts).unwrap_or_default(),
            blueprint_name: self.blueprint_name.clone(),
            blueprint_version: applied.version_tag.clone(),
            variables: Arc::clone(variables),
            // A file tool runs no program and reads no secret.
            harness_secrets: Arc::default(),
            code: None,
        })
    }
}

/// A file tool: its name as the record gives it, and the capability it is gated by.
#[derive(Clone, Copy)]
struct FileTool<'a> {
    name: &'a str,
    capability: &'a str,
}

/// One file tool call and the policy's decision on it, with what the run's record needs.
struct FileToolCall<'a> {
    policy: &'a dyn SecurityCheck,
    tool: FileTool<'a>,
    /// The call's context as the tool built it, for the policy to explain.
    context: &'a serde_json::Value,
    /// The context the policy actually tested, for the record.
    audit_context: &'a serde_json::Value,
    cwd: &'a str,
    allowed: bool,
    rule: Option<usize>,
    /// The denial's reason; `None` when allowed.
    refusal: Option<&'a str>,
    /// Where the call's host-call span began, and its ticket.
    marker: u64,
    ticket: interpreter::runtime::CallTicket,
}

/// Records the decision and finishes the run. A file tool's run is its one decision: it
/// ends here, so the read's own I/O and its outcome are not part of it.
fn finish_file_tool_run(
    recording: &Recording,
    log: &interpreter::runtime::DecisionLog,
    call: &FileToolCall<'_>,
) {
    let FileToolCall {
        policy,
        tool,
        context,
        audit_context,
        cwd,
        allowed,
        rule,
        refusal,
        marker,
        ticket,
    } = *call;
    let capability = tool.capability;
    let explanation = policy.explain("main", capability, context, cwd);
    policy.audit(
        AuditDecision::new(
            "main",
            capability,
            audit_context,
            allowed,
            "policy",
            rule,
            refusal,
        )
        .with_explanation(explanation.as_ref())
        .with_site(CallSite::new(Some(ticket), EntryPath::FileTool)),
    );
    if let Some(recorder) = policy.recorder() {
        recorder.exit_host_call(marker, allowed);
    }
    recording.finish(FinishedRun {
        dispatched: true,
        error: refusal.map(|reason| ExecuteError {
            kind: ErrorKind::PermissionDenied,
            message: FileToolError::denied(capability, reason).message,
            diagnostics: Vec::new(),
            denial: Some(DenialDetails {
                caller: "main".into(),
                capability: capability.into(),
                source: "policy",
            }),
        }),
        result: None,
        console: String::new(),
        usage: interpreter::runtime::limits::ExecutionUsage::default(),
        log: log.finish(),
        mcp_catalog: None,
        wall: recording.started().elapsed(),
    });
}

/// A file tool call that failed for this call alone: a denial, a missing file, a
/// path outside the VFS. It is answered as an `isError` result the model reads,
/// carrying a `{ kind, message }` error like the one `execute` reports, because
/// clients such as `langchain-mcp-adapters` raise on a JSON-RPC error and end the
/// agent run. JSON-RPC errors stay for protocol problems.
#[derive(Serialize)]
struct FileToolError {
    kind: FileToolErrorKind,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum FileToolErrorKind {
    PermissionDenied,
    FileError,
}

impl FileToolError {
    fn denied(capability: &str, reason: &str) -> Self {
        Self {
            kind: FileToolErrorKind::PermissionDenied,
            message: format!("permission denied: caller=main capability={capability}: {reason}"),
        }
    }

    fn file(path: &str, error: ContainError) -> Self {
        Self {
            kind: FileToolErrorKind::FileError,
            message: format!("{path}: {error}"),
        }
    }
}

/// The `{ error }` result a failed file tool call answers with.
#[derive(Serialize)]
struct FileToolErrorOutput {
    error: FileToolError,
}

fn file_tool_failure(error: FileToolError) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured_error(tool_value(
        FileToolErrorOutput { error },
    )?))
}

/// Walk `base` (optionally recursing) through the contained handle, naming each
/// entry by the guest path the walk arrived at. Stops at [`LIST_MAX_ENTRIES`],
/// returning `truncated = true`.
///
/// Entry metadata does not follow symlinks, so a link to a directory is reported
/// as an entry and never descended into. A mount point below `base` is descended
/// into through its volume's handle, not the empty directory the root holds there.
fn list_entries(
    base: &ContentPath,
    base_guest: &str,
    recursive: bool,
) -> Result<(Vec<FileEntry>, bool), ContainError> {
    let mut entries = Vec::new();
    // A subdirectory still to drain, named by its parent's handle rather than by
    // its own: the descriptors this holds open are bounded by the tree's depth.
    let mut pending = Vec::new();
    let mounts = if recursive {
        base.mounts_below()
    } else {
        Vec::new()
    };
    let mut dir = Arc::new(base.open_dir()?);
    let mut prefix = base_guest.to_string();
    let mut rel = std::path::PathBuf::new();
    loop {
        for item in dir.entries()? {
            if entries.len() >= LIST_MAX_ENTRIES {
                return Ok((entries, true));
            }
            let item = item?;
            let meta = item.metadata()?;
            let file_type = meta.file_type();
            let is_dir = file_type.is_dir();
            let name = item.file_name();
            let path = format!("{prefix}/{}", name.to_string_lossy());
            if recursive && is_dir {
                let child = rel.join(&name);
                pending.push((Arc::clone(&dir), name, path.clone(), child));
            }
            entries.push(FileEntry {
                path,
                kind: kind_of(&file_type),
                // Only a regular file has a size of its own: a symlink's `len()` is
                // the byte length of its target.
                bytes: if file_type.is_file() { meta.len() } else { 0 },
            });
        }
        let Some((parent, name, next, child)) = pending.pop() else {
            return Ok((entries, false));
        };
        dir = match mounts.iter().find(|(mount, _)| *mount == child) {
            Some((_, volume)) => Arc::new(volume.try_clone()?),
            None => Arc::new(parent.open_dir(&name)?),
        };
        prefix = next;
        rel = child;
    }
}

/// The guest path a listing is rooted at, normalized the way the resolver
/// normalizes it. Empty for the VFS root, so appending `/{name}` yields a
/// `/`-rooted VFS path at every depth.
fn guest_dir_prefix(dir: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    for part in dir.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                names.pop();
            }
            name => names.push(name),
        }
    }
    names.iter().fold(String::new(), |mut path, name| {
        path.push('/');
        path.push_str(name);
        path
    })
}

/// Read a `[offset, offset+limit)` line window from `resolved`, streaming so a
/// huge file costs only the window in memory. Lines are split on `\n` (a trailing
/// `\r` is stripped) and decoded lossily; lines past `READ_MAX_LINE_CHARS` are
/// truncated and counted.
fn read_window(
    vfs_path: &str,
    resolved: &ContentPath,
    offset: u32,
    limit: u32,
) -> Result<ReadFileOutput, ContainError> {
    use std::io::{BufRead, BufReader};

    let bytes = resolved.metadata()?.len();
    let mut reader = BufReader::new(resolved.open()?.into_std());

    let mut content = String::new();
    let mut buf = Vec::new();
    let mut current: u32 = 0;
    let mut returned: u32 = 0;
    let mut truncated_lines: u32 = 0;
    let mut has_more = false;

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break; // EOF
        }
        current += 1;
        if current < offset {
            continue;
        }
        if returned >= limit {
            has_more = true; // this line exists but spills past the window
            break;
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        let line = String::from_utf8_lossy(&buf);
        if returned > 0 {
            content.push('\n');
        }
        if line.chars().count() > READ_MAX_LINE_CHARS {
            content.extend(line.chars().take(READ_MAX_LINE_CHARS));
            content.push_str(" …[line truncated]");
            truncated_lines += 1;
        } else {
            content.push_str(&line);
        }
        returned += 1;
    }

    let line_end = if returned > 0 {
        offset + returned - 1
    } else {
        0
    };
    Ok(ReadFileOutput {
        path: vfs_path.to_string(),
        content,
        line_start: if returned > 0 { offset } else { 0 },
        line_end,
        returned_lines: returned,
        has_more,
        next_offset: has_more.then_some(line_end + 1),
        truncated_lines,
        bytes,
    })
}

// Hand-written rather than `#[tool_handler]` so `list_tools` can serve a
// per-blueprint description (the canonical prompt with `{vfs_mode}` resolved).
impl ServerHandler for SubmilliMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Submilli: compile and run TypeScript-subset programs in a sandbox.")
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        mut context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tool_call_id = tool_call_id(&context);
        attach_run_context(&mut context, tool_call_id.clone());
        let Some(hub) = self.shared.events.clone() else {
            return self.dispatch_tool(request, context).await;
        };
        let started = std::time::Instant::now();
        let tool = request.name.to_string();
        let session = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(session_header);
        let result = self.dispatch_tool(request, context).await;
        let (ok, bytes) = match &result {
            Ok(result) => (
                !result.is_error.unwrap_or(false),
                serde_json::to_vec(result).map_or(0, |bytes| bytes.len() as u64),
            ),
            Err(error) => (
                false,
                serde_json::to_vec(error).map_or(0, |bytes| bytes.len() as u64),
            ),
        };
        super::super::record::events::tool_call(
            &hub,
            session.as_deref(),
            tool_call_id.as_deref(),
            &tool,
            ok,
            bytes,
            started.elapsed(),
        );
        result
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        use submilli_shared::prompt::tools as shared;
        let mut tools = self.tool_router.list_all();
        let applied = self.require_blueprint().await?;
        let mut blueprint = (*applied.blueprint).clone();
        let session_id = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(session_header);
        let variables = self.variables(session_id.as_deref()).await?;
        blueprint.vfs = blueprint
            .vfs
            .resolve(&variables)
            .map_err(|error| ErrorData::invalid_request(error.to_string(), None))?;
        let execute = submilli_shared::prompt::execute_tool_description(
            &blueprint,
            submilli_shared::prompt::PromptSurface::Mcp,
        );
        for tool in &mut tools {
            let description = match tool.name.as_ref() {
                TOOL_NAME => Some(execute.clone()),
                "submilli__typescript__packages__search" => {
                    Some(shared::PACKAGES_SEARCH.to_string())
                }
                "submilli__typescript__packages__docs" => Some(shared::PACKAGES_DOCS.to_string()),
                "submilli__typescript__builtins__list" => Some(shared::BUILTINS_LIST.to_string()),
                "submilli__typescript__builtins__docs" => Some(shared::BUILTINS_DOCS.to_string()),
                "submilli__typescript__last_run" => Some(shared::LAST_RUN.to_string()),
                _ => None,
            };
            if let Some(description) = description {
                tool.description = Some(Cow::Owned(description));
            }
        }
        Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}

impl SubmilliMcp {
    /// Runs a tool call: argument normalization, then the tool itself.
    async fn dispatch_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let is_execute = request.name.as_ref() == TOOL_NAME;
        let tool = self
            .tool_router
            .get(&request.name)
            .ok_or_else(|| ErrorData::invalid_params("tool not found", None))?;
        if let Some(arguments) = request.arguments.as_mut() {
            normalize_tool_arguments(arguments, &tool.input_schema);
        }
        let tcc = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tcc).await.or_else(tool_call_failure);
        if !is_execute {
            return result;
        }
        // An execute call refused before its run carries an id of its own.
        with_execution_id(result, &uuid::Uuid::new_v4().to_string())
    }
}

/// The longest `tool_call_id` kept. A longer one is dropped rather than
/// truncated, which could make two ids collide.
const MAX_TOOL_CALL_ID_BYTES: usize = 256;

/// The client's id for this tool call, when it names a usable one: Claude Code sends
/// `claudecode/toolUseId` in the request's `_meta`.
fn tool_call_id(context: &RequestContext<RoleServer>) -> Option<String> {
    context
        .meta
        .0
        .get("claudecode/toolUseId")
        .and_then(serde_json::Value::as_str)
        .and_then(usable_tool_call_id)
}

pub(super) fn usable_tool_call_id(id: &str) -> Option<String> {
    (id.len() <= MAX_TOOL_CALL_ID_BYTES).then(|| id.to_owned())
}

/// Puts what the run's record needs of the call where the tools find it: the client's
/// name from its `initialize` and the id of this tool call.
fn attach_run_context(context: &mut RequestContext<RoleServer>, tool_call_id: Option<String>) {
    let client = context
        .peer
        .peer_info()
        .map(|info| McpClient(info.client_info.name.clone()));
    let Some(parts) = context.extensions.get_mut::<axum::http::request::Parts>() else {
        return;
    };
    if let Some(client) = client {
        parts.extensions.insert(client);
    }
    if let Some(id) = tool_call_id {
        parts.extensions.insert(McpToolCall(id));
    }
}

/// Puts `id` in the result as `execution_id`, unless it already carries one.
fn with_execution_id(
    result: Result<CallToolResult, ErrorData>,
    id: &str,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(mut result) => {
            if let Some(mut value) = result.structured_content.take() {
                if let Some(fields) = value.as_object_mut() {
                    fields
                        .entry("execution_id")
                        .or_insert_with(|| serde_json::json!(id));
                }
                return Ok(if result.is_error.unwrap_or(false) {
                    CallToolResult::structured_error(value)
                } else {
                    CallToolResult::structured(value)
                });
            }
            Ok(result)
        }
        Err(mut error) => {
            let mut data = error
                .data
                .take()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            data.entry("execution_id")
                .or_insert_with(|| serde_json::json!(id));
            error.data = Some(serde_json::Value::Object(data));
            Err(error)
        }
    }
}

pub(super) fn tool_call_failure(error: ErrorData) -> Result<CallToolResult, ErrorData> {
    // rmcp also uses INVALID_PARAMS for missing internal extensions.
    // Only parameter deserialization failures are correctable by the model.
    if error.code != ErrorCode::INVALID_PARAMS
        || !error
            .message
            .starts_with("failed to deserialize parameters:")
    {
        return Err(error);
    }
    Ok(CallToolResult::structured_error(serde_json::json!({
        "error": {
            "kind": "invalid_arguments",
            "message": error.message,
        }
    })))
}

// Only coerce declared scalar parameters; strings such as source code and paths
// must retain their original values. Serde still validates ranges and required fields.
fn normalize_tool_arguments(
    arguments: &mut serde_json::Map<String, serde_json::Value>,
    schema: &serde_json::Map<String, serde_json::Value>,
) {
    let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    for (name, value) in arguments {
        let Some(text) = value.as_str() else {
            continue;
        };
        let Some(property) = properties.get(name) else {
            continue;
        };
        let replacement = match scalar_parameter_type(property) {
            Some("boolean") => text.parse::<bool>().ok().map(serde_json::Value::Bool),
            Some("integer" | "number") => text
                .parse::<serde_json::Number>()
                .ok()
                .map(serde_json::Value::Number),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *value = replacement;
        }
    }
}

fn scalar_parameter_type(schema: &serde_json::Value) -> Option<&str> {
    let kind = schema.get("type")?;
    if let Some(kind) = kind.as_str() {
        return Some(kind);
    }
    // Optional parameters use a type array containing the scalar and null.
    // Ambiguous unions must retain the supplied value.
    let mut kinds = kind
        .as_array()?
        .iter()
        .filter(|kind| kind.as_str() != Some("null"));
    let scalar = kinds.next()?.as_str()?;
    if kinds.next().is_some() {
        return None;
    }
    Some(scalar)
}
