//! What a blueprint lets a program use, as documentation shows it: the
//! standard library modules and registry packages narrowed to the callables
//! `main` could call, and whether the session's `@tools` are callable.
//!
//! Showing is broader than allowing: a callable stays when some call to it
//! could proceed, with a note on what limits it, such as a filter or a
//! person's approval. Every discovery surface reads one [`DocsScope`], so
//! docs, search, suggestions and catalog listings agree.

use std::collections::{BTreeMap, BTreeSet};

use interpreter::doc_directives::DirectiveError;
use interpreter::docs_view::{
    Availability, DocAvailability, project_declaration, scoped_documentation,
};
use interpreter::packages::StdlibView;
use interpreter::{DocCapability, PackageDeclaration, Stdlib};
use submilli_blueprint::Blueprint;
use submilli_build::Artifact;
use submilli_policy::RulesAvailability;

use interpreter::mangle::USER_PACKAGE;

/// The filesystem capabilities the workspace tools of `submilli:code` use,
/// which its declarations do not tag.
const CODE_CAPABILITIES: [&str; 4] = ["fs.read", "fs.write", "fs.stat", "fs.list"];

/// What `main` could use under a blueprint, or under no blueprint at all,
/// with what the host must have configured for some capabilities to work.
pub struct HostAvailability<'a> {
    /// `None` when unscoped: every capability is available.
    rules: Option<RulesAvailability<'a, Blueprint>>,
    /// Git is opt-in: a blueprint enables the module by configuring it.
    git: bool,
    llm: bool,
    embedding: bool,
}

impl<'a> HostAvailability<'a> {
    pub fn for_blueprint(blueprint: &'a Blueprint) -> Self {
        Self {
            rules: Some(RulesAvailability::new(blueprint, USER_PACKAGE)),
            git: blueprint.git.is_some(),
            llm: !blueprint.llm.models.is_empty(),
            embedding: !blueprint.embedding.models.is_empty(),
        }
    }

    /// Discovery with no blueprint: everything but the opt-in Git module.
    pub fn unscoped() -> HostAvailability<'static> {
        HostAvailability {
            rules: None,
            git: false,
            llm: true,
            embedding: true,
        }
    }

    /// Model calls need a configured model and Git calls a configured
    /// repository setup, whatever the rules allow.
    fn configured(&self, capability: &str) -> bool {
        match capability {
            "llm.call" => self.llm,
            "embedding.embed" => self.embedding,
            _ if is_git_capability(capability) => self.git,
            _ => true,
        }
    }
}

/// A standard library Git capability, as opposed to a package's that merely
/// starts with `git.`.
fn is_git_capability(capability: &str) -> bool {
    capability.starts_with("git.") && interpreter::stdlib::capabilities::find(capability).is_some()
}

impl DocAvailability for HostAvailability<'_> {
    fn tag(&self, tag: &DocCapability) -> Availability {
        if !self.configured(&tag.capability) {
            return Availability::Denied;
        }
        match &self.rules {
            Some(rules) => rules.tag(tag),
            None => Availability::OPEN,
        }
    }

    fn capability(&self, name: &str) -> Availability {
        if !self.configured(name) {
            return Availability::Denied;
        }
        match &self.rules {
            Some(rules) => rules.capability(name),
            None => Availability::OPEN,
        }
    }
}

/// The artifacts of the registry packages `packages` names. One that fails to
/// load is left out: discovery shows it as declared but not loaded.
pub fn load_declared<'a>(
    packages: impl IntoIterator<Item = &'a String>,
    store: &submilli_build::PackageStore,
) -> Vec<Artifact> {
    packages
        .into_iter()
        .filter_map(|package| store.load(package).ok())
        .collect()
}

/// Whether programs under `blueprint` could call the session's `@tools`.
pub fn tools_availability(blueprint: &Blueprint) -> Availability {
    HostAvailability::for_blueprint(blueprint).capability(interpreter::runtime::TOOLS_CAPABILITY)
}

/// A registry package as a reader sees it.
#[derive(Debug, Clone)]
pub struct PackageView {
    pub name: String,
    pub version: String,
    pub description: String,
    pub keywords: Vec<String>,
    /// The declaration narrowed to what the reader can call.
    pub declaration: PackageDeclaration,
    /// [`Self::declaration`], rendered.
    pub declarations: String,
    /// The package's `docs/readme.md` with the regions the reader cannot use
    /// removed, or the directives that make it malformed. Malformed
    /// documentation is never shown whole in its place.
    pub documentation: Result<String, Vec<DirectiveError>>,
    /// Whether anything is left to call.
    pub usable: bool,
}

impl PackageView {
    /// `artifact` as `availability` lets a reader use it; pass
    /// [`interpreter::docs_view::Unscoped`] for an author reading all of it.
    pub fn new(artifact: &Artifact, availability: &dyn DocAvailability) -> Self {
        let projection = project_declaration(&artifact.package_declaration, availability);
        Self {
            name: artifact.metadata.package_name.clone(),
            version: artifact.metadata.package_version.clone(),
            description: artifact.metadata.description.clone(),
            keywords: artifact.metadata.keywords.clone(),
            declarations: interpreter::packages::render_declarations(&projection.declaration),
            declaration: projection.declaration,
            documentation: scoped_documentation(&artifact.documentation, availability),
            usable: projection.usable,
        }
    }

    /// Whether a lowercase search `query` names the package, its description,
    /// a keyword, or something the reader can use from it.
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_lowercase().contains(query)
            || self.description.to_lowercase().contains(query)
            || self
                .keywords
                .iter()
                .any(|keyword| keyword.to_lowercase().contains(query))
            || self
                .declaration
                .values
                .keys()
                .chain(self.declaration.types.keys())
                .any(|name| name.to_lowercase().contains(query))
    }

    /// The one-line description listings show.
    pub fn summary(&self) -> String {
        if self.description.trim().is_empty() {
            format!("Registry package {} v{}.", self.name, self.version)
        } else {
            self.description.clone()
        }
    }
}

/// Why a declared package with nothing left to call is not shown.
pub fn nothing_allowed_message(name: &str) -> String {
    format!(
        "`{name}` is declared by this blueprint, but the blueprint allows none of its operations"
    )
}

/// Why a package's installed documentation cannot be shown, naming each
/// malformed directive.
pub fn invalid_documentation_message(name: &str, errors: &[DirectiveError]) -> String {
    let errors = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    format!("{} ({errors})", invalid_documentation_summary(name))
}

/// [`invalid_documentation_message`] without the directives, for a reader
/// shown them line by line.
pub fn invalid_documentation_summary(name: &str) -> String {
    format!(
        "the installed documentation of `{name}` has malformed directives; rebuild or reinstall \
         the package"
    )
}

/// Why a declared package's documentation is missing altogether.
pub fn not_loaded_message(name: &str) -> String {
    format!(
        "`{name}` is declared by this blueprint but its artifact could not be loaded — run \
         `submilli install` to fetch it."
    )
}

/// The documentation a reader can use, from one blueprint or none.
pub struct DocsScope {
    stdlib: StdlibView,
    tools: Availability,
    /// The registry packages the blueprint declares, loaded or not.
    declared: BTreeSet<String>,
    packages: BTreeMap<String, PackageView>,
}

impl DocsScope {
    /// Discovery with no blueprint: the standard library but Git, no
    /// registry packages, and no `@tools`, which belong to a session.
    pub fn unscoped() -> Self {
        Self::new(
            &HostAvailability::unscoped(),
            Availability::Denied,
            BTreeSet::new(),
            [],
        )
    }

    /// [`Self::unscoped`], built once per process.
    pub fn unscoped_shared() -> &'static Self {
        static UNSCOPED: std::sync::LazyLock<DocsScope> =
            std::sync::LazyLock::new(DocsScope::unscoped);
        &UNSCOPED
    }

    /// What `blueprint` lets `main` use. `artifacts` are the loaded registry
    /// packages; those the blueprint does not declare are left out.
    pub fn for_blueprint<'a>(
        blueprint: &Blueprint,
        artifacts: impl IntoIterator<Item = &'a Artifact>,
    ) -> Self {
        let declared: BTreeSet<String> = blueprint.packages.iter().cloned().collect();
        let artifacts = artifacts
            .into_iter()
            .filter(|artifact| declared.contains(&artifact.metadata.package_name));
        Self::new(
            &HostAvailability::for_blueprint(blueprint),
            tools_availability(blueprint),
            declared.clone(),
            artifacts,
        )
    }

    fn new<'a>(
        availability: &HostAvailability<'_>,
        tools: Availability,
        declared: BTreeSet<String>,
        artifacts: impl IntoIterator<Item = &'a Artifact>,
    ) -> Self {
        let code_usable = CODE_CAPABILITIES
            .iter()
            .any(|capability| availability.capability(capability).is_available());
        // Git is shown wherever a blueprint configures it: reading a
        // repository's status and history needs no grant.
        let stdlib = Stdlib::core().view_where(availability, |name, usable| match name {
            "submilli:code" => code_usable,
            "submilli:git" => availability.git,
            _ => usable,
        });
        let packages = artifacts
            .into_iter()
            .map(|artifact| {
                let view = PackageView::new(artifact, availability);
                (view.name.clone(), view)
            })
            .collect();
        Self {
            stdlib,
            tools,
            declared,
            packages,
        }
    }

    /// The standard library modules the reader can use.
    pub fn stdlib(&self) -> &StdlibView {
        &self.stdlib
    }

    /// The closest name to `name` among the modules and `extra` names the
    /// reader can use.
    pub fn suggest(&self, name: &str, extra: &[String]) -> Option<String> {
        self.stdlib
            .suggest_filtered(name, extra, |candidate| self.allows(candidate))
    }

    /// The modules and `extra` entries the reader can use, as a listing.
    pub fn catalog(
        &self,
        extra: Vec<interpreter::packages::CatalogEntry>,
    ) -> interpreter::packages::Catalog {
        self.stdlib
            .catalog_filtered(extra, |name| self.allows(name))
    }

    /// Whether programs could call the session's `@tools`, and what limits
    /// the calls.
    pub fn tools(&self) -> &Availability {
        &self.tools
    }

    /// Whether the blueprint declares the registry package `name`, whether
    /// or not its artifact loaded.
    pub fn declares(&self, name: &str) -> bool {
        self.declared.contains(name)
    }

    /// A declared registry package that loaded, usable or not.
    pub fn package(&self, name: &str) -> Option<&PackageView> {
        self.packages.get(name)
    }

    /// The declared registry packages with something to call.
    pub fn usable_packages(&self) -> impl Iterator<Item = &PackageView> {
        self.packages.values().filter(|package| package.usable)
    }

    /// Whether discovery may advertise `name`: a standard library module or
    /// registry package with something left to call, or `@tools` when its
    /// calls could proceed. Other names, such as `@mcp/<server>`, are
    /// decided where they are resolved.
    pub fn allows(&self, name: &str) -> bool {
        if name == interpreter::runtime::TOOLS_PACKAGE {
            return self.tools.is_available();
        }
        if let Some(package) = self.packages.get(name) {
            return package.usable;
        }
        if name.starts_with("submilli:") {
            return self.stdlib.contains(name);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blueprint(yaml: &str) -> Blueprint {
        submilli_blueprint::parse(yaml).expect("blueprint parses")
    }

    fn declarations(scope: &DocsScope, module: &str) -> String {
        scope
            .stdlib()
            .docs(module)
            .map(|doc| doc.declarations)
            .unwrap_or_default()
    }

    #[test]
    fn git_capabilities_need_git_configured() {
        let open = blueprint("name: t\ndefault: allow\n");
        let availability = HostAvailability::for_blueprint(&open);
        assert_eq!(availability.capability("git.clone"), Availability::Denied);
        assert!(availability.capability("fs.read").is_available());
        // A package's capability that only starts like Git's is not Git.
        assert!(
            availability
                .capability("git.sr.ht/repos.list")
                .is_available()
        );
    }

    #[test]
    fn unscoped_shows_the_standard_library_but_git_and_tools() {
        let scope = DocsScope::unscoped();
        for module in [
            "submilli:http",
            "submilli:fs",
            "submilli:code",
            "submilli:session",
        ] {
            assert!(scope.allows(module), "{module}");
        }
        assert!(!scope.allows("submilli:git"));
        assert!(!scope.allows("@tools"));
        assert!(scope.allows("submilli:url"));
    }

    #[test]
    fn a_read_only_blueprint_shows_reads_and_hides_writes() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n    - capability: session.read\n      action: allow\n",
            ),
            &[],
        );
        let fs = declarations(&scope, "submilli:fs");
        assert!(fs.contains("function readText("), "{fs}");
        assert!(!fs.contains("function writeText("), "{fs}");
        assert!(scope.allows("submilli:code"));
        let session = declarations(&scope, "submilli:session");
        assert!(session.contains("function get"), "{session}");
        assert!(!session.contains("function set("), "{session}");
        assert!(!scope.allows("submilli:http"));
        assert!(!scope.allows("submilli:llm"));
    }

    #[test]
    fn get_only_http_keeps_request_restricted_and_hides_download() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n    - capability: http.request\n      filter: method == \"GET\"\n      action: allow\n    - capability: http.download\n      action: allow\n",
            ),
            &[],
        );
        let http = declarations(&scope, "submilli:http");
        assert!(http.contains("function get("), "{http}");
        assert!(!http.contains("function post("), "{http}");
        assert!(http.contains("function request("), "{http}");
        assert!(
            http.contains(
                "Under this blueprint, `http.request` is allowed when `method == \"GET\"`; otherwise denied."
            ),
            "{http}"
        );
        // Downloading writes a file too, which the blueprint does not allow.
        assert!(!http.contains("function download("), "{http}");
    }

    #[test]
    fn ask_human_operations_are_shown_with_a_note() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: fs.write\n      action: ask-human\n",
            ),
            &[],
        );
        let fs = declarations(&scope, "submilli:fs");
        assert!(fs.contains("function writeText("), "{fs}");
        assert!(
            fs.contains("Under this blueprint, `fs.write` needs a person's approval."),
            "{fs}"
        );
    }

    #[test]
    fn a_shadowed_grant_hides_the_module() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: llm.call\n      action: deny\n    - capability: llm.call\n      action: allow\nllm:\n  providers:\n    test:\n      type: anthropic\n  models:\n    test-model:\n      provider: test\n",
            ),
            &[],
        );
        assert!(!scope.allows("submilli:llm"));
    }

    #[test]
    fn configuration_prerequisites_hide_what_cannot_work() {
        let unconfigured = DocsScope::for_blueprint(&blueprint("name: t\ndefault: allow\n"), &[]);
        assert!(!unconfigured.allows("submilli:git"));
        assert!(!unconfigured.allows("submilli:llm"));
        assert!(!unconfigured.allows("submilli:embedding"));
        assert!(!unconfigured.allows("submilli:secrets"));
        assert!(unconfigured.allows("submilli:http"));
    }

    #[test]
    fn tools_follow_first_match_rules() {
        let filtered = blueprint(
            "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: tools.call\n      filter: tool == \"query_orders\"\n      action: allow\n",
        );
        let scope = DocsScope::for_blueprint(&filtered, &[]);
        assert!(scope.allows("@tools"));
        assert!(matches!(scope.tools(), Availability::Available { notes } if !notes.is_empty()));
        let shadowed = blueprint(
            "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: tools.call\n      action: deny\n    - capability: tools.call\n      action: allow\n",
        );
        assert!(!DocsScope::for_blueprint(&shadowed, &[]).allows("@tools"));
    }

    #[test]
    fn read_only_standard_library_snapshot() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n    - capability: fs.stat\n      action: allow\n    - capability: fs.list\n      action: allow\n    - capability: session.read\n      action: allow\n    - capability: session.list\n      action: allow\n",
            ),
            &[],
        );
        let modules: Vec<&str> = scope.stdlib().names().collect();
        insta::assert_snapshot!(
            "read_only_standard_library",
            format!(
                "modules: {}\n\n# submilli:fs\n\n{}\n\n# submilli:session\n\n{}\n",
                modules.join(", "),
                declarations(&scope, "submilli:fs"),
                declarations(&scope, "submilli:session")
            )
        );
    }

    #[test]
    fn restricted_http_snapshot() {
        let scope = DocsScope::for_blueprint(
            &blueprint(
                "name: t\ndefault: deny\npermissions:\n  main:\n    - capability: http.get\n      filter: host == \"api.example.com\"\n      action: allow\n    - capability: http.request\n      filter: method == \"GET\" or method == \"HEAD\"\n      action: allow\n    - capability: http.post\n      action: ask-human\n",
            ),
            &[],
        );
        insta::assert_snapshot!("restricted_http", declarations(&scope, "submilli:http"));
    }
}
