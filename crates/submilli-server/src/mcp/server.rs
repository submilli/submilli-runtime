//! The `submilli__typescript__execute` MCP tool. Reuses the same compile/run
//! path and `RunOutcome` → `{ result, console, error }` mapping as the REST
//! `/v1/execute` handler; the blueprint is fixed per service instance (one per
//! `/mcp/{blueprint}` endpoint), so the tool takes only `code`.

use std::borrow::Cow;
use std::sync::Arc;

use interpreter::runtime::fs::{ContainError, ContentPath, guest_normalize, resolve_content};
use interpreter::runtime::security::AuditDecision;
use interpreter::runtime::{CallSite, CheckOutcome, EntryPath, SecurityCheck, Vfs, VfsInfo};
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
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VfsConfig, required_harness_secrets};
use submilli_shared::PolicyCheck;
use submilli_shared::library_visibility::LibraryVisibility;

use crate::app::AppState;
use crate::error::ExecuteError;
use crate::handlers::execute::{ExecuteInputs, VfsSource, blueprint_miss_message, execute_core};
use crate::packages;
use crate::session_manager::{attach_limits, build_vfs, vfs_info};

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
/// code — `"file"`, `"directory"`, `"symlink"`, `"other"` — so the two surfaces
/// describe the same entry the same way. `bytes` is the size of a regular file and
/// 0 for everything else.
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

/// The `{ result, console, error }` shape shared by `execute` and `lastRun`.
/// `LastRun` carries the same fields (full, un-suppressed console).
#[derive(Serialize)]
struct ExecuteOutput {
    result: Option<String>,
    console: Vec<String>,
    error: Option<ExecuteError>,
}

/// The client's name from its `initialize`, for the run's record.
#[derive(Clone)]
struct McpClient(String);

/// The client's id for the tool call, for the run's record and events.
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
    state: AppState,
    blueprint_name: String,
    /// Blueprint snapshot captured at build time. `list_tools` re-fetches the
    /// current blueprint to resolve the description's policy-dependent placeholders
    /// (`{vfs_mode}`, `{http_access}`); this is only the fallback for when the
    /// blueprint was removed mid-session. The execute path also re-fetches per call.
    blueprint: Blueprint,
}

impl SubmilliMcp {
    pub(crate) fn new(state: AppState, blueprint_name: String, blueprint: Blueprint) -> Self {
        Self {
            tool_router: Self::tool_router(),
            state,
            blueprint_name,
            blueprint,
        }
    }

    /// The current blueprint for a tool that cannot run without one. A name whose
    /// stored revision this binary can no longer parse stays reserved but has no
    /// runnable blueprint, so the refusal names that reason instead of telling the
    /// agent the endpoint it is connected to addresses nothing.
    async fn require_blueprint(&self) -> Result<Blueprint, ErrorData> {
        match self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(blueprint_store_error)?
        {
            Some(blueprint) => Ok(blueprint),
            None => Err(ErrorData::invalid_request(
                blueprint_miss_message(&self.state, &self.blueprint_name)
                    .await
                    .map_err(blueprint_store_error)?,
                None,
            )),
        }
    }

    /// The current blueprint, re-fetched from the store so a mid-session update is
    /// reflected; falls back to the build-time snapshot if it was removed.
    async fn current_blueprint(&self) -> Result<Blueprint, ErrorData> {
        Ok(self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(blueprint_store_error)?
            .unwrap_or_else(|| self.blueprint.clone()))
    }

    /// Open the VFS a tool operates on. `per_session` resolves the session's
    /// durable directory (and keeps it alive); every other mode gets a standalone
    /// VFS per call. A cross-call workflow — write in `execute`, read with
    /// `files.read` — therefore needs a directory that is the same one each
    /// time: a `per_session` root, a `named` root, or a path under a mount, which
    /// remount the same declared volume. Only an `ephemeral` root is a fresh
    /// temp directory per call. Shared by `execute`, `files.read` and
    /// `files.list`; only a program run enforces the size limits, which costs a
    /// walk of each limited directory.
    async fn acquire_vfs(
        &self,
        blueprint: &Blueprint,
        session_id: Option<&str>,
        purpose: VfsUse,
        variables: &submilli_blueprint::VarBindings,
    ) -> Result<(Vfs, VfsInfo), ErrorData> {
        let manager = self.state.session_manager();
        if matches!(blueprint.vfs, VfsConfig::PerSession { .. }) {
            let sid = session_id
                .ok_or_else(|| ErrorData::invalid_request("missing mcp-session-id", None))?;
            manager
                .ensure(sid, blueprint)
                .await
                .map_err(vfs_session_error)?;
            let pair = if purpose == VfsUse::RunProgram {
                manager
                    .vfs_for_execute_with_variables(sid, blueprint, variables)
                    .await
            } else {
                manager.session_vfs_with_variables(sid, blueprint, variables)
            }
            .map_err(vfs_session_error)?;
            manager.touch(sid).await;
            Ok(pair)
        } else {
            let vfs = build_vfs(
                blueprint,
                variables,
                None,
                manager.ephemeral_root(),
                manager.volume_registry(),
            )
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let vfs = if purpose == VfsUse::RunProgram {
                attach_limits(vfs, blueprint, manager.volume_registry())
                    .await
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            } else {
                vfs
            };
            Ok((vfs, vfs_info(blueprint)))
        }
    }
}

/// What a tool needs its VFS for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VfsUse {
    /// Run a program, which may write: the size limit is enforced.
    RunProgram,
    /// Read or list files: no limit to enforce.
    ReadFiles,
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
        let audit = parts
            .extensions
            .get::<Arc<crate::audit::ExecutionAudit>>()
            .cloned()
            .ok_or_else(|| ErrorData::internal_error("missing execution audit context", None))?;
        self.execute_inner(args, parts, audit).await
    }

    async fn execute_inner(
        &self,
        args: ExecuteArgs,
        parts: axum::http::request::Parts,
        audit: Arc<crate::audit::ExecutionAudit>,
    ) -> Result<CallToolResult, ErrorData> {
        let blueprint = Arc::new(self.require_blueprint().await.inspect_err(|error| {
            audit.error(if error.code == ErrorCode::INTERNAL_ERROR {
                crate::error::ErrorKind::RuntimeError
            } else {
                crate::error::ErrorKind::BlueprintNotFound
            });
        })?);

        // Stateful transport: every connection has a session id (rmcp rejects a
        // non-initialize request without one before we get here).
        let session_id = session_header(&parts);
        // Variables were bound and validated at `initialize`; read this session's
        // resolved bindings (empty if none) for the policy filter.
        let variables = self
            .state
            .session_manager()
            .variables(session_id.as_deref().unwrap_or(""));

        audit.annotate(
            &args.code,
            &self.blueprint_name,
            Some(&blueprint),
            &variables,
        );
        audit.begin();
        let harness_secrets = match self
            .state
            .session_manager()
            .harness_secrets(session_id.as_deref().unwrap_or(""))
        {
            Some(secrets) => secrets,
            None if required_harness_secrets(&blueprint.secrets).is_empty() => {
                Arc::new(HarnessSecretBindings::new())
            }
            None => {
                audit.error(crate::error::ErrorKind::InvalidRequest);
                return Err(ErrorData::invalid_request(
                    "session_requires_secrets",
                    Some(serde_json::json!({
                        "required": required_harness_secrets(&blueprint.secrets),
                    })),
                ));
            }
        };

        // The file area opens under this tool's rules (a `per_session` root needs
        // the session id), ahead of the shared core, which does the rest.
        let (vfs, vfs_info) = self
            .acquire_vfs(
                &blueprint,
                session_id.as_deref(),
                VfsUse::RunProgram,
                &variables,
            )
            .await
            .inspect_err(|_| {
                audit.error(crate::error::ErrorKind::RuntimeError);
            })?;
        let outcome = execute_core(
            &self.state,
            ExecuteInputs {
                session_id: session_id.as_deref().unwrap_or(""),
                code: &args.code,
                blueprint_name: &self.blueprint_name,
                blueprint,
                variables,
                harness_secrets,
                audit: Some(audit),
                vfs_source: VfsSource::Mcp { vfs, vfs_info },
                run_entry: crate::record::RunEntry::Mcp,
                client: parts
                    .extensions
                    .get::<McpClient>()
                    .map(|client| client.0.clone()),
                tool_call_id: parts
                    .extensions
                    .get::<McpToolCall>()
                    .map(|call| call.0.clone()),
                idempotency_key: None,
            },
        )
        .await;
        let response = outcome.response;
        if !outcome.dispatched {
            // Nothing ran: a protocol-level failure, as it always was for MCP.
            let message = response
                .error
                .map(|error| error.message)
                .unwrap_or_default();
            return Err(ErrorData::internal_error(message, None));
        }
        let (result, console, error) = (response.result, response.console, response.error);

        structured_result(ExecuteOutput {
            result,
            console,
            error,
        })
    }

    // Description is served from `submilli_shared::prompt::tools` via `list_tools`,
    // like the discovery tools — the HTTP client harness publishes the same one.
    #[tool(name = "submilli__typescript__last_run")]
    async fn last_run(
        &self,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = session_header(&parts)
            .ok_or_else(|| ErrorData::invalid_request("no session for lastRun", None))?;
        match self.state.sessions().get(&session_id).await {
            Ok(Some(run)) => structured_result(ExecuteOutput {
                result: run.result,
                console: run.console,
                error: run.error,
            }),
            Ok(None) => Err(ErrorData::invalid_request(
                "no previous run in this session",
                None,
            )),
            Err(error) => {
                tracing::warn!(operation = "get", session = %session_id, %error, "last-run storage failed");
                Err(ErrorData::internal_error(
                    "last-run storage unavailable",
                    None,
                ))
            }
        }
    }

    // Description is served from `submilli_shared::prompt::tools` via
    // `list_tools` — the same table the REST prompt endpoint serves, so the
    // MCP and HTTP client surfaces can't teach different things.
    #[tool(name = "submilli__typescript__packages__docs")]
    async fn packages_docs(
        &self,
        Parameters(args): Parameters<PackageDocsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let lookup = match self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(blueprint_store_error)?
        {
            Some(blueprint) => {
                let catalog = self
                    .state
                    .mcp_catalog(&self.blueprint_name, &blueprint)
                    .await
                    .map_err(mcp_discovery_error)?;
                packages::lookup_with_blueprint(
                    &args.name,
                    &catalog,
                    &blueprint,
                    self.state.package_store(),
                )
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

    // Description is served from `submilli_shared::prompt::tools` via
    // `list_tools` — the same table the REST prompt endpoint serves, so the
    // MCP and HTTP client surfaces can't teach different things.
    #[tool(name = "submilli__typescript__packages__search")]
    async fn packages_search(
        &self,
        Parameters(args): Parameters<PackageSearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // Fold this blueprint's `@mcp/<server>` packages in alongside the stdlib
        // hits, so search can discover MCP tooling — not just `packages.docs` once
        // the server name is already known.
        let value = match self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(blueprint_store_error)?
        {
            Some(blueprint) => {
                let catalog = self
                    .state
                    .mcp_catalog(&self.blueprint_name, &blueprint)
                    .await
                    .map_err(mcp_discovery_error)?;
                packages::search_json_with_blueprint(
                    &args.query,
                    &catalog,
                    &blueprint,
                    self.state.package_store(),
                    packages::Fetch::Mcp,
                )
            }
            None => packages::search_json(&args.query, packages::Fetch::Mcp),
        };
        Ok(CallToolResult::structured(value))
    }

    // Description is served from `submilli_shared::prompt::tools` via
    // `list_tools` — the same table the REST prompt endpoint serves, so the
    // MCP and HTTP client surfaces can't teach different things.
    #[tool(name = "submilli__typescript__builtins__docs")]
    async fn builtins_docs(
        &self,
        Parameters(args): Parameters<BuiltinsDocsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // The bound blueprint's `@mcp/*` names, so a package name asked of this
        // tool gets the correcting call rather than a bare unknown.
        let (mcp_packages, visibility) = match self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(blueprint_store_error)?
        {
            Some(blueprint) => {
                let catalog = self
                    .state
                    .mcp_catalog(&self.blueprint_name, &blueprint)
                    .await
                    .map_err(mcp_discovery_error)?;
                (
                    packages::mcp_package_names(&catalog),
                    LibraryVisibility::for_blueprint(&blueprint),
                )
            }
            None => (Vec::new(), LibraryVisibility::unscoped()),
        };
        Ok(CallToolResult::structured(packages::builtins_docs_json(
            &args.names,
            &mcp_packages,
            &packages::Fetch::Mcp,
            visibility,
        )))
    }

    // Description is served from `submilli_shared::prompt::tools` via
    // `list_tools` — the same table the REST prompt endpoint serves, so the
    // MCP and HTTP client surfaces can't teach different things.
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
        let blueprint = Arc::new(self.require_blueprint().await?);

        let session_id = session_header(&parts);
        let variables = self
            .state
            .session_manager()
            .variables(session_id.as_deref().unwrap_or(""));
        if let Err(refused) = self.authorize_file_call(
            &parts,
            Arc::clone(&blueprint),
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
        let (vfs, _info) = self
            .acquire_vfs(
                &blueprint,
                session_id.as_deref(),
                VfsUse::ReadFiles,
                &variables,
            )
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
        let blueprint = Arc::new(self.require_blueprint().await?);

        let session_id = session_header(&parts);
        let variables = self
            .state
            .session_manager()
            .variables(session_id.as_deref().unwrap_or(""));
        let dir = args.path.as_deref().unwrap_or(".");
        let recursive = args.recursive.unwrap_or(false);
        if let Err(refused) = self.authorize_file_call(
            &parts,
            Arc::clone(&blueprint),
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
        let (vfs, _info) = self
            .acquire_vfs(
                &blueprint,
                session_id.as_deref(),
                VfsUse::ReadFiles,
                &variables,
            )
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
    /// File tools act as the session's main program, including on isolated VFSs.
    ///
    /// The path is checked lexically before the policy sees it. The policy refuses
    /// a path that cannot name anything in the VFS (`..` past the root, a NUL byte)
    /// too, but as a denial, which tells the model a rule stopped it rather than
    /// that the path is wrong.
    fn authorize_file_call(
        &self,
        parts: &axum::http::request::Parts,
        blueprint: Arc<Blueprint>,
        variables: Arc<submilli_blueprint::VarBindings>,
        tool: FileTool<'_>,
        path: &str,
        recursive: Option<bool>,
    ) -> Result<(), FileToolError> {
        let capability = tool.capability;
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
        let recording = self.file_tool_recording(parts, &blueprint, &variables, tool.name);
        let log = recording.as_ref().map(crate::record::Recording::log);
        let policy: Arc<dyn SecurityCheck> =
            Arc::new(PolicyCheck::with_variables(blueprint.clone(), variables));
        let policy = log.as_ref().map_or(policy.clone(), |log| log.wrap(policy));
        let call = policy.recorder().map(|recorder| {
            let marker = recorder.enter_host_call();
            (marker, recorder.begin_call("main", capability, None))
        });
        let outcome = policy.check_with_cwd("main", capability, &context, config.cwd());
        let audit_context = policy.audit_context(capability, &context, config.cwd());
        self.state.audit().file_decision(
            parts,
            &blueprint,
            capability,
            audit_context.as_ref(),
            &outcome,
        );
        let (allowed, rule, refusal) = match outcome {
            CheckOutcome::Allow { rule } => (true, rule, None),
            CheckOutcome::Deny { reason, rule } => (false, rule, Some(reason)),
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

    /// A file tool's decision is a run of its own, when runs are recorded. Nothing is
    /// built for a server without a run recorder.
    fn file_tool_recording(
        &self,
        parts: &axum::http::request::Parts,
        blueprint: &Arc<Blueprint>,
        variables: &Arc<submilli_blueprint::VarBindings>,
        tool: &str,
    ) -> Option<crate::record::Recording> {
        crate::record::Recording::start(&self.state, || crate::record::RunStart {
            execution_id: uuid::Uuid::new_v4().to_string(),
            label: parts
                .extensions
                .get::<crate::audit::Principal>()
                .map_or_else(|| "unauthenticated".to_owned(), |p| p.0.clone()),
            entry: crate::record::RunEntry::McpFileTool { tool: tool.into() },
            test_of: None,
            client: parts.extensions.get::<McpClient>().map(|c| c.0.clone()),
            tool_call_id: parts.extensions.get::<McpToolCall>().map(|c| c.0.clone()),
            session_id: session_header(parts),
            idempotency_key: None,
            blueprint_name: self.blueprint_name.clone(),
            blueprint: Arc::clone(blueprint),
            blueprint_hash: crate::audit::blueprint_hash(blueprint),
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
    recording: &crate::record::Recording,
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
    recording.finish(crate::record::FinishedRun {
        dispatched: true,
        error: refusal.map(|reason| ExecuteError {
            kind: crate::error::ErrorKind::PermissionDenied,
            message: FileToolError::denied(capability, reason).message,
            diagnostics: Vec::new(),
            denial: Some(crate::error::DenialDetails {
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
        wall: recording.started.elapsed(),
    });
}

/// A file tool call that failed for this call alone: a denial, a missing file, a
/// path outside the VFS. It is answered as an `isError` result the model reads,
/// carrying a `{ kind, message }` error like the one `execute` reports, because
/// clients such as `langchain-mcp-adapters` raise on a JSON-RPC error and end the
/// agent run. JSON-RPC errors stay for protocol problems: no session, unknown tool,
/// malformed request envelopes.
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
/// as an entry and never descended into — which is also what keeps the walk from
/// leaving the root once `base` itself has been opened under containment. A mount
/// point below `base` is descended into through its volume's handle, not the
/// empty directory the root holds there.
fn list_entries(
    base: &ContentPath,
    base_guest: &str,
    recursive: bool,
) -> Result<(Vec<FileEntry>, bool), ContainError> {
    let mut entries = Vec::new();
    // A subdirectory still to drain, named by its parent's handle rather than by
    // its own: siblings share one parent, so the descriptors this holds open are
    // bounded by the tree's depth instead of its width.
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
                // the byte length of its target, which would tell the caller how long
                // a path it is not allowed to see is.
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
///
/// Escapes are already refused by the resolver, so a `..` with nothing to pop
/// cannot reach here; it saturates at the root rather than being reported twice.
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
/// `\r` is stripped) and decoded lossily, so binary-ish content reads without
/// erroring; lines past `READ_MAX_LINE_CHARS` are truncated and counted.
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
// Argument normalization and tool failures are handled before returning to clients.
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
        let Some(hub) = self.state.event_hub().cloned() else {
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
        crate::record::events::tool_call(
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
        let mut blueprint = self.current_blueprint().await?;
        let session_id = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(session_header);
        let variables = self
            .state
            .session_manager()
            .variables(session_id.as_deref().unwrap_or(""));
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
    /// Runs a tool call: the audit, argument normalization, and the tool itself.
    async fn dispatch_tool(
        &self,
        mut request: CallToolRequestParams,
        mut context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let audit = if request.name.as_ref() == TOOL_NAME {
            let parts = context.extensions.get::<axum::http::request::Parts>();
            let principal = parts
                .and_then(|p| p.extensions.get::<crate::audit::Principal>())
                .map_or("unauthenticated", |p| p.0.as_str());
            let session = parts.and_then(session_header);
            let audit = crate::audit::ExecutionAudit::new(
                self.state.audit().clone(),
                principal,
                "mcp",
                session.as_deref(),
            );
            let code = request
                .arguments
                .as_ref()
                .and_then(|a| a.get("code"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let variables = self
                .state
                .session_manager()
                .variables(session.as_deref().unwrap_or(""));
            audit.annotate(code, &self.blueprint_name, None, &variables);
            if let Some(parts) = context.extensions.get_mut::<axum::http::request::Parts>() {
                parts.extensions.insert(audit.clone());
            }
            Some(audit)
        } else {
            None
        };
        let tool = self
            .tool_router
            .get(&request.name)
            .ok_or_else(|| ErrorData::invalid_params("tool not found", None))?;
        if let Some(arguments) = request.arguments.as_mut() {
            normalize_tool_arguments(arguments, &tool.input_schema);
        }
        let tcc = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tcc).await;
        if let (Some(audit), Err(error)) = (&audit, &result)
            && error.code == rmcp::model::ErrorCode::INVALID_PARAMS
        {
            audit.error(crate::error::ErrorKind::InvalidRequest);
        }
        let result = result.or_else(tool_call_failure);
        let Some(audit) = audit else {
            return result;
        };
        let success = result.as_ref().is_ok_and(|r| !r.is_error.unwrap_or(false));
        audit.finish(success);
        with_execution_id(result, &audit.id)
    }
}

/// The longest `tool_call_id` kept. A client's id is an opaque handle of a few dozen
/// bytes; a longer one is not an id worth carrying through every record and event, so it
/// is dropped rather than truncated, which could make two ids collide.
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

fn usable_tool_call_id(id: &str) -> Option<String> {
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

fn mcp_discovery_error(error: submilli_shared::mcp::DiscoveryError) -> ErrorData {
    tracing::error!(error = ?error, "MCP discovery initialization failed");
    ErrorData::internal_error(error.to_string(), None)
}

fn blueprint_store_error(error: crate::blueprint::StoreError) -> ErrorData {
    ErrorData::internal_error(crate::blueprint::store_failure_message(error), None)
}

fn vfs_session_error(error: crate::session_manager::SessionError) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

fn with_execution_id(
    result: Result<CallToolResult, ErrorData>,
    id: &str,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(mut result) => {
            if let Some(mut value) = result.structured_content.take() {
                if let Some(fields) = value.as_object_mut() {
                    fields.insert("execution_id".into(), serde_json::json!(id));
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
            data.insert("execution_id".into(), serde_json::json!(id));
            error.data = Some(serde_json::Value::Object(data));
            Err(error)
        }
    }
}

fn tool_call_failure(error: ErrorData) -> Result<CallToolResult, ErrorData> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_failure_preserves_protocol_and_internal_errors() {
        for error in [
            ErrorData::invalid_params("Missing extension: HTTP request parts", None),
            ErrorData::invalid_params("tool not found", None),
            ErrorData::invalid_request("missing mcp-session-id", None),
            ErrorData::internal_error("failed to deserialize parameters: internal failure", None),
        ] {
            assert_eq!(tool_call_failure(error.clone()).unwrap_err(), error);
        }
    }

    #[test]
    fn an_overlong_tool_call_id_is_dropped() {
        let at_cap = "a".repeat(MAX_TOOL_CALL_ID_BYTES);
        assert_eq!(
            usable_tool_call_id(&at_cap).as_deref(),
            Some(at_cap.as_str())
        );
        assert_eq!(
            usable_tool_call_id(&"a".repeat(MAX_TOOL_CALL_ID_BYTES + 1)),
            None
        );
        assert_eq!(usable_tool_call_id("toolu_01").as_deref(), Some("toolu_01"));
    }
}
