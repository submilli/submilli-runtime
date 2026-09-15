//! Outbound `@mcp/<server>` transport dispatch.
//!
//! Every `@mcp/<server>.<tool>(...)` call lowers (in codegen) to **one** host
//! import — `submilli:mcp.call(server, tool, argsJson) -> string` — rather than a
//! per-tool import. Codegen passes the server and tool names as constants and the
//! args serialized to JSON, then turns the returned JSON text into the declared
//! return type (a `string`, or a typed value via the `JSON.parse` validator).
//!
//! This module provides that host fn. It runs the `mcp.<server>` capability check
//! (one per server, with the tool in the filter context) *before* dispatch (a
//! denial throws before any network bytes leave), then
//! calls the embedder-provided [`McpTransport`] — the actual JSON-RPC `tools/call`,
//! auth injection, and refresh-on-`401` live in the embedder (submilli-server),
//! keeping this crate free of its async/HTTP dependencies.

use std::future::Future;
use std::pin::Pin;

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_string_type, read_string_arg, register_host_fn_async, write_submilli_string_struct,
};
use crate::stdlib::shared::check_security;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

/// The single internal host module every MCP call dispatches through.
pub const MCP_MODULE_NAME: &str = "submilli:mcp";

/// Why an outbound MCP `tools/call` failed. The host fn maps each to a catchable
/// script error (see [`mcp_error_to_throw`]).
#[derive(Debug)]
pub enum McpCallError {
    /// The OAuth refresh token was revoked upstream; the blueprint is demoted to
    /// PENDING. Surfaces to the script as a catchable `McpAuthExpiredError`.
    AuthExpired,
    /// The server reported a tool-level or JSON-RPC error.
    Mcp { message: String },
    /// A non-2xx HTTP response from the server.
    Upstream { status: u16, body: String },
    /// The request never completed (connection / transport failure).
    Transport(String),
}

/// The embedder-provided outbound MCP transport: performs the JSON-RPC
/// `tools/call` (resolving auth, refreshing on `401`, reusing connections) and
/// returns the tool result as a `serde_json::Value`.
///
/// Object-safe boxed-future trait (rather than `async fn`) so the interpreter
/// stays free of the embedder's async stack while still `await`-ing the call from
/// within the async host function.
pub trait McpTransport: Send + Sync {
    fn call<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, McpCallError>> + Send + 'a>>;
}

fn mcp_call_func_type(linker: &Linker<StoreData>) -> wasmtime::Result<FuncType> {
    let engine = linker.engine();
    let string_struct = intrinsic_string_type(engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(string_struct)));
    Ok(FuncType::new(
        engine,
        [string.clone(), string.clone(), string.clone()],
        [string],
    ))
}

/// Install the async `submilli:mcp.call` dispatch host fn. Reads `(server, tool,
/// argsJson)`, runs the capability check, dispatches through
/// [`StoreData::mcp_transport`], and returns the result serialized as JSON text.
pub fn install_mcp_async(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let ty = mcp_call_func_type(linker)?;
    register_host_fn_async(
        linker,
        MCP_MODULE_NAME,
        crate::mangle::host(MCP_MODULE_NAME, "call"),
        ty,
        /* deterministic = */ false,
        move |caller, params, results| {
            Box::pin(async move {
                let server = read_string_arg(&mut *caller, &params[0], "mcp.call (server)")?;
                let tool = read_string_arg(&mut *caller, &params[1], "mcp.call (tool)")?;
                let args_json = read_string_arg(&mut *caller, &params[2], "mcp.call (args)")?;

                // Deny before any network bytes leave. One capability per server
                // (`mcp.<server>`, known when the blueprint is written); the tool is
                // in the filter context so a policy can constrain by tool.
                check_security(
                    &*caller,
                    &format!("mcp.{server}"),
                    serde_json::json!({ "tool": tool, "transport": "streamable_http" }),
                )?;

                let transport = caller.data().mcp_transport.clone().ok_or_else(|| {
                    wasmtime::Error::msg(format!(
                        "@mcp/{server}.{tool}: MCP transport not configured"
                    ))
                })?;

                let value = transport
                    .call(&server, &tool, &args_json)
                    .await
                    .map_err(|err| mcp_error_to_throw(&server, &tool, err))?;

                let text = serde_json::to_string(&value).map_err(|e| {
                    wasmtime::Error::msg(format!(
                        "@mcp/{server}.{tool}: result is not serializable: {e}"
                    ))
                })?;
                let st = write_submilli_string_struct(&mut *caller, &text)?;
                results[0] = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    )
}

/// Codegen-facing definitions for the internal `submilli:mcp.call` host fn:
/// `call(server: string, tool: string, args: string): string`. Lowered as
/// `(ref $string) x3 -> (ref $string)` by the generic host-import path.
pub fn mcp_call_package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MCP_MODULE_NAME);
    defs.values.insert(
        "call".to_string(),
        ValueSymbol {
            name: "call".to_string(),
            mangled_name: crate::mangle::host(MCP_MODULE_NAME, "call"),
            declaration_span: Span::at(crate::FileId::MCP),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new("server", Type::String),
                    Param::new("tool", Type::String),
                    Param::new("args", Type::String),
                ],
                ret: Type::String,
                type_predicate: None,
                doc: None,
            },
        },
    );
    defs
}

/// Map a transport failure to a catchable script `Error`. `McpAuthExpiredError` is
/// a message-prefixed `Error` (typed error classes aren't landed yet — same shape
/// as the permission-denied throw).
fn mcp_error_to_throw(server: &str, tool: &str, err: McpCallError) -> wasmtime::Error {
    let message = match err {
        McpCallError::AuthExpired => format!(
            "McpAuthExpiredError: @mcp/{server}.{tool}: the OAuth credential expired upstream; \
             re-authenticate the blueprint"
        ),
        McpCallError::Mcp { message } => format!("@mcp/{server}.{tool}: {message}"),
        McpCallError::Upstream { status, body } => {
            format!("@mcp/{server}.{tool}: server returned HTTP {status}: {body}")
        }
        McpCallError::Transport(detail) => {
            format!("@mcp/{server}.{tool}: transport error: {detail}")
        }
    };
    wasmtime::Error::msg(message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use wasmtime::{Linker, Module};

    use super::{McpCallError, McpTransport};
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        install_tenant_limits,
    };
    use crate::{
        ObjectField, PackageDeclaration, Param, Shape, Span, Type, ValueKind, ValueSymbol,
        compile_script, mangle,
    };

    /// One `@mcp/test` package: `echo(args: { msg: string }): { reply: string }`
    /// (a typed round-trip) plus `raw(): unknown` (the untyped fallback). Mirrors
    /// what the catalog mapper produces — structural object types, with their
    /// shapes registered so codegen emits the object machinery.
    fn test_mcp_package() -> PackageDeclaration {
        let pkg = "@mcp/test";
        let mut defs = PackageDeclaration::with_package(pkg);
        defs.mcp_server = Some("test".to_string());

        let mut arg_fields = BTreeMap::new();
        arg_fields.insert("msg".to_string(), ObjectField::required(Type::String));
        let arg_obj = Type::Object { fields: arg_fields };
        let mut ret_fields = BTreeMap::new();
        ret_fields.insert("reply".to_string(), ObjectField::required(Type::String));
        let ret_obj = Type::Object { fields: ret_fields };
        let mut opt_fields = BTreeMap::new();
        opt_fields.insert("user".to_string(), ObjectField::optional(Type::String));
        let opt_arg_obj = Type::Object { fields: opt_fields };
        defs.shapes.push(Shape::from_type(&arg_obj).unwrap());
        defs.shapes.push(Shape::from_type(&ret_obj).unwrap());
        defs.shapes.push(Shape::from_type(&opt_arg_obj).unwrap());

        let mut insert_fn = |name: &str, params: Vec<Param>, ret: Type| {
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
        insert_fn("echo", vec![Param::new("args", arg_obj)], ret_obj);
        insert_fn("raw", Vec::new(), Type::Unknown);
        // All-optional args: callable with zero args, the default fills in `{}`.
        insert_fn(
            "getTeams",
            vec![Param::with_default(
                "args",
                opt_arg_obj,
                crate::DefaultValue::EmptyObject,
            )],
            Type::Unknown,
        );
        defs
    }

    /// Records every call and returns a canned result.
    struct MockTransport {
        result: serde_json::Value,
        calls: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    impl McpTransport for MockTransport {
        fn call<'a>(
            &'a self,
            server: &'a str,
            tool: &'a str,
            args_json: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<serde_json::Value, McpCallError>>
                    + Send
                    + 'a,
            >,
        > {
            self.calls.lock().unwrap().push((
                server.to_string(),
                tool.to_string(),
                args_json.to_string(),
            ));
            let result = self.result.clone();
            Box::pin(async move { Ok(result) })
        }
    }

    struct DenyAll;
    impl SecurityCheck for DenyAll {
        fn check(&self, _: &str, capability: &str, _: &serde_json::Value) -> CheckOutcome {
            CheckOutcome::Deny {
                reason: format!("denied {capability}"),
            }
        }
    }

    async fn run(
        source: &str,
        result: serde_json::Value,
        security: Option<Arc<dyn SecurityCheck>>,
    ) -> (
        wasmtime::Result<Option<String>>,
        Arc<Mutex<Vec<(String, String, String)>>>,
    ) {
        let pkg = test_mcp_package();
        let compiled =
            compile_script(source, "<test>", crate::FileId(0), &[], &[&pkg]).expect("compiles");

        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut data = StoreData::with_vfs(Vfs::tempdir().unwrap());
        data.install_type_info(compiled.type_info.clone());
        data.mcp_transport = Some(Arc::new(MockTransport {
            result,
            calls: calls.clone(),
        }));
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
        let inst = linker.instantiate_async(&mut store, &module).await.unwrap();
        let out = dispatch_main_async(&mut store, &inst).await;
        (out, calls)
    }

    #[tokio::test]
    async fn typed_tool_round_trips_and_serializes_args() {
        let (out, calls) = run(
            r#"import test from "@mcp/test";
               function main(): void {
                 const r = test.echo({ msg: "hi" });
                 assert(r.reply === "yo", "reply");
               }"#,
            serde_json::json!({ "reply": "yo" }),
            None,
        )
        .await;
        out.expect("typed MCP call must succeed");
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test");
        assert_eq!(calls[0].1, "echo");
        assert!(
            calls[0].2.contains("\"msg\":\"hi\""),
            "args serialized to JSON: {}",
            calls[0].2
        );
    }

    #[tokio::test]
    async fn all_optional_tool_callable_with_zero_args_serializes_empty_object() {
        // The `args` param has a `{}` default, so the zero-arg call type-checks; the
        // synthesized empty object serializes to `{}` (absent optional field skipped).
        let (out, calls) = run(
            r#"import test from "@mcp/test";
               function main(): string {
                 return test.getTeams() as string;
               }"#,
            serde_json::json!("[]"),
            None,
        )
        .await;
        out.expect("zero-arg MCP call must succeed");
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "getTeams");
        assert_eq!(calls[0].2, "{}", "empty optional args serialize to {{}}");
    }

    #[tokio::test]
    async fn named_import_also_routes_to_mcp_call() {
        // `import { echo } from "@mcp/test"; echo(...)` resolves through the
        // bare-identifier call path, not namespace member access — it must still
        // become an McpCall (else codegen panics on an unknown top-level fn).
        let (out, calls) = run(
            r#"import { echo } from "@mcp/test";
               function main(): void {
                 const r = echo({ msg: "hi" });
                 assert(r.reply === "yo", "reply");
               }"#,
            serde_json::json!({ "reply": "yo" }),
            None,
        )
        .await;
        out.expect("named-import MCP call must succeed");
        assert_eq!(calls.lock().unwrap()[0].1, "echo");
    }

    #[tokio::test]
    async fn untyped_tool_returns_unknown_json_value() {
        let (out, _) = run(
            r#"import test from "@mcp/test";
               interface RawResult { k: number; }
               function main(): void {
                 const r = test.raw() as RawResult;
                 assert(r.k === 1, "raw");
               }"#,
            serde_json::json!({ "k": 1 }),
            None,
        )
        .await;
        out.expect("untyped MCP call must succeed");
    }

    #[tokio::test]
    async fn zero_arg_tool_sends_empty_object() {
        let (_out, calls) = run(
            r#"import test from "@mcp/test";
               function main(): void { test.raw(); }"#,
            serde_json::json!({ "k": 1 }),
            None,
        )
        .await;
        assert_eq!(calls.lock().unwrap()[0].2, "{}");
    }

    #[tokio::test]
    async fn denied_call_throws_before_dispatch() {
        let (out, calls) = run(
            r#"import test from "@mcp/test";
               function main(): void { test.echo({ msg: "hi" }); }"#,
            serde_json::json!({ "reply": "yo" }),
            Some(Arc::new(DenyAll)),
        )
        .await;
        let err = out.expect_err("a denied capability must throw");
        assert!(
            err.to_string().contains("permission denied"),
            "unexpected error: {err}"
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "transport must not be reached when denied"
        );
    }
}
