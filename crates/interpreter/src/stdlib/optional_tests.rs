//! `submilli:agents` and `submilli:skills` end to end: a program compiled and run
//! under a [`Stdlib`] that enables them, against fake providers.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::runtime::agents::{
    AgentCallError, AgentInfo, AgentOutcome, AgentProvider, AgentRequest, AgentUsage,
};
use crate::runtime::skills::{Skill, SkillError, SkillInfo, SkillProvider};
use crate::runtime::{
    CheckOutcome, RuntimeConfig, SecurityCheck, StoreData, Vfs, dispatch_main_async,
    install_runtime_async_for,
};
use crate::stdlib::{OptionalPackage, Stdlib, capabilities};

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

fn harness_stdlib() -> Stdlib {
    Stdlib::core()
        .with(OptionalPackage::Agents)
        .with(OptionalPackage::Skills)
}

/// Answers every run with `answer`, lists `listing`, and records what it was
/// asked to run.
struct FakeAgents {
    answer: Result<String, AgentCallError>,
    listing: Result<Vec<AgentInfo>, AgentCallError>,
    requests: Mutex<Vec<AgentRequest>>,
}

impl FakeAgents {
    fn answering(answer: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(answer.to_string()),
            listing: Ok(two_agents()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn failing(error: AgentCallError) -> Arc<Self> {
        Arc::new(Self {
            answer: Err(error),
            listing: Ok(two_agents()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn listing(listing: Result<Vec<AgentInfo>, AgentCallError>) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(String::new()),
            listing,
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<AgentRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

impl AgentProvider for FakeAgents {
    fn run<'a>(&'a self, request: AgentRequest) -> Boxed<'a, Result<AgentOutcome, AgentCallError>> {
        self.requests.lock().expect("requests").push(request);
        let answer = self.answer.clone();
        Box::pin(async move {
            answer.map(|text| AgentOutcome {
                text,
                usage: Some(AgentUsage {
                    input_tokens: Some(3),
                    output_tokens: None,
                }),
            })
        })
    }

    fn agents<'a>(&'a self) -> Boxed<'a, Result<Vec<AgentInfo>, AgentCallError>> {
        let listing = self.listing.clone();
        Box::pin(async move { listing })
    }
}

fn two_agents() -> Vec<AgentInfo> {
    vec![
        AgentInfo {
            name: "researcher".to_string(),
            description: Some("Finds\nsources".to_string()),
        },
        AgentInfo {
            name: "deployer".to_string(),
            description: None,
        },
    ]
}

/// Two skills; `review` bundles one file. Records every name it is asked to
/// load and every file it is asked to read.
struct FakeSkills {
    loads: Mutex<Vec<String>>,
    reads: Mutex<Vec<(String, String)>>,
}

impl FakeSkills {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            loads: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
        })
    }
}

impl SkillProvider for FakeSkills {
    fn list<'a>(&'a self) -> Boxed<'a, Result<Vec<SkillInfo>, SkillError>> {
        Box::pin(async {
            Ok(vec![
                SkillInfo {
                    name: "review".to_string(),
                    description: Some("Review a diff".to_string()),
                },
                SkillInfo {
                    name: "internal".to_string(),
                    description: None,
                },
                // Not one name: `load` would refuse it, so `list()` drops it.
                SkillInfo {
                    name: "team/review".to_string(),
                    description: None,
                },
            ])
        })
    }

    fn load<'a>(&'a self, name: &'a str) -> Boxed<'a, Result<Skill, SkillError>> {
        self.loads.lock().expect("loads").push(name.to_string());
        Box::pin(async move {
            match name {
                "review" => Ok(Skill {
                    name: "review".to_string(),
                    description: Some("Review a diff".to_string()),
                    content: "Read the diff twice.".to_string(),
                }),
                _ => Err(SkillError::NotFound {
                    name: name.to_string(),
                }),
            }
        })
    }

    fn read_file<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
    ) -> Boxed<'a, Result<String, SkillError>> {
        self.reads
            .lock()
            .expect("reads")
            .push((name.to_string(), path.to_string()));
        Box::pin(async move {
            match (name, path) {
                ("review", "templates/report.md") => Ok("# Report".to_string()),
                _ => Err(SkillError::FileNotFound {
                    name: name.to_string(),
                    path: path.to_string(),
                }),
            }
        })
    }
}

/// Denies `capability` calls whose context field `field` equals `value`, and
/// records every context it is asked about.
struct DenyOne {
    field: &'static str,
    value: &'static str,
    contexts: Mutex<Vec<serde_json::Value>>,
}

impl DenyOne {
    fn new(field: &'static str, value: &'static str) -> Arc<Self> {
        Arc::new(Self {
            field,
            value,
            contexts: Mutex::new(Vec::new()),
        })
    }
}

impl SecurityCheck for DenyOne {
    fn check(&self, _caller: &str, _capability: &str, context: &serde_json::Value) -> CheckOutcome {
        self.contexts
            .lock()
            .expect("contexts")
            .push(context.clone());
        if context[self.field] == self.value {
            CheckOutcome::Deny {
                rule: None,
                reason: format!("policy denies {}", self.value),
            }
        } else {
            CheckOutcome::Allow { rule: None }
        }
    }
}

#[derive(Default)]
struct Harness {
    agents: Option<Arc<dyn AgentProvider>>,
    skills: Option<Arc<dyn SkillProvider>>,
    policy: Option<Arc<dyn SecurityCheck>>,
}

impl Harness {
    fn agents(mut self, agents: Arc<dyn AgentProvider>) -> Self {
        self.agents = Some(agents);
        self
    }

    fn skills(mut self, skills: Arc<dyn SkillProvider>) -> Self {
        self.skills = Some(skills);
        self
    }

    fn policy(mut self, policy: Arc<dyn SecurityCheck>) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Compile `source` under the harness set and run it; `main`'s result.
    async fn run(self, source: &str) -> wasmtime::Result<String> {
        let stdlib = harness_stdlib();
        let parsed = crate::parse_script(source, crate::FileId(0));
        let compiled = crate::compile_parsed_script_timed(
            source,
            "test.ts",
            &parsed,
            &stdlib.package_declarations(),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.agent_provider = self.agents;
        data.skill_provider = self.skills;
        if let Some(policy) = self.policy {
            data.security_check = policy;
        }
        let mut store = cfg.store_async(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async_for(&mut linker, &mut store, stdlib)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .map(Option::unwrap_or_default)
    }
}

fn compile_errors(source: &str, stdlib: Stdlib) -> Vec<String> {
    let parsed = crate::parse_script(source, crate::FileId(0));
    match crate::compile_parsed_script_timed(
        source,
        "test.ts",
        &parsed,
        &stdlib.package_declarations(),
        &[],
        &[],
    ) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|d| format!("{} {}", d.message, d.help.join(" ")))
            .collect(),
    }
}

#[test]
fn a_disabled_package_is_not_available_in_this_harness() {
    for module in ["submilli:agents", "submilli:skills"] {
        let source =
            format!("import {{ list }} from \"{module}\";\nfunction main(): void {{ list(); }}\n");
        let errors = compile_errors(&source, Stdlib::core());
        assert!(
            errors
                .iter()
                .any(|e| e.contains(&format!("`{module}` is not available in this harness"))),
            "{errors:?}"
        );
        let parsed = crate::parse_script(&source, crate::FileId(0));
        let imports = parsed.external_imports().expect("imports");
        assert!(
            imports.stdlib.contains(module),
            "{module} is stdlib, not a registry package"
        );
        assert!(imports.registry_packages.is_empty());
    }
}

#[test]
fn only_an_enabling_set_discovers_and_catalogs_the_packages() {
    let core_modules: Vec<String> = crate::packages::search("")
        .into_iter()
        .map(|m| m.name)
        .collect();
    let harness_modules: Vec<String> = harness_stdlib()
        .search("")
        .into_iter()
        .map(|m| m.name)
        .collect();
    for module in ["submilli:agents", "submilli:skills"] {
        assert!(
            !core_modules.iter().any(|m| m == module),
            "{core_modules:?}"
        );
        assert!(
            harness_modules.iter().any(|m| m == module),
            "{harness_modules:?}"
        );
        assert!(crate::packages::docs(module).is_none());
        assert!(harness_stdlib().docs(module).is_some());
    }
    for capability in ["agent.run", "skill.load"] {
        assert!(capabilities::find(capability).is_none());
        assert!(capabilities::find_for(harness_stdlib(), capability).is_some());
    }
    let agents_only = Stdlib::core().with(OptionalPackage::Agents);
    assert!(capabilities::find_for(agents_only, "agent.run").is_some());
    assert!(capabilities::find_for(agents_only, "skill.load").is_none());
}

#[test]
fn a_written_schema_argument_is_refused() {
    let errors = compile_errors(
        "import agents from \"submilli:agents\";\n\
         function main(): void { agents.run(\"researcher\", \"go\", \"{}\"); }\n",
        harness_stdlib(),
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("takes the agent and the input")),
        "{errors:?}"
    );
}

#[tokio::test]
async fn an_untyped_run_returns_the_agents_text() {
    let agents = FakeAgents::answering("found three sources");
    let out = Harness::default()
        .agents(agents.clone())
        .run(
            r#"import agents from "submilli:agents";
               function main(): string { return agents.run("researcher", "find sources"); }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "found three sources");
    assert_eq!(
        agents.requests(),
        [AgentRequest {
            agent: "researcher".to_string(),
            input: "find sources".to_string(),
            schema: None,
            caller: "main".to_string(),
        }]
    );
}

#[tokio::test]
async fn a_typed_run_sends_a_schema_and_checks_the_answer() {
    let agents = FakeAgents::answering(r#"{"score": 7, "summary": "fine"}"#);
    let out = Harness::default()
        .agents(agents.clone())
        .run(
            r#"import agents from "submilli:agents";
               interface Review { score: number; summary: string }
               function main(): string {
                 const review = agents.run<Review>("researcher", "review it");
                 return review.summary + " " + String(review.score);
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "fine 7");
    let schema = agents.requests()[0].schema.clone().expect("a schema");
    assert!(schema.contains("\"score\""), "{schema}");
}

#[tokio::test]
async fn a_typed_run_throws_when_the_answer_does_not_match() {
    let out = Harness::default()
        .agents(FakeAgents::answering(r#"{"score": "high"}"#))
        .run(
            r#"import agents from "submilli:agents";
               interface Review { score: number }
               function main(): string {
                 try {
                   agents.run<Review>("researcher", "review it");
                   return "accepted";
                 } catch (e: TypeError) {
                   return "rejected";
                 }
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "rejected");
}

#[tokio::test]
async fn a_denied_run_never_reaches_the_harness() {
    let agents = FakeAgents::answering("deployed");
    let policy = DenyOne::new("agent", "deployer");
    let out = Harness::default()
        .agents(agents.clone())
        .policy(policy.clone())
        .run(
            r#"import agents from "submilli:agents";
               function main(): string {
                 try {
                   agents.run("deployer", "ship it");
                   return "ran";
                 } catch (e: PermissionDeniedError) {
                   return "denied";
                 }
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "denied");
    assert!(agents.requests().is_empty());
    let contexts = policy.contexts.lock().expect("contexts").clone();
    assert_eq!(contexts, [serde_json::json!({ "agent": "deployer" })]);
}

#[tokio::test]
async fn list_leaves_out_the_agents_the_caller_may_not_run() {
    let out = Harness::default()
        .agents(FakeAgents::answering(""))
        .policy(DenyOne::new("agent", "deployer"))
        .run(
            r#"import agents from "submilli:agents";
               function main(): string {
                 return agents.list().map((a) => a.name + ":" + (a.description ?? "-")).join(",");
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "researcher:Finds sources");
}

#[tokio::test]
async fn harness_failures_and_a_missing_provider_are_catchable() {
    let source = r#"import agents from "submilli:agents";
        function main(): string {
          try { agents.run("researcher", "go"); return "ran"; }
          catch (e) { return e.message; }
        }"#;
    let failed = Harness::default()
        .agents(FakeAgents::failing(AgentCallError::Failed {
            agent: "researcher".to_string(),
            message: "step limit reached".to_string(),
        }))
        .run(source)
        .await
        .expect("program completes");
    assert_eq!(
        failed,
        "agents.run: agent `researcher` failed: step limit reached"
    );

    let unconfigured = Harness::default()
        .run(source)
        .await
        .expect("program completes");
    assert!(
        unconfigured.contains("no agent provider is configured"),
        "{unconfigured}"
    );
}

#[tokio::test]
async fn skills_list_load_and_read_files() {
    let skills = FakeSkills::new();
    let out = Harness::default()
        .skills(skills.clone())
        .policy(DenyOne::new("name", "internal"))
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 const names = skills.list().map((s) => s.name).join(",");
                 const review = skills.load("review");
                 const report = skills.readFile("review", "templates/report.md");
                 return names + "|" + review.content + "|" + report;
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "review|Read the diff twice.|# Report");
}

#[tokio::test]
async fn read_file_is_gated_by_the_skill_and_refuses_paths_outside_it() {
    let skills = FakeSkills::new();
    let out = Harness::default()
        .skills(skills.clone())
        .policy(DenyOne::new("name", "internal"))
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 let out = "";
                 try { skills.readFile("internal", "SKILL.md"); out += "read "; }
                 catch (e: PermissionDeniedError) { out += "denied "; }
                 try { skills.readFile("review", "../secrets"); out += "escaped"; }
                 catch (e: RangeError) { out += "range"; }
                 return out;
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "denied range");
    assert!(
        skills.reads.lock().expect("reads").is_empty(),
        "neither call may reach the harness"
    );
}

#[test]
fn an_unknown_type_argument_is_refused_in_agents_words() {
    let errors = compile_errors(
        "import agents from \"submilli:agents\";\n\
         function main(): void { agents.run<unknown>(\"researcher\", \"go\"); }\n",
        harness_stdlib(),
    );
    assert!(
        errors.iter().any(
            |e| e.contains("`agents.run<unknown>` would not verify anything")
                && e.contains("agents.run<Report>(agent, input)")
        ),
        "{errors:?}"
    );
}

#[test]
fn a_parsed_script_typechecks_against_an_enabling_set() {
    let source = "import { list } from \"submilli:skills\";\nfunction main(): void { list(); }\n";
    let parsed = crate::parse_script(source, crate::FileId(0));
    crate::compile::typecheck_parsed_checked(
        source,
        &parsed,
        &harness_stdlib().package_declarations(),
    )
    .expect("skills is available");
    assert!(
        crate::compile::typecheck_parsed_checked(
            source,
            &parsed,
            &Stdlib::core().package_declarations()
        )
        .is_err()
    );
}

#[test]
fn a_package_compiles_against_an_enabling_set() {
    let source = "import { list } from \"submilli:agents\";\n\
                  export function names(): string[] { return list().map((a) => a.name); }\n";
    let modules = [crate::PackageSourceModule {
        path: crate::ModulePath::from("lib"),
        source,
    }];
    crate::compile::compile_package_with_transitive_checked_for(
        harness_stdlib(),
        "@acme/fanout",
        crate::ModulePath::from("lib"),
        &modules,
        &[],
        &[],
    )
    .expect("agents is importable");
    assert!(
        crate::compile::compile_package_with_transitive_checked(
            "@acme/fanout",
            crate::ModulePath::from("lib"),
            &modules,
            &[],
            &[],
        )
        .is_err()
    );
}

#[tokio::test]
async fn a_typed_run_with_a_prose_answer_throws_a_syntax_error() {
    let out = Harness::default()
        .agents(FakeAgents::answering("Sure, here is the report"))
        .run(
            r#"import agents from "submilli:agents";
               interface Review { score: number }
               function main(): string {
                 try {
                   agents.run<Review>("researcher", "review it");
                   return "accepted";
                 } catch (e: SyntaxError) {
                   return "not json";
                 }
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "not json");
}

#[tokio::test]
async fn an_empty_or_failed_listing_is_what_the_program_sees() {
    let empty = Harness::default()
        .agents(FakeAgents::listing(Ok(Vec::new())))
        .run(
            r#"import agents from "submilli:agents";
               function main(): string { return String(agents.list().length); }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(empty, "0");

    let failed = Harness::default()
        .agents(FakeAgents::listing(Err(AgentCallError::Failed {
            agent: String::new(),
            message: "catalog unavailable".to_string(),
        })))
        .run(
            r#"import agents from "submilli:agents";
               function main(): string {
                 try { agents.list(); return "listed"; } catch (e) { return e.message; }
               }"#,
        )
        .await
        .expect("program completes");
    assert!(failed.contains("catalog unavailable"), "{failed}");
}

#[tokio::test]
async fn a_skill_name_that_is_not_one_segment_never_reaches_the_harness() {
    let skills = FakeSkills::new();
    let out = Harness::default()
        .skills(skills.clone())
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 let out = "";
                 for (const name of ["../other", "a/b", "", ".."]) {
                   try { skills.load(name); out += "loaded "; }
                   catch (e: RangeError) { out += "range "; }
                 }
                 try { skills.readFile("../../etc", "passwd"); out += "read"; }
                 catch (e: RangeError) { out += "range"; }
                 return out;
               }"#,
        )
        .await
        .expect("program completes");
    assert_eq!(out, "range range range range range");
    assert!(skills.loads.lock().expect("loads").is_empty());
    assert!(skills.reads.lock().expect("reads").is_empty());
}

#[tokio::test]
async fn an_unknown_skill_and_a_missing_provider_are_catchable() {
    let source = r#"import skills from "submilli:skills";
        function main(): string {
          try { skills.load("nope"); return "loaded"; } catch (e) { return e.message; }
        }"#;
    let unknown = Harness::default()
        .skills(FakeSkills::new())
        .run(source)
        .await
        .expect("program completes");
    assert_eq!(
        unknown,
        "skills.load: no skill named `nope`; call `list()` for the skills you may load"
    );

    let unconfigured = Harness::default()
        .run(source)
        .await
        .expect("program completes");
    assert!(
        unconfigured.contains("no skill provider is configured"),
        "{unconfigured}"
    );
}

#[tokio::test]
async fn a_namespaced_skill_name_reaches_the_harness() {
    let skills = FakeSkills::new();
    let out = Harness::default()
        .skills(skills.clone())
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 try { skills.load("plugin:review"); return "loaded"; } catch (e) { return e.message; }
               }"#,
        )
        .await
        .expect("program completes");
    assert!(out.contains("no skill named `plugin:review`"), "{out}");
    assert_eq!(*skills.loads.lock().expect("loads"), ["plugin:review"]);
}

/// Answers every load with the `review` skill, whatever was asked for.
struct CaseFoldingSkills;

impl SkillProvider for CaseFoldingSkills {
    fn list<'a>(&'a self) -> Boxed<'a, Result<Vec<SkillInfo>, SkillError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn load<'a>(&'a self, _name: &'a str) -> Boxed<'a, Result<Skill, SkillError>> {
        Box::pin(async {
            Ok(Skill {
                name: "review".to_string(),
                description: None,
                content: "Read the diff twice.".to_string(),
            })
        })
    }

    fn read_file<'a>(
        &'a self,
        _name: &'a str,
        _path: &'a str,
    ) -> Boxed<'a, Result<String, SkillError>> {
        Box::pin(async { Ok(String::new()) })
    }
}

/// The policy decided on the name the program wrote, so a skill answered under
/// another name is refused rather than handed over.
#[tokio::test]
async fn a_skill_answered_under_another_name_is_refused() {
    let out = Harness::default()
        .skills(Arc::new(CaseFoldingSkills))
        .policy(DenyOne::new("name", "review"))
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 try { return skills.load("Review").content; } catch (e) { return e.message; }
               }"#,
        )
        .await
        .expect("program completes");
    assert!(out.contains("with a different skill"), "{out}");
}

#[tokio::test]
async fn an_oversized_name_is_refused_with_a_short_message() {
    let out = Harness::default()
        .skills(FakeSkills::new())
        .run(
            r#"import skills from "submilli:skills";
               function main(): string {
                 try { skills.load("x".repeat(5000)); return "loaded"; }
                 catch (e: RangeError) { return e.message; }
               }"#,
        )
        .await
        .expect("program completes");
    assert!(out.contains("at most 4096 bytes"), "{out}");
    assert!(out.len() < 600, "{}", out.len());
}
