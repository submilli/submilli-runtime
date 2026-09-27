//! Catalog of the semantic-security capabilities the stdlib host functions
//! gate via `check_security`. This is the single source of truth behind
//! `submilli blueprint init`, the `submilli blueprint capability` verbs, and
//! the server's `GET /v1/capabilities`, so an operator sees every capability
//! the runtime can gate.
//!
//! **Keep in sync with the host functions.** When a host fn in `stdlib::fs` /
//! `stdlib::http` (or a new gated module) starts or stops gating a capability,
//! update the matching entry here — see CLAUDE.md. The `capabilities` test in
//! the `submilli` CLI asserts every `example_filter` below parses.

/// One context field a capability's `filter:` expression can match on.
pub struct FilterField {
    pub name: &'static str,
    /// The operand's shape in a filter comparison: `string`, `number`, or
    /// `boolean`.
    pub ty: &'static str,
    /// One line on what the field carries.
    pub doc: &'static str,
}

const fn field(name: &'static str, ty: &'static str, doc: &'static str) -> FilterField {
    FilterField { name, ty, doc }
}

/// One gated capability and the policy `filter:` surface it exposes.
pub struct Capability {
    /// The name passed to `check()` — exactly what a policy rule's
    /// `capability:` must equal (capability names are matched verbatim).
    pub name: &'static str,
    /// Why the runtime refuses this capability to `main` outright, ahead of the
    /// policy engine — `None` for an ordinary capability a rule can grant to any
    /// caller. A rule granting a marked capability to `main` is dead, so the
    /// surfaces that scaffold, add, and lint rules read this one fact rather
    /// than each restating the carve-out, and each shows the reason verbatim.
    pub main_denial: Option<&'static str>,
    /// One-line description of what gating the capability controls.
    pub summary: &'static str,
    /// Context fields a `filter:` expression can match on, as supplied by a
    /// representative call. Documents what a rule can constrain.
    pub filter_fields: &'static [FilterField],
    /// A ready-to-uncomment example `filter:` expression for the scaffold.
    pub example_filter: &'static str,
}

impl Capability {
    /// Whether the name is a template (`mcp.<server>`) rather than a concrete
    /// name a policy rule can use verbatim.
    pub fn is_template(&self) -> bool {
        self.name.contains('<')
    }

    /// Whether a policy rule granting this capability to `main` can take effect.
    pub fn grantable_to_main(&self) -> bool {
        self.main_denial.is_none()
    }

    /// The filter fields' names, for surfaces that don't show types.
    pub fn field_names(&self) -> impl Iterator<Item = &'static str> {
        self.filter_fields.iter().map(|f| f.name)
    }
}

/// A group of capabilities sharing a source module, for scaffold sectioning.
pub struct CapabilityGroup {
    /// The owning stdlib module, e.g. `submilli:fs`.
    pub module: &'static str,
    pub capabilities: &'static [Capability],
}

const OP: FilterField = field(
    "op",
    "string",
    "The specific operation within the capability, e.g. \"read\", \"readText\"",
);
const PATH: FilterField = field(
    "path",
    "string",
    "Normalized absolute VFS path the call targets",
);
const RECURSIVE: FilterField = field(
    "recursive",
    "boolean",
    "Whether the operation applies recursively",
);
const FROM: FilterField = field("from", "string", "Normalized absolute source VFS path");
const TO: FilterField = field("to", "string", "Normalized absolute destination VFS path");
const KEY: FilterField = field("key", "string", "Session key the call targets");
const PREFIX: FilterField = field("prefix", "string", "Session key prefix being listed");

const FS: &[Capability] = &[
    Capability {
        name: "fs.read",
        main_denial: None,
        summary: "Read files and code workspace content (including search and ignore rules)",
        filter_fields: &[OP, PATH],
        example_filter: "path glob \"*.csv\"",
    },
    Capability {
        name: "fs.write",
        main_denial: None,
        summary: "Create, write, append, or apply code edits to files",
        filter_fields: &[OP, PATH],
        example_filter: "path glob \"/out/*\"",
    },
    Capability {
        name: "fs.stat",
        main_denial: None,
        summary: "Inspect metadata (including code workspace discovery)",
        filter_fields: &[OP, PATH],
        example_filter: "path glob \"/data/*\"",
    },
    Capability {
        name: "fs.list",
        main_denial: None,
        summary: "List directory entries (including code search, glob and tree)",
        filter_fields: &[OP, PATH, RECURSIVE],
        example_filter: "path glob \"/data/*\"",
    },
    Capability {
        name: "fs.mkdir",
        main_denial: None,
        summary: "Create directories",
        filter_fields: &[OP, PATH, RECURSIVE],
        example_filter: "path glob \"/tmp/*\"",
    },
    Capability {
        name: "fs.remove",
        main_denial: None,
        summary: "Delete files or directories",
        filter_fields: &[OP, PATH, RECURSIVE],
        example_filter: "path glob \"/tmp/*\"",
    },
    Capability {
        name: "fs.move",
        main_denial: None,
        summary: "Move or rename a path",
        filter_fields: &[OP, FROM, TO],
        example_filter: "to glob \"/archive/*\"",
    },
    Capability {
        name: "fs.copy",
        main_denial: None,
        summary: "Copy a path",
        filter_fields: &[OP, FROM, TO, RECURSIVE],
        example_filter: "to glob \"/backup/*\"",
    },
];

const BRANCH: FilterField = field("branch", "string", "Local or requested remote branch name");
const REMOTE: FilterField = field("remote", "string", "Exact HTTPS repository URL");
const REMOTE_NAME: FilterField = field("remoteName", "string", "Named remote, for example origin");
const GIT: &[Capability] = &[
    Capability {
        name: "git.init",
        main_denial: None,
        summary: "Create a local repository and its VFS directory",
        filter_fields: &[OP, PATH],
        example_filter: "path == \"/repo\"",
    },
    Capability {
        name: "git.clone",
        main_denial: None,
        summary: "Clone an HTTPS repository into a VFS directory",
        filter_fields: &[OP, PATH, REMOTE_NAME, REMOTE, BRANCH],
        example_filter: "path == \"/repo\" and remote == \"https://github.com/acme/project.git\"",
    },
    Capability {
        name: "git.fetch",
        main_denial: None,
        summary: "Fetch or pull HTTPS remote branches into an existing repository",
        filter_fields: &[OP, PATH, REMOTE_NAME, REMOTE, BRANCH],
        example_filter: "path == \"/repo\" and remote == \"https://github.com/acme/project.git\"",
    },
    Capability {
        name: "git.commit",
        main_denial: None,
        summary: "Commit staged changes with blueprint identity",
        filter_fields: &[OP, PATH, BRANCH],
        example_filter: "path == \"/repo\" and branch == \"main\"",
    },
];

const HTTP_VERB_FIELDS: &[FilterField] = &[
    field("host", "string", "Destination host, without port"),
    field("path", "string", "URL path component"),
    field("body_size", "number", "Request body size in bytes"),
    field("timeout_ms", "number", "Request timeout in milliseconds"),
];

const HTTP: &[Capability] = &[
    Capability {
        name: "http.get",
        main_denial: None,
        summary: "HTTP GET",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.post",
        main_denial: None,
        summary: "HTTP POST",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.put",
        main_denial: None,
        summary: "HTTP PUT",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.patch",
        main_denial: None,
        summary: "HTTP PATCH",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.delete",
        main_denial: None,
        summary: "HTTP DELETE",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.head",
        main_denial: None,
        summary: "HTTP HEAD",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.options",
        main_denial: None,
        summary: "HTTP OPTIONS",
        filter_fields: HTTP_VERB_FIELDS,
        example_filter: "host == \"api.example.com\"",
    },
    Capability {
        name: "http.download",
        main_denial: None,
        summary: "Download a URL straight to the VFS",
        filter_fields: &[
            field("host", "string", "Download host, without port"),
            field("url_path", "string", "URL path component"),
            field(
                "vfs_path",
                "string",
                "Normalized absolute destination path in the VFS",
            ),
            field(
                "max_bytes",
                "number",
                "Requested download size cap in bytes",
            ),
            field(
                "overwrite",
                "boolean",
                "Whether an existing file may be clobbered",
            ),
            field(
                "decompress",
                "boolean",
                "Whether the response is decompressed on write",
            ),
        ],
        example_filter: "host == \"cdn.example.com\" and overwrite == false",
    },
];

/// Outbound MCP calls — one capability per declared server, `mcp.<server>` (the
/// `<server>` placeholder is filled in per blueprint, whose `mcp:` block names its
/// servers). The tool being called is in the filter context, so a rule can allow a
/// server broadly or constrain it to specific tools.
const MCP: &[Capability] = &[Capability {
    name: "mcp.<server>",
    main_denial: None,
    summary: "Call tools on a declared outbound MCP server (streamable_http)",
    filter_fields: &[
        field("tool", "string", "Name of the MCP tool being called"),
        field(
            "transport",
            "string",
            "MCP transport; always \"streamable_http\" today",
        ),
    ],
    example_filter: "tool == \"create_issue\"",
}];

const LLM: &[Capability] = &[Capability {
    name: "llm.call",
    // No `main_denial`: main-module access is the point. A model call is not a
    // secret read — the program names a model and supplies a prompt, and the
    // credential is resolved inside the provider and never returned.
    main_denial: None,
    summary: "Call a model (call, batch) and enumerate the models it may call (models). \
              Narrowing `model` also narrows what `models()` reveals: every candidate is \
              filtered through this same rule, so a listing never offers a model the \
              caller would be denied at call time",
    filter_fields: &[
        field(
            "op",
            "string",
            "Which operation: \"call\", \"batch\", or \"models\"",
        ),
        field(
            "model",
            "string",
            "Model name the call targets; \"\" on the `models` op itself, then each \
             candidate's own name as the listing is filtered",
        ),
        field(
            "prompt_count",
            "number",
            "Prompts in this dispatch — 1 for call, N for batch, 0 for models. Prompt text \
             is never in this context",
        ),
    ],
    example_filter: "model glob \"claude-*\"",
}];

const SECRETS: &[Capability] = &[Capability {
    name: "secrets.get",
    main_denial: Some(
        "secret values are never available to main-module code, and no policy can \
         grant this. The package that needs this credential resolves it internally \
         and never returns it — pass the secret NAME to that package's API instead",
    ),
    summary: "Read a blueprint-declared secret value",
    filter_fields: &[field("name", "string", "Declared secret name being read")],
    example_filter: "name == \"STRIPE_API_KEY\"",
}];

const SESSION: &[Capability] = &[
    Capability {
        name: "session.read",
        main_denial: None,
        summary: "Read session state (get, has), and decide which keys a list may reveal",
        filter_fields: &[OP, KEY],
        example_filter: "key glob \"triage/*\"",
    },
    Capability {
        name: "session.write",
        main_denial: None,
        summary: "Store or overwrite a session value (set)",
        filter_fields: &[OP, KEY],
        example_filter: "key glob \"triage/*\"",
    },
    Capability {
        name: "session.remove",
        main_denial: None,
        summary: "Delete a session key",
        filter_fields: &[OP, KEY],
        example_filter: "key glob \"triage/*\"",
    },
    Capability {
        name: "session.list",
        main_denial: None,
        summary: "Enumerate session keys under a prefix",
        filter_fields: &[OP, PREFIX],
        example_filter: "prefix == \"triage/\"",
    },
];

const CATALOG: &[CapabilityGroup] = &[
    CapabilityGroup {
        module: "submilli:fs",
        capabilities: FS,
    },
    CapabilityGroup {
        module: "submilli:git",
        capabilities: GIT,
    },
    CapabilityGroup {
        module: "submilli:http",
        capabilities: HTTP,
    },
    CapabilityGroup {
        module: "submilli:llm",
        capabilities: LLM,
    },
    CapabilityGroup {
        module: "@mcp",
        capabilities: MCP,
    },
    CapabilityGroup {
        module: "submilli:secrets",
        capabilities: SECRETS,
    },
    CapabilityGroup {
        module: "submilli:session",
        capabilities: SESSION,
    },
];

/// Every stdlib capability the runtime can gate, grouped by source module.
pub fn catalog() -> &'static [CapabilityGroup] {
    CATALOG
}

/// Look up a capability by its exact name (templates included, by their
/// literal `mcp.<server>` spelling).
pub fn find(name: &str) -> Option<&'static Capability> {
    CATALOG
        .iter()
        .flat_map(|group| group.capabilities)
        .find(|cap| cap.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Marking a capability not-grantable-to-`main` changes the runtime's
    /// answer for every caller, so growing the set is a deliberate act.
    #[test]
    fn only_secrets_get_is_refused_to_main() {
        let refused: Vec<&str> = catalog()
            .iter()
            .flat_map(|group| group.capabilities)
            .filter(|cap| !cap.grantable_to_main())
            .map(|cap| cap.name)
            .collect();
        assert_eq!(refused, ["secrets.get"]);
    }

    #[test]
    fn catalog_entries_are_well_formed() {
        let mut seen = HashSet::new();
        for group in catalog() {
            for cap in group.capabilities {
                assert!(
                    cap.name.contains('.') && !cap.name.is_empty(),
                    "capability name '{}' must be `module.action`",
                    cap.name
                );
                assert!(seen.insert(cap.name), "duplicate capability '{}'", cap.name);
                assert!(!cap.summary.is_empty(), "{}: empty summary", cap.name);
                // `filter_fields` may be empty for a capability gated by name alone
                // (e.g. the `@mcp` family, whose name already pins server + tool).
                assert!(
                    !cap.example_filter.is_empty(),
                    "{}: empty example_filter",
                    cap.name
                );
                assert!(
                    cap.main_denial.is_none_or(|reason| !reason.is_empty()),
                    "{}: empty main_denial reason",
                    cap.name
                );
                for f in cap.filter_fields {
                    assert!(!f.name.is_empty(), "{}: unnamed filter field", cap.name);
                    assert!(
                        matches!(f.ty, "string" | "number" | "boolean"),
                        "{}.{}: unknown field type '{}'",
                        cap.name,
                        f.name,
                        f.ty
                    );
                    assert!(!f.doc.is_empty(), "{}.{}: empty doc", cap.name, f.name);
                }
            }
        }
    }

    #[test]
    fn session_group_is_cataloged() {
        let group = catalog()
            .iter()
            .find(|g| g.module == "submilli:session")
            .expect("submilli:session group");
        let names: Vec<&str> = group.capabilities.iter().map(|cap| cap.name).collect();
        assert_eq!(
            names,
            [
                "session.read",
                "session.write",
                "session.remove",
                "session.list"
            ]
        );
        assert_eq!(
            find("session.list")
                .unwrap()
                .field_names()
                .collect::<Vec<_>>(),
            ["op", "prefix"]
        );
    }

    #[test]
    fn llm_group_is_cataloged() {
        let group = catalog()
            .iter()
            .find(|g| g.module == "submilli:llm")
            .expect("submilli:llm group");
        let names: Vec<&str> = group.capabilities.iter().map(|cap| cap.name).collect();
        assert_eq!(names, ["llm.call"]);
        assert_eq!(
            find("llm.call").unwrap().field_names().collect::<Vec<_>>(),
            ["op", "model", "prompt_count"]
        );
        // The double gate lives in the summary wording and the host fn, not in
        // the struct, so an operator reading only the catalog still has to learn
        // that narrowing `model` also shortens what `models()` returns.
        let summary = find("llm.call").unwrap().summary;
        assert!(
            summary.contains("models()"),
            "the summary must say narrowing `model` narrows discovery: {summary}"
        );
    }

    #[test]
    fn find_and_is_template() {
        assert_eq!(find("fs.read").map(|c| c.name), Some("fs.read"));
        assert!(find("fs.nope").is_none());
        assert!(find("mcp.<server>").is_some_and(Capability::is_template));
        assert!(!find("fs.read").unwrap().is_template());
    }
}
