//! `submilli:security` — semantic capability check.
//!
//! One async Rust host function registered directly under the package name.
//! The `context` value is serialized by re-entering its `toJson` vtable slot
//! (the dispatch may run guest code for user classes), then handed to the
//! embedder's policy engine.

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    permission_denied, permission_denied_invariant, read_string_arg, register_host_fn_async,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::vtable::dispatch_vtable_slot;
use crate::runtime::security::CheckOutcome;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:security";

/// `toJson` is slot 1 of the four-slot `$VTable`.
const TO_JSON_SLOT: usize = 1;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    defs.values.insert(
        "check".to_string(),
        ValueSymbol {
            name: "check".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "check"),
            declaration_span: Span::at(crate::FileId::SECURITY),
            kind: ValueKind::Function {
                generics: vec!["T".to_string()],
                params: vec![
                    Param::new("capability", Type::String),
                    Param::new("context", Type::TypeVar("T".to_string())),
                ],
                ret: Type::Void,
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::SECURITY,
                    "/**\n * Semantic security check. Invokes the embedder's policy engine; \
                     throws a catchable `PermissionDeniedError` if the policy denies the \
                     call.\n * @param capability Dotted \
                     capability identifier (e.g. `\"test.com/op\"`).\n * @param context \
                     Arbitrary value (typically an object) whose fields the policy matches \
                     against rules. Serialized to JSON at the host boundary.\n */",
                ),
            },
        },
    );
    defs
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    // The erased-generic `context: T` lowers to `(ref null $Object)`.
    let nullable_object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let ty = FuncType::new(&engine, [string, nullable_object], []);
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "check"),
        ty,
        /* deterministic = */ false,
        |caller, params, _results| {
            Box::pin(async move {
                let capability =
                    read_string_arg(&mut *caller, &params[0], "security.check (capability)")?;
                if matches!(params[1], Val::AnyRef(None)) {
                    wasmtime::bail!("security.check: context must not be null");
                }
                let json_val = dispatch_vtable_slot(caller, &params[1], TO_JSON_SLOT, &[]).await?;
                let context_json =
                    read_string_arg(&mut *caller, &json_val, "security.check (context)")?;
                let context: serde_json::Value =
                    serde_json::from_str(&context_json).map_err(|e| {
                        wasmtime::Error::msg(format!("security.check: malformed context JSON: {e}"))
                    })?;
                let who = consumer_of_running_package(&*caller, &capability)?;
                match caller
                    .data()
                    .security_check
                    .clone()
                    .check(&who, &capability, &context)
                {
                    CheckOutcome::Allow => Ok(()),
                    CheckOutcome::Deny { reason } => {
                        Err(permission_denied(who, capability, reason))
                    }
                }
            })
        },
    )
}

/// Who is asking the package that called `check`, walking out from the innermost wasm frame
/// to the first frame owned by a different package.
///
/// Unlike a gated stdlib op — attributed to its immediate caller — an explicit `check` gates
/// the *consumer* of the package that runs it: a library checking `test.op` asks "may my
/// caller do this?", so attribution is one below the running code.
///
/// **Which caller, once a closure crosses a package boundary?** The running code and the
/// package last entered have different owners then, and two readings exist. This picks the
/// *immediate invoker* — the first differing frame walking outward — over pointing further up
/// the chain, because it is what the frame walk naturally produces, because a defensive check
/// wants to know who actually triggered this code, and because the alternative builds a
/// confused deputy: a package would answer for work it did not request.
///
/// **`main` running is an error only with a package beneath it.** That is the confused-deputy
/// shape — main-authored code reached through a package, where naming the package would let
/// main speak for it. It is the escalation this work closes.
///
/// Both unknown-principal paths refuse rather than guess, matching [`running_package`]: no
/// frames at all, and any frame in the walk whose module declares no name.
///
/// A script asking about *itself*, with no package on the stack, is answered rather than
/// refused. It reads the operator's own `main:` rules and grants nothing: `check` performs no
/// side effect, and every real operation is gated independently by `check_security`, which has
/// no `main` exception. Refusing it would silently strip a documented, exercised capability —
/// and would make an operator's `main:` rule for that capability dead on arrival — while
/// closing no hole, since the confused-deputy case is caught by the guard above regardless.
fn consumer_of_running_package(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    capability: &str,
) -> wasmtime::Result<String> {
    let backtrace = wasmtime::WasmBacktrace::force_capture(store);
    let mut frames = backtrace.frames().iter();
    let Some(running) = frames.next().map(wasmtime::FrameInfo::module) else {
        return Err(permission_denied_invariant(
            "<no wasm frame>",
            capability,
            "no wasm frame is executing, so there is no caller to gate",
        ));
    };
    let Some(running) = running.name() else {
        return Err(permission_denied_invariant(
            "<unnamed module>",
            capability,
            "the running module declares no package name, so its caller cannot be identified",
        ));
    };
    // Fail closed on an unnamed frame anywhere in the walk, not just at the top. Skipping one
    // would let the walk step *past* an unidentifiable principal and name whatever sits
    // beyond it — attributing to a package that is not the immediate invoker.
    let mut below = None;
    for frame in frames {
        let Some(owner) = frame.module().name() else {
            return Err(permission_denied_invariant(
                "<unnamed module>",
                capability,
                "a frame between the running code and its caller declares no package name, so \
                 the caller cannot be identified",
            ));
        };
        if owner != running {
            below = Some(owner);
            break;
        }
    }
    if running == crate::mangle::USER_PACKAGE && below.is_some() {
        // Invariant, not policy: no rule can grant this, so the message must not send the
        // reader off to ask the operator for one.
        return Err(permission_denied_invariant(
            running,
            capability,
            "security.check gates the caller of the package that runs it. This is main's own \
             code running inside a package, which has no caller to gate — naming the package \
             here would let main-authored code speak for it",
        ));
    }
    // No differing frame means one principal owns the whole stack: a script asking about
    // itself, a package's test file calling into its library, or an initializer with nothing
    // above it. The consumer sits outside the guest, which is the script's position.
    Ok(below.unwrap_or(crate::mangle::USER_PACKAGE).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One package owns every frame — a package's own test file calling into its library, or
    /// an initializer with nothing above it. There is no "one below" to name, and the walk
    /// must not return the running package as its own consumer. The consumer sits outside the
    /// guest, which is the script's position.
    #[tokio::test]
    async fn a_package_alone_on_the_stack_answers_main() {
        let source = "import { check } from \"submilli:security\";\n\n\
                      function main(): void {\n  check(\"test.com/op\", { foo: 1 });\n}\n";
        let compiled = crate::compile::compile_script_owned_by(
            "test:solo",
            source,
            "script.subm",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.security_check = std::sync::Arc::new(DenyAll);
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("DenyAll refuses");
        assert!(
            format!("{err}").contains("caller=main"),
            "a lone package's consumer is the script; got: {err}"
        );
    }

    /// The case the guard exists for, and the only shape `check` refuses: `main`-authored code
    /// reached through a package. Here `main`'s `toJson` runs inside the package's
    /// `JSON.stringify`, so the running code is `main`'s with a package beneath it. Answering
    /// would name that package and let `main`-authored code speak for it — the escalation this
    /// work closes, arriving at `security.check` instead of at a gated host fn.
    #[tokio::test]
    async fn main_authored_code_inside_a_package_cannot_ask_on_the_packages_behalf() {
        let (lib_bytes, lib_decl, lib_type_info) = crate::codegen::tests::compile_package_modules(
            "test:wrap",
            &[(
                "lib",
                r#"
                /** Pass-through JSON encoder. */
                export function passthrough(value: unknown): string {
                    return JSON.stringify(value);
                }
                "#,
            )],
            &[],
        );
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, "test:wrap", lib_inst)
            .expect("register library instance");
        let public_name = crate::mangle::package_symbol("test:wrap", "passthrough");
        let func = lib_inst
            .get_func(&mut store, public_name.as_str())
            .expect("library public export");
        linker
            .define(&mut store, "test:wrap", "passthrough", func)
            .expect("plain package import alias");

        let consumer = crate::compile_script(
            r#"
            import { passthrough } from "test:wrap";
            import { check } from "submilli:security";

            function gate(): void {
                check("test.com/op", { foo: 1 });
            }

            class Probe {
                seen: number;
                constructor() { this.seen = 0; }
                toJson(): string {
                    gate();
                    this.seen = 1;
                    return "\"ok\"";
                }
            }

            function main(): number {
                const p = new Probe();
                const _ = passthrough(p);
                return p.seen;
            }
            "#,
            "consumer.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("consumer compiles");
        store
            .data_mut()
            .install_type_info(consumer.type_info.clone());
        let consumer_module = wasmtime::Module::new(&engine, &consumer.wasm).expect("module");
        let inst = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .expect("instantiate consumer");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("main-authored code inside a package may not ask");
        let msg = format!("{err}");
        assert!(
            msg.contains("no caller to gate") && !msg.contains("caller=test:wrap"),
            "must refuse rather than name the package: {msg}"
        );
    }

    /// Rebuilds `bytes` without its `name` custom section, producing a module with no declared
    /// principal — the shape an embedder can hand to the raw-bytes API, which codegen itself
    /// never emits.
    fn strip_name_section(bytes: &[u8]) -> Vec<u8> {
        use wasmparser::{Parser, Payload};
        let mut out = bytes[..8].to_vec();
        for payload in Parser::new(0).parse_all(bytes) {
            let payload = payload.expect("payload");
            if let Payload::CustomSection(reader) = &payload
                && reader.name() == "name"
            {
                continue;
            }
            if let Some((id, range)) = payload.as_section() {
                out.push(id);
                let mut len = Vec::new();
                leb128_write(&mut len, range.len() as u64);
                out.extend_from_slice(&len);
                out.extend_from_slice(&bytes[range]);
            }
        }
        out
    }

    fn leb128_write(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    /// A module with no `name` section has no principal to speak for, so a gated call from it
    /// must be refused rather than attributed. Reachable through the embeddable API, which
    /// accepts raw wasm bytes this compiler did not produce.
    ///
    /// This is the assertion whose absence let `security.check`'s walk silently skip unnamed
    /// frames instead of failing closed.
    #[tokio::test]
    async fn a_module_without_a_name_is_refused_rather_than_attributed() {
        let source = r#"
            import { get } from "submilli:secrets";
            function main(): void { const _ = get("TOKEN"); }
        "#;
        let compiled = crate::compile_script(source, "anon.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let stripped = strip_name_section(&compiled.wasm);

        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let module = wasmtime::Module::new(&engine, &stripped).expect("stripped module validates");
        assert_eq!(
            module.name(),
            None,
            "the fixture must genuinely lack a module name, or it proves nothing"
        );

        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("an unnameable principal must be refused");
        let msg = format!("{err}");
        assert!(
            msg.contains("<unnamed module>"),
            "must name the unresolvable principal rather than defaulting to one: {msg}"
        );
        assert!(
            !msg.contains("caller=main"),
            "must not fall back to main: {msg}"
        );
    }

    /// The same unnameable principal reaching `security.check` rather than a gated op. Before
    /// the walk failed closed it dropped unnamed frames from consideration, so this reported
    /// `<no wasm frame>` — claiming nothing was executing while a frame plainly was. The label
    /// is the discriminator.
    ///
    /// Not covered here: an unnamed frame *between* two named ones. That needs a linked module
    /// this compiler cannot emit, so the walk's mid-stack guard rests on the code, not a test.
    #[tokio::test]
    async fn security_check_from_an_unnamed_module_names_the_right_failure() {
        let source = r#"
            import { check } from "submilli:security";
            function main(): void { check("test.com/op", { foo: 1 }); }
        "#;
        let compiled = crate::compile_script(source, "anon.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let stripped = strip_name_section(&compiled.wasm);

        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let module = wasmtime::Module::new(&engine, &stripped).expect("module validates");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("an unnameable principal must be refused");
        let msg = format!("{err}");
        assert!(
            msg.contains("<unnamed module>"),
            "must name the running module as the unresolvable one, not claim no frame exists: \
             {msg}"
        );
        // An invariant, not a policy outcome — no rule can grant it, so the message must not
        // send the reader off to ask the operator for one.
        assert!(
            !msg.contains("operator's policy"),
            "an unresolvable principal is an invariant refusal, not a policy denial: {msg}"
        );
    }

    /// An unnamed frame *between* the running code and its caller. `@test/inner` runs the
    /// check and is named; the relay that invoked it is not; `main` is named beyond that.
    ///
    /// The walk must refuse here rather than skip the unnameable frame and name `main` — that
    /// would attribute the call to something two hops out that never invoked the check, which
    /// is the confused deputy the guard exists to prevent. Before the walk failed closed it
    /// did exactly that.
    ///
    /// `install_package_modules_async` now refuses to link an unnamed package, so this is
    /// unreachable in production; the modules are linked by hand to reach the guard directly.
    #[tokio::test]
    async fn an_unnamed_frame_between_caller_and_callee_is_refused() {
        let (inner_bytes, inner_decl, inner_ti) = crate::codegen::tests::compile_package_modules(
            "test:inner",
            &[(
                "lib",
                r#"
                import { check } from "submilli:security";

                /**
                 * Gates an operation on behalf of whoever called it.
                 * @capability test.com/op { foo }
                 */
                export function guarded(foo: number): void {
                    check("test.com/op", { foo });
                }
                "#,
            )],
            &[],
        );
        let (relay_bytes, relay_decl, relay_ti) = crate::codegen::tests::compile_package_modules(
            "test:relay",
            &[(
                "lib",
                r#"
                import { guarded } from "test:inner";

                /** Calls the inner package, standing between it and the script. */
                export function relay(foo: number): void {
                    guarded(foo);
                }
                "#,
            )],
            &[&inner_decl],
        );
        // The middle frame loses its identity; the other two keep theirs.
        let relay_bytes = strip_name_section(&relay_bytes);

        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(inner_ti);
        data.install_type_info(relay_ti);
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");

        let inner_module = wasmtime::Module::new(&engine, &inner_bytes).expect("inner module");
        let inner_inst = linker
            .instantiate_async(&mut store, &inner_module)
            .await
            .expect("instantiate inner");
        linker
            .instance(&mut store, "test:inner", inner_inst)
            .expect("register inner");

        let relay_module = wasmtime::Module::new(&engine, &relay_bytes).expect("relay module");
        assert_eq!(
            relay_module.name(),
            None,
            "the middle module must genuinely lack a name, or the test proves nothing"
        );
        let relay_inst = linker
            .instantiate_async(&mut store, &relay_module)
            .await
            .expect("instantiate relay");
        linker
            .instance(&mut store, "test:relay", relay_inst)
            .expect("register relay");
        let public_name = crate::mangle::package_symbol("test:relay", "relay");
        let relay_fn = relay_inst
            .get_func(&mut store, public_name.as_str())
            .expect("relay export");
        linker
            .define(&mut store, "test:relay", "relay", relay_fn)
            .expect("plain package import alias");

        let consumer = crate::compile_script(
            r#"
            import { relay } from "test:relay";
            function main(): void { relay(1); }
            "#,
            "consumer.subm",
            crate::FileId(0),
            &[&relay_decl],
            &[],
        )
        .expect("consumer compiles");
        store
            .data_mut()
            .install_type_info(consumer.type_info.clone());
        let consumer_module = wasmtime::Module::new(&engine, &consumer.wasm).expect("module");
        let inst = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .expect("instantiate consumer");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("an unnameable frame in the walk must be refused");
        let msg = format!("{err}");
        assert!(
            msg.contains("<unnamed module>"),
            "must refuse on the unnameable frame: {msg}"
        );
        assert!(
            !msg.contains("caller=main"),
            "must not skip the unnameable frame and name main: {msg}"
        );
    }

    #[test]
    fn module_name_matches_runtime_constant() {
        // Kept stable for any external embedders that reference the
        // constant by re-export from `crate::runtime`.
        assert_eq!(MODULE_NAME, "submilli:security");
    }

    struct DenyAll;
    impl crate::runtime::SecurityCheck for DenyAll {
        fn check(
            &self,
            _caller: &str,
            _capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            CheckOutcome::Deny {
                reason: "blocked by policy".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn uncaught_denial_renders_message_and_backtrace() {
        let source = "import { check } from \"submilli:security\";\n\n\
                      function main(): void {\n  check(\"test.com/op\", { foo: 1 });\n}\n";
        let compiled = crate::compile_script(source, "script.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.security_check = std::sync::Arc::new(DenyAll);
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let err = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("uncaught denial surfaces as a runtime error");

        let msg = format!("{err}");
        assert!(
            msg.contains(
                "PermissionDeniedError: permission denied: \
                 caller=main capability=test.com/op: blocked by policy"
            ),
            "expected the class-prefixed denial message; got: {msg}"
        );
        assert!(
            msg.contains("fields:")
                && msg.contains("capability = \"test.com/op\"")
                && msg.contains("caller = \"main\""),
            "denial data fields should render on the fields line; got: {msg}"
        );

        let (sources, file) = crate::Sources::single("script.subm", source);
        let rendered =
            crate::backtrace::render(&err, &sources, file, crate::backtrace::BacktraceMode::Full)
                .expect("an uncaught denial should render a backtrace");
        assert!(
            rendered.contains("at main ("),
            "backtrace should point at main: {rendered}"
        );
        assert!(
            rendered.contains("check(\"test.com/op\""),
            "source context should show the check() call site: {rendered}"
        );
    }
}
