//! The embedder-supplied seam for `@tools`: a program calls one of the
//! application's own tools, and the harness that declared it answers.
//!
//! Every `@tools.<tool>(...)` call lowers (in codegen) to **one** host import,
//! `submilli:tools.call(tool, argsJson) -> unknown`, exactly as `@mcp/<server>`
//! calls lower to `submilli:mcp.call`. The host fn runs the `tools.call`
//! capability check before anything reaches the harness, then awaits the
//! embedder's [`ToolProvider`]. Normal runtime casts validate declared return
//! types.
//!
//! **Waiting is not running.** The provider may take as long as the harness
//! needs, for example while a person approves the call. The time spent waiting
//! is added to [`StoreData::clock_paused`], which an embedder's execution
//! timeout subtracts, so a program's time limit counts its own work only. The
//! provider bounds the wait itself. Dropping the future returned by
//! [`ToolProvider::call`] is how the engine cancels it.

use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::call_log::{Payload, Side, record_payload};
use crate::runtime::fuel;
use crate::runtime::host::{intrinsic_string_type, read_string_arg, register_host_fn_async};
use crate::runtime::mcp::McpResponse;
use crate::stdlib::shared::{audit_denial_in, check_security_call, running_package};
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

/// The internal host module every `@tools` call dispatches through.
pub const TOOLS_MODULE_NAME: &str = "submilli:tools";
/// The source-level name of the virtual package.
pub const TOOLS_PACKAGE: &str = "@tools";
/// The capability every call is checked against, with the tool in its `tool` field.
pub const TOOLS_CAPABILITY: &str = "tools.call";

/// Why the harness did not return a value. Each maps to a catchable guest error,
/// except [`ToolCallError::Cancelled`], which ends the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallError {
    /// The tool ran and failed. Thrown as `Error(message)`.
    Failed { message: String },
    /// The harness, or a person it asked, refused the call. Thrown as
    /// `PermissionDeniedError`, reported with source `harness`.
    Denied { message: String },
    /// The harness did not answer in time. Thrown as `TimeoutError`.
    TimedOut,
    /// The embedder could not hold another waiting call. Thrown as
    /// `QuotaExceededError`.
    QuotaExceeded { message: String },
    /// The wait was abandoned, as when the session ends. The run ends with it.
    Cancelled,
}

impl ToolCallError {
    /// The failure as the call log keeps it: a stable kind and a message.
    fn record(&self) -> serde_json::Value {
        let (kind, message) = match self {
            ToolCallError::Failed { message } => ("failed", message.as_str()),
            ToolCallError::Denied { message } => ("denied", message.as_str()),
            ToolCallError::TimedOut => ("timeout", ""),
            ToolCallError::QuotaExceeded { message } => ("quota", message.as_str()),
            ToolCallError::Cancelled => ("cancelled", ""),
        };
        serde_json::json!({ "kind": kind, "message": message })
    }
}

/// The embedder-provided answerer for `@tools` calls. The value is a bounded
/// JSON tree ([`McpResponse::take`] admits it), so guest conversion stays
/// bounded whoever produced it.
///
/// Object-safe boxed-future trait (rather than `async fn`), like
/// [`McpTransport`](crate::runtime::McpTransport), so a harness may answer in
/// process or forward the call over the wire.
pub trait ToolProvider: Send + Sync {
    fn call<'a>(
        &'a self,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<McpResponse, ToolCallError>> + Send + 'a>>;
}

fn tools_call_func_type(linker: &Linker<StoreData>) -> wasmtime::Result<FuncType> {
    let engine = linker.engine();
    let string_struct = intrinsic_string_type(engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(string_struct)));
    let object = crate::runtime::intrinsic_types::build_intrinsic_types(engine)?.object;
    let unknown = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(object)));
    Ok(FuncType::new(engine, [string.clone(), string], [unknown]))
}

/// Install the async `submilli:tools.call` host fn. Reads `(tool, argsJson)`,
/// runs the capability check, awaits [`StoreData::tool_provider`] with the
/// clock paused, and builds guest values directly from the returned tree.
pub fn install_tools_async(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let ty = tools_call_func_type(linker)?;
    register_host_fn_async(
        linker,
        TOOLS_MODULE_NAME,
        crate::mangle::host(TOOLS_MODULE_NAME, "call"),
        ty,
        move |caller, params, results| {
            Box::pin(async move {
                let params: &[Val; 2] = params.try_into().map_err(|_| {
                    crate::runtime::host::fatal_host_error("tools argument count is invalid")
                })?;
                let result_slot = results.first_mut().ok_or_else(|| {
                    crate::runtime::host::fatal_host_error("tools result slot is missing")
                })?;
                let tool = read_string_arg(&mut *caller, &params[0], "tools.call (tool)")?;
                let args_json = read_string_arg(&mut *caller, &params[1], "tools.call (args)")?;

                // Deny before the harness hears of the call. The tool is in the
                // filter context so a policy can choose which tools a program reaches.
                let ticket = check_security_call(
                    &mut *caller,
                    TOOLS_CAPABILITY,
                    serde_json::json!({ "tool": tool }),
                )?;
                record_payload(&*caller, ticket, Side::Request, || {
                    Payload::meta(serde_json::json!({ "tool": tool }))
                        .with_body(args_json.as_bytes())
                });
                fuel::charge(&mut *caller, fuel::IO, args_json.len() as u64)?;
                let request = ToolRequest { tool, args_json };
                let response = super::effects::execute(
                    caller,
                    "tools.call",
                    request,
                    move |caller, _, request| {
                        Box::pin(async move {
                            let ToolRequest { tool, args_json } = request;
                            // Looked up after the gate, so a denied caller cannot learn
                            // whether the session offers tools.
                            let provider = caller.data().tool_provider.clone().ok_or_else(|| {
                    wasmtime::Error::msg(format!(
                        "{TOOLS_PACKAGE}.{tool}: this session offers no application tools"
                    ))
                })?;
                            let principal = running_package(&*caller)
                                .map_err(|error| error.into_denial(TOOLS_CAPABILITY))?;

                            let waited = Instant::now();
                            let outcome = provider.call(&tool, &args_json).await;
                            let data = caller.data_mut();
                            data.clock_paused = data.clock_paused.saturating_add(waited.elapsed());

                            // A refusal by the harness is a denial in the audit trail, inside the
                            // call the policy allowed.
                            if let Err(ToolCallError::Denied { .. }) = &outcome {
                                audit_denial_in(
                                    &*caller,
                                    ticket,
                                    &principal,
                                    TOOLS_CAPABILITY,
                                    &serde_json::json!({ "tool": tool }),
                                    crate::runtime::host::DenialSource::Harness.as_str(),
                                    "the application refused the call",
                                );
                            }
                            record_payload(&*caller, ticket, Side::Response, || match &outcome {
                                Ok(response) => Payload::meta(serde_json::Value::Null)
                                    .with_owned_body(
                                        serde_json::to_vec(response.value()).unwrap_or_default(),
                                    ),
                                Err(error) => Payload::meta(error.record()),
                            });
                            outcome.map_err(|error| tool_error_to_throw(&principal, &tool, error))
                        })
                    },
                )
                .await?;
                *result_slot = super::mcp::allocate_response(caller, &response)?;
                Ok(())
            })
        },
    )
}

#[derive(serde::Serialize)]
struct ToolRequest {
    tool: String,
    args_json: String,
}

fn tool_error_to_throw(principal: &str, tool: &str, error: ToolCallError) -> wasmtime::Error {
    match error {
        ToolCallError::Failed { message } => {
            wasmtime::Error::msg(format!("{TOOLS_PACKAGE}.{tool}: {message}"))
        }
        ToolCallError::Denied { message } => crate::runtime::host::permission_denied_by_harness(
            principal,
            TOOLS_CAPABILITY,
            format!("{TOOLS_PACKAGE}.{tool}: {message}"),
        ),
        ToolCallError::TimedOut => crate::runtime::host::timeout_error(format!(
            "{TOOLS_PACKAGE}.{tool}: the application did not answer in time"
        )),
        ToolCallError::QuotaExceeded { message } => {
            crate::runtime::host::quota_exceeded_error(format!("{TOOLS_PACKAGE}.{tool}: {message}"))
        }
        ToolCallError::Cancelled => crate::runtime::host::fatal_host_error(format!(
            "{TOOLS_PACKAGE}.{tool}: the call was cancelled"
        )),
    }
}

/// Codegen-facing definitions for the internal `submilli:tools.call` host fn:
/// `call(tool: string, args: string): unknown`.
pub fn tools_call_package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(TOOLS_MODULE_NAME);
    defs.values.insert(
        "call".to_string(),
        ValueSymbol {
            name: "call".to_string(),
            mangled_name: crate::mangle::host(TOOLS_MODULE_NAME, "call"),
            declaration_span: Span::at(crate::FileId::MCP),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new("tool", Type::String),
                    Param::new("args", Type::String),
                ],
                ret: Type::Unknown,
                type_predicate: None,
                doc: None,
            },
        },
    );
    defs
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use wasmtime::{Linker, Module};

    use super::{ToolCallError, ToolProvider};
    use crate::runtime::McpResponse;
    use crate::runtime::host::DenialSource;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        install_tenant_limits,
    };
    use crate::{
        ObjectField, PackageDeclaration, Param, Shape, Span, Type, ValueKind, ValueSymbol,
        VirtualDispatch, compile_script, mangle,
    };

    /// `@tools` with `echo(args: { msg: string }): { reply: string }` and
    /// `raw(): unknown`, shaped as the catalog mapper builds them.
    fn tools_package() -> PackageDeclaration {
        let pkg = super::TOOLS_PACKAGE;
        let mut defs = PackageDeclaration::with_package(pkg);
        defs.virtual_dispatch = Some(VirtualDispatch::Tools);
        let object = |field: &str| Type::Object {
            index: None,
            fields: BTreeMap::from([(field.to_string(), ObjectField::required(Type::String))]),
        };
        let (args, ret) = (object("msg"), object("reply"));
        defs.shapes.push(Shape::from_type(&args).unwrap());
        defs.shapes.push(Shape::from_type(&ret).unwrap());
        let mut insert = |name: &str, params: Vec<Param>, ret: Type| {
            defs.values.insert(
                name.to_string(),
                ValueSymbol {
                    name: name.to_string(),
                    mangled_name: mangle::host(pkg, name),
                    declaration_span: Span::at(crate::FileId::MCP),
                    kind: ValueKind::Function {
                        generics: Vec::new(),
                        params,
                        ret,
                        type_predicate: None,
                        doc: None,
                    },
                },
            );
        };
        insert("echo", vec![Param::new("args", args)], ret);
        insert("raw", Vec::new(), Type::Unknown);
        defs
    }

    type Calls = Arc<Mutex<Vec<(String, String)>>>;

    /// Records every call, waits `delay`, then answers `answer`.
    struct Answer {
        answer: Result<serde_json::Value, ToolCallError>,
        delay: Duration,
        calls: Calls,
    }

    impl ToolProvider for Answer {
        fn call<'a>(
            &'a self,
            tool: &'a str,
            args_json: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<McpResponse, ToolCallError>> + Send + 'a>,
        > {
            self.calls
                .lock()
                .unwrap()
                .push((tool.to_string(), args_json.to_string()));
            Box::pin(async move {
                tokio::time::sleep(self.delay).await;
                let mut value = self.answer.clone()?;
                Ok(McpResponse::take(&mut value).expect("bounded"))
            })
        }
    }

    struct DenyAll;
    impl SecurityCheck for DenyAll {
        fn check(&self, _: &str, capability: &str, _: &serde_json::Value) -> CheckOutcome {
            CheckOutcome::Deny {
                rule: None,
                reason: format!("denied {capability}"),
            }
        }
    }

    struct Run {
        result: wasmtime::Result<Option<String>>,
        calls: Calls,
        clock_paused: Duration,
    }

    async fn run_with(
        source: &str,
        answer: Result<serde_json::Value, ToolCallError>,
        delay: Duration,
        security: Option<Arc<dyn SecurityCheck>>,
        provided: bool,
    ) -> Run {
        let pkg = tools_package();
        let compiled =
            compile_script(source, "<test>", crate::FileId(0), &[], &[&pkg]).expect("compiles");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let calls = Calls::default();
        let mut data = StoreData::with_vfs(Vfs::none());
        data.install_type_info(compiled.type_info.clone());
        if provided {
            data.tool_provider = Some(Arc::new(Answer {
                answer,
                delay,
                calls: calls.clone(),
            }));
        }
        if let Some(check) = security {
            data.security_check = check;
        }
        let mut store = cfg.store_async(&engine, data).unwrap();
        install_tenant_limits(&mut store);
        let module = Module::new(&engine, &compiled.wasm).unwrap();
        let mut linker = Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let result = dispatch_main_async(&mut store, &instance).await;
        Run {
            result,
            calls,
            clock_paused: store.data().clock_paused,
        }
    }

    async fn run(source: &str, answer: Result<serde_json::Value, ToolCallError>) -> Run {
        run_with(source, answer, Duration::ZERO, None, true).await
    }

    fn escaped_source(error: &wasmtime::Error) -> Option<DenialSource> {
        error
            .downcast_ref::<crate::backtrace::ThrownError>()
            .and_then(|thrown| thrown.denial.as_ref())
            .map(|denial| denial.source)
    }

    #[tokio::test]
    async fn typed_tool_round_trips_through_the_provider() {
        let run = run(
            r#"import tools from "@tools";
               function main(): string { return tools.echo({ msg: "hi" }).reply; }"#,
            Ok(serde_json::json!({ "reply": "hello" })),
        )
        .await;
        assert_eq!(run.result.unwrap().as_deref(), Some("hello"));
        assert_eq!(
            *run.calls.lock().unwrap(),
            [("echo".to_string(), r#"{"msg":"hi"}"#.to_string())]
        );
    }

    #[tokio::test]
    async fn a_zero_argument_tool_sends_an_empty_object_and_returns_unknown() {
        let run = run(
            r#"import { raw } from "@tools";
               function main(): string { return JSON.stringify(raw()) ?? ""; }"#,
            Ok(serde_json::json!([1, "two"])),
        )
        .await;
        assert_eq!(run.result.unwrap().as_deref(), Some(r#"[1,"two"]"#));
        assert_eq!(run.calls.lock().unwrap()[0].1, "{}");
    }

    #[tokio::test]
    async fn a_value_that_does_not_match_the_declared_type_throws_type_error() {
        let run = run(
            r#"import tools from "@tools";
               function main(): string {
                   try { return tools.echo({ msg: "hi" }).reply; }
                   catch (e) { return e instanceof TypeError ? "type" : "other"; }
               }"#,
            Ok(serde_json::json!({ "reply": 5 })),
        )
        .await;
        assert_eq!(run.result.unwrap().as_deref(), Some("type"));
    }

    #[tokio::test]
    async fn a_policy_denial_throws_before_the_harness_hears_of_the_call() {
        let run = run_with(
            r#"import tools from "@tools";
               function main(): void { tools.raw(); }"#,
            Ok(serde_json::Value::Null),
            Duration::ZERO,
            Some(Arc::new(DenyAll)),
            true,
        )
        .await;
        let error = run.result.unwrap_err();
        assert_eq!(escaped_source(&error), Some(DenialSource::Policy));
        assert!(run.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn each_harness_answer_throws_its_own_error_class() {
        let cases = [
            (
                ToolCallError::Failed {
                    message: "no such order".into(),
                },
                "e instanceof Error && e.message == \"@tools.raw: no such order\"",
            ),
            (
                ToolCallError::Denied {
                    message: "refused".into(),
                },
                "e instanceof PermissionDeniedError",
            ),
            (ToolCallError::TimedOut, "e instanceof TimeoutError"),
            (
                ToolCallError::QuotaExceeded {
                    message: "full".into(),
                },
                "e instanceof QuotaExceededError",
            ),
        ];
        for (answer, check) in cases {
            let source = format!(
                r#"import tools from "@tools";
                   function main(): string {{
                       try {{ tools.raw(); return "returned"; }}
                       catch (e) {{ return {check} ? "caught" : "wrong"; }}
                   }}"#
            );
            let run = run(&source, Err(answer.clone())).await;
            assert_eq!(run.result.unwrap().as_deref(), Some("caught"), "{answer:?}");
        }
    }

    #[tokio::test]
    async fn an_escaped_harness_refusal_reports_source_harness() {
        let run = run(
            r#"import tools from "@tools";
               function main(): void { tools.raw(); }"#,
            Err(ToolCallError::Denied {
                message: "a person said no".into(),
            }),
        )
        .await;
        let error = run.result.unwrap_err();
        assert_eq!(escaped_source(&error), Some(DenialSource::Harness));
    }

    #[tokio::test]
    async fn a_cancelled_call_ends_the_run_uncaught() {
        let run = run(
            r#"import tools from "@tools";
               function main(): string { try { tools.raw(); } catch (e) { return "caught"; } return "returned"; }"#,
            Err(ToolCallError::Cancelled),
        )
        .await;
        let error = run.result.unwrap_err();
        assert!(
            error.is::<crate::runtime::host::FatalHostError>(),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn without_a_provider_the_call_throws_a_catchable_error() {
        let run = run_with(
            r#"import tools from "@tools";
               function main(): string { try { tools.raw(); } catch (e) { return e.message; } return ""; }"#,
            Ok(serde_json::Value::Null),
            Duration::ZERO,
            None,
            false,
        )
        .await;
        assert_eq!(
            run.result.unwrap().as_deref(),
            Some("@tools.raw: this session offers no application tools")
        );
    }

    #[tokio::test]
    async fn time_spent_waiting_for_the_harness_is_recorded_as_paused() {
        let run = run_with(
            r#"import tools from "@tools";
               function main(): void { tools.raw(); tools.raw(); }"#,
            Ok(serde_json::Value::Null),
            Duration::from_millis(30),
            None,
            true,
        )
        .await;
        run.result.unwrap();
        assert!(
            run.clock_paused >= Duration::from_millis(60),
            "{:?}",
            run.clock_paused
        );
    }

    #[test]
    fn importing_tools_without_a_catalog_is_a_compile_error() {
        let error = compile_script(
            r#"import tools from "@tools"; function main(): void {}"#,
            "<test>",
            crate::FileId(0),
            &[],
            &[],
        )
        .unwrap_err();
        assert!(
            format!("{error:?}").contains("`@tools` is unavailable"),
            "{error:?}"
        );
    }

    #[test]
    fn the_import_is_reported_as_tools() {
        let parsed = crate::compile::parse_script(
            r#"import tools from "@tools"; import x from "@mcp/x"; function main(): void {}"#,
            crate::FileId(0),
        );
        let imports = parsed.external_imports().unwrap();
        assert!(imports.tools);
        assert!(imports.registry_packages.is_empty());
        assert_eq!(imports.mcp_servers.len(), 1);
    }
}
