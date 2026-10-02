//! Renders the LLM-facing `execute` tool description — the "system prompt" the
//! MCP server hands its caller. The text lives in `llm-prompt.md`; this
//! crate extracts the prompt body and resolves its placeholders (`{sandbox}`,
//! `{http_access}`, `{builtins}`) against the compiler and a blueprint's policy.
//!
//! Shared so the server (which serves it) and the CLI (which prints it for
//! debugging) can't drift apart.

use crate::library_visibility::LibraryVisibility;
use interpreter::stdlib::capabilities;
use submilli_blueprint::{Action, Blueprint, DefaultAction, FieldMatch, FilterExpr};

/// The canonical LLM-facing prompt, embedded at build time. We serve its
/// `## The prompt` section as the `execute` tool description.
const LLM_PROMPT_DOC: &str = include_str!("../../../llm-prompt.md");

/// The caller id of the user's script — the one the prompt describes.
const MAIN_CALLER: &str = "main";

/// The stdlib module whose capabilities gate outbound HTTP.
const HTTP_MODULE: &str = "submilli:http";

/// Descriptions for the discovery tools every LLM-facing surface exposes —
/// the MCP tools reference these directly in their `#[tool]` attributes, and
/// the REST prompt endpoint serves them so the HTTP harness teaches
/// its model the same thing. Keep them here, not inline, so the two surfaces
/// can't drift.
pub mod tools {
    pub const PACKAGES_SEARCH: &str = "Search available packages by name, description, or exported \
        symbol name — stdlib modules (e.g. \"submilli:crypto\"), this blueprint's declared registry \
        packages (e.g. \"@submilli/github\"), and its discovered @mcp/<server> packages (results \
        tagged with a \"source\" of \"host\", \"registry\", or \"mcp\"). An empty query lists every \
        available package. A query matching nothing returns the available-package catalog and a \
        pointer to the built-ins tool rather than an empty list.";

    pub const PACKAGES_DOCS: &str = "Return the type declarations (a TypeScript `.d.ts`-style \
        listing of every exported function and type) plus a one-line description for a package — \
        an importable stdlib module (e.g. \"submilli:crypto\"), a registry package, or an \
        @mcp/<server> package. Read these before importing a package: the signatures are the \
        contract. A name that is a language built-in rather than a package (e.g. \"Temporal\") \
        is served here too, tagged \"source\": \"builtin\" — use it directly, with no `import`. \
        Unknown names return a structured { error }, carrying a did-you-mean candidate when a \
        close one exists.";

    pub const BUILTINS_LIST: &str = "List the language built-ins that are always in scope without \
        an `import`, returning { types, namespaces }. Use this to discover names before fetching \
        their declarations. Built-in namespaces such as Temporal and JSON must be used directly, \
        not imported.";

    pub const BUILTINS_DOCS: &str = "Return TypeScript `.d.ts`-style declarations for one or more \
        language built-ins — the globals always in scope without an `import` (e.g. \"Array\", \
        \"Map\", \"Temporal\", \"JSON\"). Their signatures are a strict subset and differ from \
        Node/TS in places, so fetch the names you intend to use. A dotted member path (e.g. \
        \"Temporal.Instant\") returns just that member — much smaller than the whole namespace, \
        so prefer it when you know the member you want. Unknown names return an { error } entry \
        rather than failing. A package name asked here returns an { error } naming the \
        package-docs call to make; use that tool for importable modules.";

    /// For harnesses exposing built-ins as one tool (name omitted ⇒ list).
    pub const BUILTINS_MERGED: &str = "Without a name, list the language built-ins always in scope \
        without an `import` ({ types, namespaces }); with one, return its TypeScript `.d.ts`-style \
        declarations. Their signatures are a strict subset and differ from Node/TS in places (no \
        `Date` — use `Temporal`), so fetch the names you intend to use before relying on a method. \
        Built-in namespaces such as Temporal and JSON are used directly, never imported. A dotted \
        member path (e.g. \"Temporal.Instant\") returns just that member rather than the whole \
        namespace. A package name asked here returns an { error } naming the package-docs call to \
        make; use that tool for importable packages.";

    pub const LAST_RUN: &str = "Return the most recent execution's result and FULL console \
        (un-suppressed) for this session. Use it to read debug output after a successful run, \
        which the execute result omits. Takes no arguments.";
}

/// Extract the prompt body — the text under the `## The prompt` heading, up to
/// the next horizontal rule. Falls back to the whole doc if the markers move.
pub fn prompt_template() -> &'static str {
    let body = LLM_PROMPT_DOC
        .split_once("\n## The prompt")
        .map_or(LLM_PROMPT_DOC, |(_, rest)| rest);
    body.split("\n---").next().unwrap_or(body).trim()
}

/// Discovery vocabulary exposed by the consumer of the prompt.
#[derive(Clone, Copy)]
pub enum PromptSurface {
    Mcp,
    Rest,
}

/// The `execute` tool description for a blueprint: the canonical prompt with
/// policy-dependent library listings and guidance resolved.
pub fn execute_tool_description(blueprint: &Blueprint, surface: PromptSurface) -> String {
    let visibility = LibraryVisibility::for_blueprint(blueprint);
    let http_visible = visibility.allows(HTTP_MODULE);
    let sandbox = if visibility.allows("submilli:fs") || visibility.allows("submilli:code") {
        format!("Sandbox: File system {}.", vfs_mode_phrase(&blueprint.vfs))
    } else {
        String::new()
    };
    let http_access = if http_visible {
        format!(
            "Network (`submilli:http`): {}.",
            http_access_phrase(blueprint)
        )
    } else {
        String::new()
    };
    let [search, docs, builtins] = surface.discovery_tools();
    prompt_template()
        .replace("{stdlib_modules}", &stdlib_modules_phrase(visibility))
        .replace("{sandbox}", &sandbox)
        .replace(
            "{http_guidance}",
            if http_visible { HTTP_GUIDANCE } else { "" },
        )
        .replace("{http_access}", &http_access)
        .replace(
            "{llm_guidance}",
            if visibility.allows("submilli:llm") {
                LLM_GUIDANCE
            } else {
                ""
            },
        )
        .replace(
            "{session_guidance}",
            if visibility.allows("submilli:session") {
                SESSION_GUIDANCE
            } else {
                ""
            },
        )
        .replace("{builtins}", &builtins_phrase())
        .replace("{mcp_packages}", &mcp_packages_phrase(blueprint))
        .replace("{t_search}", search)
        .replace("{t_docs}", docs)
        .replace("{t_builtins_docs}", builtins)
        .replace(
            "{git_package}",
            if visibility.allows("submilli:git") {
                GIT_GUIDANCE
            } else {
                ""
            },
        )
}

const GIT_GUIDANCE: &str = "\n\nGit is available as `submilli:git`. Read its package docs before use; \
    init, clone, fetch/pull, and commit require their respective Git capabilities. \
    Other local operations need no Git capability. Push is not available.";

const LLM_GUIDANCE: &str = r#"Model calls (`submilli:llm`): `call(model, prompt)` returns a
`Completion`; `call<T>` returns a checked `T`. `ok` is not "nothing
threw" — a truncated or filtered completion is `ok: false` **and still
carries `text`**, so `if (!r.ok) continue` drops usable output. Token
counts may be `null`: indeterminate, not free. `batch` is
bounded-concurrent, one result per prompt, positionally. Models are
operator-declared — `models()` lists them, and `contextWindow` /
`description` are `null` when undeclared, so filtering drops those."#;

const SESSION_GUIDANCE: &str = r#"

Session state (`submilli:session`): a key-value store scoped to this
session — `set(key, value)` writes, `get<T>(key)` reads it back checked
against `T` (throws a catchable `TypeError` on a shape mismatch),
`has`/`remove`/`list` round it out. There is no `set<T>`; the value's
type is inferred. It is **memory-only**: it does not survive a server
restart, so a key you wrote on an earlier call may legitimately be
missing. Read a key that may be absent as `get<T | null>(key)` and
handle the `null`."#;

impl PromptSurface {
    fn discovery_tools(self) -> [&'static str; 3] {
        match self {
            Self::Mcp => [
                "submilli__typescript__packages__search",
                "submilli__typescript__packages__docs",
                "submilli__typescript__builtins__docs",
            ],
            Self::Rest => ["search", "docs", "builtins"],
        }
    }
}

const HTTP_GUIDANCE: &str = r#"When making outbound HTTP, do NOT construct `Authorization` headers
or include API keys in query strings. The operator's policy
configures an `auth_proxy` that injects credentials transparently —
just hit the URL."#;

fn stdlib_modules_phrase(visibility: LibraryVisibility) -> String {
    [
        "submilli:code",
        "submilli:fs",
        "submilli:http",
        "submilli:url",
        "submilli:crypto",
        "submilli:uuid",
        "submilli:session",
        "submilli:llm",
    ]
    .into_iter()
    .filter(|name| visibility.allows(name))
    .map(|name| format!("`{name}`"))
    .collect::<Vec<_>>()
    .join(", ")
}

/// A worked example of using a *typed* MCP tool result directly — narrowing an
/// optional (`foo?`) field instead of casting. Targets the observed failure of
/// LLMs casting an already-typed result to a hand-written subset interface.
/// Lives apart from the format string so the braces and quotes stay readable.
const MCP_PACKAGES_EXAMPLE: &str = r#"  import github from "@mcp/github";
  // list_commits is already typed — no cast. `commit` is optional, so narrow
  // it against null before reading a field.
  function main(): string {
    const commits = github.list_commits({ owner: "o", repo: "r" });
    let last = "";
    for (const c of commits) {
      if (c.commit !== null) last = c.commit.message;
    }
    return last;
  }"#;

/// Resolve `{mcp_packages}`: empty when the blueprint declares no MCP servers,
/// else a short note explaining discovery of available `@mcp/<server>` packages and how to
/// consume their tools — use a typed result directly, cast only for `unknown`.
/// Injected only when relevant, so the no-MCP prompt stays terse. Starts with a
/// blank line so it slots cleanly between paragraphs.
fn mcp_packages_phrase(blueprint: &Blueprint) -> String {
    if blueprint.mcp.is_empty() {
        return String::new();
    }
    format!(
        "\n\nMCP servers are configured. Call `{{t_search}}` with `@mcp/` to find \
         currently available packages; unauthenticated or unreachable servers are omitted. \
         Call `{{t_docs}}` with an available package \
         name for its tools and signatures, then `import` it. Most tools are \
         typed — read the signature and use the result directly, narrowing \
         optional (`foo?`) fields against `null` before access; do not cast a \
         typed result to a hand-written type. Cast (`as T`) only when a tool's \
         return type is literally `unknown` (its server published no output \
         schema): make a test call to see the shape, then declare a type alias \
         or `interface` and cast. Using a typed result directly:\n\n{MCP_PACKAGES_EXAMPLE}"
    )
}

/// Resolve the `{builtins}` placeholder — the catalog of globals in scope
/// without an `import`, sourced from the prelude so it can't drift from the
/// compiler. Blueprint-independent.
fn builtins_phrase() -> String {
    let b = interpreter::packages::builtins();
    format!(
        "{}; namespaces {}",
        b.types.join(", "),
        b.namespaces.join(", ")
    )
}

/// Describe the bound blueprint's VFS configuration for the sandbox section.
fn vfs_mode_phrase(vfs: &submilli_blueprint::VfsConfig) -> String {
    use submilli_blueprint::VfsConfig;
    let size_limit_phrase = || -> String {
        vfs.size_limit()
            .map_or(String::new(), |bytes| format!(" (limit: {bytes} bytes)"))
    };
    let root = match vfs {
        VfsConfig::None => return "none — `submilli:fs` is disabled".into(),
        VfsConfig::Ephemeral { .. } => {
            format!(
                "ephemeral — a fresh sandbox, wiped after each call{}",
                size_limit_phrase()
            )
        }
        VfsConfig::PerSession { .. } => format!(
            "per_session — a sandbox that persists across calls in this session{}",
            size_limit_phrase()
        ),
        VfsConfig::Named { volume, access, .. } => format!(
            "named — volume `{volume}` ({}), whose files persist across calls and sessions and \
             are shared with other blueprints that use it",
            access_phrase(*access)
        ),
    };
    let mounts: Vec<String> = vfs
        .mounts()
        .iter()
        .map(|(path, mount)| {
            format!(
                "`{path}` → volume `{}` ({})",
                mount.volume,
                access_phrase(mount.access)
            )
        })
        .collect();
    if mounts.is_empty() {
        return root;
    }
    format!(
        "{root}; named volumes mounted at {} keep their files across sessions (see `fs.info()`)",
        mounts.join(", ")
    )
}

/// A blueprint that leaves `access` out takes whatever the server declares,
/// which the prompt cannot see.
fn access_phrase(access: Option<submilli_blueprint::Access>) -> &'static str {
    match access {
        Some(submilli_blueprint::Access::ReadOnly) => "read-only",
        Some(submilli_blueprint::Access::ReadWrite) => "read-write",
        None => "access set by the server",
    }
}

/// The host scope an HTTP operation is permitted against.
enum HostScope {
    /// Any host — an unfiltered `allow`, or one whose filter doesn't constrain
    /// `host` (or constrains it only with a `*` glob).
    Any,
    /// A specific, non-empty set of host patterns (exact, glob, or regex).
    Hosts(Vec<String>),
}

/// Resolve the `{http_access}` placeholder: a terse summary of which hosts the
/// `main` script may reach with each HTTP method, derived from the blueprint's
/// `allow` rules. The server treats `ask-human` as deny and is deny-by-default,
/// so only explicit `allow` rules grant access — and a policy-free blueprint
/// reaches nothing. Under `default: allow` the posture inverts: every method
/// reaches any host unless a `deny` rule carves it out, which is too varied to
/// summarize per-method, so we state the posture instead.
fn http_access_phrase(blueprint: &Blueprint) -> String {
    if blueprint.default_action == Some(DefaultAction::Allow) {
        return "any host, any method (allow-by-default; specific `deny` rules may \
                carve out hosts or methods)"
            .into();
    }
    let rules = blueprint.permissions.get(MAIN_CALLER);
    let mut groups: Vec<(Vec<String>, HostScope)> = Vec::new();
    for cap in http_capabilities() {
        let Some(scope) = allowed_host_scope(rules, cap) else {
            continue;
        };
        let label = method_label(cap);
        match groups.iter_mut().find(|(_, s)| same_scope(s, &scope)) {
            Some((labels, _)) => labels.push(label),
            None => groups.push((vec![label], scope)),
        }
    }
    if groups.is_empty() && LibraryVisibility::for_blueprint(blueprint).allows(HTTP_MODULE) {
        return "subject to policy; approval rules do not authorize execution automatically".into();
    }
    if groups.is_empty() {
        return "none — `submilli:http` is blocked by policy".into();
    }
    groups
        .iter()
        .map(|(labels, scope)| format!("{} → {}", labels.join(", "), render_scope(scope)))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The `http.*` capability names, in catalog order.
fn http_capabilities() -> impl Iterator<Item = &'static str> {
    capabilities::catalog()
        .iter()
        .filter(|g| g.module == HTTP_MODULE)
        .flat_map(|g| g.capabilities.iter().map(|c| c.name))
}

/// The host scope `allow`ed for `capability`, or `None` if no rule allows it.
/// Unions across rules: any unconstrained-host rule wins (`Any`); otherwise the
/// host patterns from every allowing rule are gathered.
fn allowed_host_scope(
    rules: Option<&Vec<submilli_blueprint::PermissionRule>>,
    capability: &str,
) -> Option<HostScope> {
    let allowing = rules?
        .iter()
        .filter(|r| r.capability == capability && r.action == Action::Allow);
    let mut hosts = Vec::new();
    let mut allowed = false;
    for rule in allowing {
        allowed = true;
        match rule_host_scope(rule.filter.as_ref()) {
            HostScope::Any => return Some(HostScope::Any),
            HostScope::Hosts(patterns) => {
                for p in patterns {
                    if !hosts.contains(&p) {
                        hosts.push(p);
                    }
                }
            }
        }
    }
    if !allowed {
        return None;
    }
    Some(HostScope::Hosts(hosts))
}

/// The host scope a single `allow` rule grants: `Any` when nothing pins the
/// host (no filter, a filter that doesn't mention `host`, or a `host glob "*"`),
/// otherwise the listed host patterns.
fn rule_host_scope(filter: Option<&FilterExpr>) -> HostScope {
    let Some(filter) = filter else {
        return HostScope::Any;
    };
    let matches = filter.field_matches("host");
    if matches.is_empty() || matches.iter().any(is_wildcard_glob) {
        return HostScope::Any;
    }
    HostScope::Hosts(matches.iter().map(render_match).collect())
}

fn is_wildcard_glob(m: &FieldMatch) -> bool {
    matches!(m, FieldMatch::Glob(p) if p == "*")
}

fn render_match(m: &FieldMatch) -> String {
    match m {
        FieldMatch::Equals(s) | FieldMatch::Glob(s) => s.clone(),
        FieldMatch::Regex(s) => format!("/{s}/"),
    }
}

fn render_scope(scope: &HostScope) -> String {
    match scope {
        HostScope::Any => "any host".into(),
        HostScope::Hosts(hosts) => hosts.join(", "),
    }
}

fn same_scope(a: &HostScope, b: &HostScope) -> bool {
    match (a, b) {
        (HostScope::Any, HostScope::Any) => true,
        (HostScope::Hosts(x), HostScope::Hosts(y)) => x == y,
        _ => false,
    }
}

/// `http.get` → `GET`; `http.download` keeps its verb-less name.
fn method_label(capability: &str) -> String {
    let verb = capability.strip_prefix("http.").unwrap_or(capability);
    match verb {
        "download" => "download".to_string(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use submilli_blueprint::VfsConfig;

    #[test]
    fn discovery_names_match_each_prompt_surface() {
        for surface in [PromptSurface::Mcp, PromptSurface::Rest] {
            let rendered = execute_tool_description(&Blueprint::default(), surface);
            for name in surface.discovery_tools() {
                assert!(rendered.contains(&format!("`{name}`")), "{rendered}");
            }
            for stale in [
                "`packages.search`",
                "`packages.docs`",
                "`builtins.docs`",
                "{t_",
            ] {
                assert!(!rendered.contains(stale), "{rendered}");
            }
        }
    }

    /// Build a blueprint from `main`-caller rule lines (real parse path).
    fn blueprint_with(rule_lines: &str) -> Blueprint {
        let yaml = format!("name: t\npermissions:\n  main:\n{rule_lines}");
        submilli_blueprint::parse(&yaml).expect("valid blueprint")
    }

    #[test]
    fn template_strips_markers_and_resolves_placeholders() {
        let template = prompt_template();
        assert!(!template.is_empty());
        assert!(!template.starts_with("\n## The prompt"));

        let rendered = execute_tool_description(&Blueprint::default(), PromptSurface::Mcp);
        assert!(!rendered.contains("{vfs_mode}"));
        assert!(!rendered.contains("{http_access}"));
        assert!(!rendered.contains("{builtins}"));
        assert!(!rendered.contains("{mcp_packages}"));
    }

    #[test]
    fn prompt_omits_hidden_libraries_and_their_guidance() {
        let denied = execute_tool_description(&Blueprint::default(), PromptSurface::Mcp);
        for hidden in [
            "submilli:http",
            "submilli:fs",
            "submilli:code",
            "When making outbound HTTP",
            "Sandbox: File system",
            "Network (",
        ] {
            assert!(!denied.contains(hidden), "{hidden}: {denied}");
        }
        for placeholder in [
            "{stdlib_modules}",
            "{sandbox}",
            "{http_guidance}",
            "{http_access}",
        ] {
            assert!(!denied.contains(placeholder));
        }
        for action in ["allow", "ask-human"] {
            let blueprint = blueprint_with(&format!(
                "    - capability: http.get\n      action: {action}\n"
            ));
            let rendered = execute_tool_description(&blueprint, PromptSurface::Mcp);
            assert!(rendered.contains("submilli:http"));
            assert!(rendered.contains("When making outbound HTTP"));
            assert!(!rendered.contains("submilli:fs"));
            assert!(!rendered.contains("submilli:code"));
            assert!(!rendered.contains("blocked by policy"));
        }
        for (capability, code_visible) in [("fs.mkdir", false), ("fs.read", true)] {
            let blueprint = blueprint_with(&format!(
                "    - capability: {capability}\n      action: allow\n"
            ));
            let rendered = execute_tool_description(&blueprint, PromptSurface::Mcp);
            assert!(rendered.contains("submilli:fs"));
            assert!(rendered.contains("Sandbox: File system ephemeral"));
            assert_eq!(rendered.contains("submilli:code"), code_visible);
            assert!(!rendered.contains("submilli:http"));
        }
    }

    #[test]
    fn llm_prompt_guidance_requires_configuration_and_permission() {
        let config = "llm:\n  providers:\n    test:\n      type: anthropic\n  models:\n    test-model:\n      provider: test\n";
        for (configuration, policy, visible) in [
            ("", "default: allow\n", false),
            (config, "", false),
            (config, "default: allow\n", true),
            (
                config,
                "permissions:\n  main:\n    - capability: llm.call\n      action: ask-human\n",
                true,
            ),
        ] {
            let blueprint =
                submilli_blueprint::parse(&format!("name: test\n{configuration}{policy}")).unwrap();
            let prompt = execute_tool_description(&blueprint, PromptSurface::Mcp);
            for text in [
                "submilli:llm",
                "Model calls",
                "call(model, prompt)",
                "call<T>",
                "contextWindow",
                "models()",
            ] {
                assert_eq!(prompt.contains(text), visible, "{text}: {prompt}");
            }
            assert!(!prompt.contains("{llm_guidance}"));
        }
    }

    #[test]
    fn session_prompt_guidance_requires_read_and_write() {
        let rule =
            |capability: &str| format!("    - capability: {capability}\n      action: allow\n");
        for (policy, visible) in [
            (String::new(), false),
            (
                format!("permissions:\n  main:\n{}", rule("session.read")),
                false,
            ),
            (
                format!("permissions:\n  main:\n{}", rule("session.write")),
                false,
            ),
            ("default: allow\n".into(), true),
            (
                format!(
                    "permissions:\n  main:\n{}{}",
                    rule("session.read"),
                    rule("session.write")
                ),
                true,
            ),
        ] {
            let blueprint = submilli_blueprint::parse(&format!("name: test\n{policy}")).unwrap();
            let prompt = execute_tool_description(&blueprint, PromptSurface::Mcp);
            for text in ["submilli:session", "Session state", "memory-only"] {
                assert_eq!(prompt.contains(text), visible, "{policy}: {text}: {prompt}");
            }
            assert!(!prompt.contains("{session_guidance}"));
        }
    }

    #[test]
    fn git_prompt_guidance_follows_configuration_not_capability_grants() {
        for config in [
            "",
            "git:\n  identity:\n    name: Agent\n    email: agent@example.com\n",
        ] {
            for policy in [
                "default: allow\n",
                "default: deny\n",
                "permissions:\n  main:\n    - capability: git.init\n      action: allow\n",
            ] {
                let blueprint =
                    submilli_blueprint::parse(&format!("name: test\n{config}{policy}")).unwrap();
                let prompt = execute_tool_description(&blueprint, PromptSurface::Mcp);
                for text in [
                    "submilli:git",
                    "Git is available",
                    "init, clone, fetch/pull",
                    "Push is not available",
                ] {
                    assert_eq!(
                        prompt.contains(text),
                        !config.is_empty(),
                        "{text}: {prompt}"
                    );
                }
                assert!(!prompt.contains("{git_package}"));
            }
        }
    }

    #[test]
    fn mcp_packages_absent_without_servers() {
        let rendered = execute_tool_description(&Blueprint::default(), PromptSurface::Mcp);
        assert!(mcp_packages_phrase(&Blueprint::default()).is_empty());
        assert!(!rendered.contains("MCP packages available"));
    }

    #[test]
    fn mcp_packages_require_discovery_before_importing() {
        let bp = submilli_blueprint::parse("name: t\nmcp:\n  linear:\n    url: https://x/mcp\n")
            .expect("valid blueprint");
        let rendered = execute_tool_description(&bp, PromptSurface::Mcp);
        assert!(rendered.contains("MCP servers are configured"));
        assert!(!mcp_packages_phrase(&bp).contains("@mcp/linear"));
        assert!(rendered.contains("unauthenticated or unreachable servers are omitted"));
        assert!(rendered.contains("return type is literally `unknown`"));
        assert!(rendered.contains("use the result directly"));
        assert!(!rendered.contains("raw `string`"));
        assert!(!rendered.contains("{mcp_packages}"));
    }

    #[test]
    fn vfs_mode_reflects_the_config() {
        let sess = Blueprint {
            vfs: VfsConfig::PerSession {
                size_limit: Some(1024),
                mounts: Default::default(),
            },
            ..Blueprint::default()
        };
        assert!(vfs_mode_phrase(&Blueprint::default().vfs).contains("ephemeral"));
        assert!(vfs_mode_phrase(&sess.vfs).contains("per_session"));
    }

    #[test]
    fn vfs_mode_names_named_roots_and_mounts() {
        let bp = submilli_blueprint::parse(
            "name: x\nvfs:\n  mode: named\n  volume: notes\n  access: read_only\n  mounts:\n    /memory: {mode: named, volume: memory}\n",
        )
        .unwrap();
        let phrase = vfs_mode_phrase(&bp.vfs);
        assert!(
            phrase.starts_with("named — volume `notes` (read-only)"),
            "{phrase}"
        );
        assert!(
            phrase.contains("`/memory` → volume `memory` (access set by the server)"),
            "{phrase}"
        );
    }

    #[test]
    fn no_http_rules_reads_as_blocked() {
        assert_eq!(
            http_access_phrase(&Blueprint::default()),
            "none — `submilli:http` is blocked by policy"
        );
        // Approval keeps discovery visible without promising runtime authorization.
        let bp = blueprint_with("    - capability: http.get\n      action: ask-human\n");
        assert_eq!(
            http_access_phrase(&bp),
            "subject to policy; approval rules do not authorize execution automatically"
        );
    }

    #[test]
    fn unfiltered_allow_is_any_host() {
        let bp = blueprint_with("    - capability: http.get\n      action: allow\n");
        assert_eq!(http_access_phrase(&bp), "GET → any host");
    }

    #[test]
    fn wildcard_glob_and_host_free_filter_are_any_host() {
        let star = blueprint_with(
            "    - capability: http.get\n      filter: host glob \"*\"\n      action: allow\n",
        );
        assert_eq!(http_access_phrase(&star), "GET → any host");
        // a filter that constrains only the body size leaves the host unbounded
        let body_size_only = blueprint_with(
            "    - capability: http.post\n      filter: body_size < 1000\n      action: allow\n",
        );
        assert_eq!(http_access_phrase(&body_size_only), "POST → any host");
    }

    #[test]
    fn specific_hosts_are_listed_and_methods_grouped_by_scope() {
        let allow_host = |cap: &str, host: &str| {
            format!(
                "    - capability: {cap}\n      filter: host == \"{host}\"\n      action: allow\n"
            )
        };
        let rules = format!(
            "{}{}    - capability: http.download\n      filter: host glob \"cdn.example.com\"\n      action: allow\n",
            allow_host("http.get", "api.example.com"),
            allow_host("http.post", "api.example.com"),
        );
        let bp = blueprint_with(&rules);
        assert_eq!(
            http_access_phrase(&bp),
            "GET, POST → api.example.com; download → cdn.example.com"
        );
    }
}
