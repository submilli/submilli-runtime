//! `main` invocation and JSON-encoded result capture.

use wasmtime::{ExnRef, Instance, Rooted, Store, Val};

use crate::runtime::{StoreData, read_submilli_string};

#[derive(Debug, Clone)]
pub struct RunResult {
    pub value: Option<String>,
    pub console: String,
}

pub async fn dispatch_main_async(
    store: &mut Store<StoreData>,
    instance: &Instance,
) -> wasmtime::Result<Option<String>> {
    let main = instance
        .get_func(&mut *store, "main")
        .ok_or_else(|| wasmtime::Error::msg("module has no `main` export"))?;

    // Codegen owns the result encoding: for any non-`void` return it emits a
    // `__main_output` shim that calls `main` and encodes the value to a `$string`
    // (scalars via `toString`, structured returns as JSON), which is read verbatim
    // here. A `void` `main` has no shim and no output, so `main` is invoked directly
    // for its side effects.
    if let Some(to_output) = instance.get_func(&mut *store, "__main_output") {
        let mut out = [Val::null_any_ref()];
        let r = to_output.call_async(&mut *store, &[], &mut out).await;
        map_uncaught_exception(&mut *store, r)?;
        Ok(Some(read_main_string(&mut *store, &out)?))
    } else {
        let r = main.call_async(&mut *store, &[], &mut []).await;
        map_uncaught_exception(&mut *store, r)?;
        Ok(None)
    }
}

/// An uncaught Submilli `throw` surfaces from `main.call` as wasmtime's opaque
/// `ThrownException` (Display: "thrown Wasm exception") — the real message lives
/// only in the pending exception on the store. Pull the thrown `Error`'s
/// `name`/`message` out and re-shape the error around them, so the runtime and
/// CLI render something actionable instead of the placeholder.
pub(crate) fn map_uncaught_exception(
    store: &mut Store<StoreData>,
    result: wasmtime::Result<()>,
) -> wasmtime::Result<()> {
    let Err(err) = result else {
        return Ok(());
    };
    if err.is::<super::host::FatalHostError>() || super::limits::is_memory_exhausted(&err) {
        // A pending guest exception must not replace the actual fatal cause.
        // Memory exhaustion is named; a fatal host error passes unchanged.
        store.take_pending_exception();
        return Err(super::limits::name_memory_exhaustion(err));
    }
    let Some(exn) = store.take_pending_exception() else {
        return Err(err);
    };
    let Some(text) = read_thrown_error_text(store, exn) else {
        return Err(err);
    };
    // The engine captures the throw-site backtrace into the exception and
    // attaches it to the escaped error (host throws get the host-call site);
    // re-wrap it so the CLI renders a source backtrace like a trap.
    let backtrace = err.downcast_ref::<wasmtime::WasmBacktrace>().cloned();
    Err(wasmtime::Error::new(crate::backtrace::ThrownError {
        message: text,
        backtrace,
    }))
}

/// Reads `"name: message"` from a thrown exception's `$Error` payload, plus a
/// `fields:` line for any extra primitive data fields an error subclass carries
/// (`CalendarError.code`, `.status`, …). Returns `None` if the shape doesn't
/// match (e.g. a future non-Error throw), leaving the caller to fall back to
/// the original error.
fn read_thrown_error_text(store: &mut Store<StoreData>, exn: Rooted<ExnRef>) -> Option<String> {
    // The error tag carries a single param: (ref $Error).
    let Val::AnyRef(Some(error_ref)) = exn.field(&mut *store, 0).ok()? else {
        return None;
    };
    let error = error_ref.unwrap_struct(&mut *store).ok()?;
    // $Error is class-shaped: field 2 is the object-fields payload array,
    // with message at slot 0 and name at slot 1.
    let Val::AnyRef(Some(payload_ref)) = error.field(&mut *store, 2).ok()? else {
        return None;
    };
    let payload = payload_ref.unwrap_array(&mut *store).ok()?;
    let message_field = payload.get(&mut *store, 0).ok()?;
    let name_field = payload.get(&mut *store, 1).ok()?;
    let name = read_string_value(store, name_field)?;
    let message = read_string_value(store, message_field)?;
    let header = if name.is_empty() {
        message
    } else {
        format!("{name}: {message}")
    };
    match read_error_fields_line(store, error) {
        Some(fields) => Some(format!("{header}\n{fields}")),
        None => Some(header),
    }
}

const MAX_ERROR_FIELDS: usize = 8;
const MAX_ERROR_FIELD_CHARS: usize = 120;

/// Renders an error subclass's own data fields (`  fields: code = "x", …`).
///
/// The class layout puts field names in slot 1 and values in slot 2, data
/// fields first (message at 0, name at 1) with method closures after them —
/// and the data/method boundary is not recoverable from the struct. Rendering
/// only values whose vtable matches a host primitive singleton skips closures
/// and nested objects for free. Any shape surprise degrades to `None` (the
/// header renders as before).
fn read_error_fields_line(
    store: &mut Store<StoreData>,
    error: Rooted<wasmtime::StructRef>,
) -> Option<String> {
    use crate::runtime::host::{
        host_boxed_boolean_vtable, host_boxed_number_vtable, host_string_vtable,
    };

    let Val::AnyRef(Some(names_ref)) = error.field(&mut *store, 1).ok()? else {
        return None;
    };
    let names = names_ref.unwrap_array(&mut *store).ok()?;
    let Val::AnyRef(Some(payload_ref)) = error.field(&mut *store, 2).ok()? else {
        return None;
    };
    let payload = payload_ref.unwrap_array(&mut *store).ok()?;

    let string_vt = rooted_any(host_string_vtable(store).ok()?)?;
    let number_vt = rooted_any(host_boxed_number_vtable(store).ok()?)?;
    let boolean_vt = rooted_any(host_boxed_boolean_vtable(store).ok()?)?;

    let len = names
        .len(&mut *store)
        .ok()?
        .min(payload.len(&mut *store).ok()?);
    let mut parts: Vec<String> = Vec::new();
    let mut truncated = false;
    for i in 2..len {
        if parts.len() == MAX_ERROR_FIELDS {
            truncated = true;
            break;
        }
        let name_val = names.get(&mut *store, i).ok()?;
        let Some(field_name) = read_string_value(store, name_val) else {
            continue;
        };
        let value = payload.get(&mut *store, i).ok()?;
        let Some(rendered) =
            render_primitive_field(store, value, &string_vt, &number_vt, &boolean_vt)
        else {
            continue;
        };
        parts.push(format!("{field_name} = {rendered}"));
    }
    if parts.is_empty() {
        return None;
    }
    let suffix = if truncated { ", …" } else { "" };
    Some(format!("  fields: {}{suffix}", parts.join(", ")))
}

fn rooted_any(val: Val) -> Option<Rooted<wasmtime::AnyRef>> {
    match val {
        Val::AnyRef(Some(any)) => Some(any),
        _ => None,
    }
}

/// `null`, or the field's value when its vtable is a host primitive singleton;
/// `None` for everything else (closures, arrays, nested objects, bigints).
fn render_primitive_field(
    store: &mut Store<StoreData>,
    value: Val,
    string_vt: &Rooted<wasmtime::AnyRef>,
    number_vt: &Rooted<wasmtime::AnyRef>,
    boolean_vt: &Rooted<wasmtime::AnyRef>,
) -> Option<String> {
    use crate::runtime::json::{boxed_bool, boxed_number, boxed_string};

    let any = match value {
        Val::AnyRef(None) => return Some("null".to_string()),
        Val::AnyRef(Some(any)) => any,
        _ => return None,
    };
    let value_struct = any.unwrap_struct(&mut *store).ok()?;
    let Val::AnyRef(Some(vt)) = value_struct.field(&mut *store, 0).ok()? else {
        return None;
    };
    let boxed = Val::AnyRef(Some(any));
    if Rooted::ref_eq(&*store, &vt, string_vt).ok()? {
        let text = boxed_string(store, boxed).ok()?;
        return Some(quoted_truncated(&text));
    }
    if Rooted::ref_eq(&*store, &vt, number_vt).ok()? {
        let n = boxed_number(store, boxed).ok()?;
        return Some(crate::runtime::number::format_number_js(n));
    }
    if Rooted::ref_eq(&*store, &vt, boolean_vt).ok()? {
        let b = boxed_bool(store, boxed).ok()?;
        return Some(b.to_string());
    }
    None
}

fn quoted_truncated(text: &str) -> String {
    if text.chars().count() <= MAX_ERROR_FIELD_CHARS {
        return format!("{text:?}");
    }
    let cut: String = text.chars().take(MAX_ERROR_FIELD_CHARS).collect();
    format!("{:?}", format!("{cut}…"))
}

/// Decodes a `$string` GC ref (as a [`Val`]) into a Rust `String`.
fn read_string_value(store: &mut Store<StoreData>, value: Val) -> Option<String> {
    let Val::AnyRef(Some(s_ref)) = value else {
        return None;
    };
    let s_struct = s_ref.unwrap_struct(&mut *store).ok()?;
    // Field 1 of `$string` is the packed-UTF-16 `(ref $rawString)` array.
    let Val::AnyRef(Some(raw_ref)) = s_struct.field(&mut *store, 1).ok()? else {
        return None;
    };
    let arr = raw_ref.unwrap_array(&mut *store).ok()?;
    read_submilli_string(&mut *store, arr).ok()
}

/// Reads the `$string` produced by the `__main_output` shim (in `out[0]`) into a
/// Rust `String`, returned verbatim. Errors with a `main`-specific message if the
/// ref slot isn't the expected non-null `$string`.
fn read_main_string(store: &mut Store<StoreData>, out: &[Val]) -> wasmtime::Result<String> {
    let s_struct = match &out[0] {
        Val::AnyRef(Some(any)) => any.unwrap_struct(&mut *store)?,
        Val::AnyRef(None) => {
            return Err(wasmtime::Error::msg(
                "main returned null where the wasm signature is non-nullable",
            ));
        }
        other => wasmtime::bail!("unexpected ref-slot value: {other:?}"),
    };
    // Field 1 of `$string` is `(ref $rawString)` — the raw
    // packed-UTF-16 array. Field 0 is the vtable (skipped).
    let raw_field = s_struct.field(&mut *store, 1)?;
    let arr = match raw_field {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *store)?,
        Val::AnyRef(None) => {
            return Err(wasmtime::Error::msg(
                "main's $string had null raw-array field",
            ));
        }
        other => wasmtime::bail!("$string field 1 not an arrayref: {other:?}"),
    };
    read_submilli_string(&mut *store, arr)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::codegen::tests::compile;
    use crate::runtime::RuntimeConfig;

    // A string return is emitted verbatim — no JSON quoting or escaping — so a
    // quarter-megabyte payload (a realistic web result, e.g. ~235 KB markdown from
    // `jina.search`) costs no per-byte encoding pass and stays within the default
    // fuel budget. The mid-string `"` is part of the output, not an escape.
    #[tokio::test]
    async fn large_string_return_fits_default_fuel() {
        let bytes = compile(
            r#"function main(): string {
                let s: string = "x".repeat(250000);
                return s + "\"";
            }"#,
        );
        let result = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect("250 KB string returns within the default fuel budget");
        // 250k x's + the literal `"` — no surrounding quotes, no escaping.
        assert_eq!(result.value.map(|v| v.len()), Some(250_001));
    }

    #[tokio::test]
    async fn fixture_returns_string_and_captures_console() {
        let bytes =
            compile(r#"function main(): string { console.log("debug"); return "result"; }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("result"));
        assert_eq!(result.console, "debug\n");
    }

    #[tokio::test]
    async fn void_main_returns_none() {
        let bytes = compile("function main(): void { }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert!(result.value.is_none());
        assert_eq!(result.console, "");
    }

    #[tokio::test]
    async fn number_main_returns_json_number() {
        let bytes = compile("function main(): number { return 42; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("42"));
        assert_eq!(result.console, "");
    }

    #[tokio::test]
    async fn boolean_main_returns_json_boolean() {
        let bytes = compile("function main(): boolean { return 1 < 2; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
        assert_eq!(result.console, "");
    }

    #[tokio::test]
    async fn widened_boolean_literal_main_preserves_scalar_output() {
        let source = include_str!("../../tests/fixtures/narrowing/live_boolean_literal_result.ts");
        for source in [source.to_owned(), source.replace("true", "false")] {
            let bytes = compile(&source);
            let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
            assert_eq!(result.value.as_deref(), Some("changed"));
        }
    }

    #[tokio::test]
    async fn console_and_return_are_independent_streams() {
        let bytes = compile(r#"function main(): number { console.log("hi"); return 7; }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("7"));
        assert_eq!(result.console, "hi\n");
    }

    #[tokio::test]
    async fn string_return_is_verbatim_not_escaped() {
        // A `string` return is the program's output as-is — no quoting, no escaping.
        let bytes = compile(r#"function main(): string { return "a\"b\\c\n"; }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("a\"b\\c\n"));
    }

    #[tokio::test]
    async fn nan_returns_nan_via_to_string() {
        let bytes = compile("function main(): number { return 0 / 0; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("NaN"));
    }

    #[tokio::test]
    async fn infinity_returns_infinity_via_to_string() {
        let bytes = compile("function main(): number { return 1 / 0; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("Infinity"));
    }

    #[tokio::test]
    async fn object_main_returns_json_object() {
        let compiled =
            compile_full("function main(): { a: number; b: number } { return { a: 1, b: 2 }; }");
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"{"a":1,"b":2}"#));
    }

    #[tokio::test]
    async fn object_via_local_returns_json_object() {
        let compiled = compile_full(
            "function main(): { a: number } { const o: { a: number } = { a: 1 }; return o; }",
        );
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"{"a":1}"#));
    }

    #[tokio::test]
    async fn number_array_main_returns_json_array() {
        let bytes = compile("function main(): number[] { return [1, 2, 3]; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("[1,2,3]"));
    }

    #[tokio::test]
    async fn string_field_object_is_json_escaped() {
        let compiled = compile_full(r#"function main(): { n: string } { return { n: "a\"b" }; }"#);
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"{"n":"a\"b"}"#));
    }

    #[tokio::test]
    async fn nested_object_main_returns_json() {
        let compiled = compile_full(
            "function main(): { inner: { v: number } } { return { inner: { v: 1 } }; }",
        );
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"{"inner":{"v":1}}"#));
    }

    #[tokio::test]
    async fn array_of_objects_main_returns_json() {
        let compiled =
            compile_full("function main(): { v: number }[] { return [{ v: 1 }, { v: 2 }]; }");
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"[{"v":1},{"v":2}]"#));
    }

    #[tokio::test]
    async fn interface_instance_main_returns_json() {
        let compiled = compile_full(
            "interface Point { x: number; y: number; }\n\
             function main(): Point { return { x: 1, y: 2 }; }",
        );
        let result = RuntimeConfig::default()
            .run_compiled(&compiled)
            .await
            .expect("runs");
        assert_eq!(result.value.as_deref(), Some(r#"{"x":1,"y":2}"#));
    }

    fn compile_full(source: &str) -> crate::compile::CompiledScript {
        crate::compile::compile_script(source, "script.subm", crate::FileId(0), &[], &[])
            .expect("test-only compile expects no diagnostics")
    }

    #[tokio::test]
    async fn string_main_returned_verbatim() {
        // A string return is the output itself — emitted verbatim, no quotes.
        let bytes = compile(r#"function main(): string { return "hi"; }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("hi"));
    }

    #[tokio::test]
    async fn negative_zero_main_normalizes_to_zero() {
        // `(-0).toString()` is "0", matching ECMAScript.
        let bytes = compile("function main(): number { return 0 * -1; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("0"));
    }

    #[tokio::test]
    async fn timeout_interrupts_infinite_loop() {
        // Fuel is bumped so the loop outlives the watchdog tick; otherwise fuel exhaustion races the timeout.
        let bytes = compile("function main(): void { while (true) { } }");
        let cfg = RuntimeConfig {
            fuel: u64::MAX,
            timeout: Some(Duration::from_millis(50)),
            ..RuntimeConfig::default()
        };
        let err = cfg.run(&bytes).await.expect_err("must trap");
        let trap = err.downcast_ref::<wasmtime::Trap>();
        assert_eq!(
            trap,
            Some(&wasmtime::Trap::Interrupt),
            "expected timeout interrupt, got {trap:?} (full error: {err:?})",
        );
    }

    #[tokio::test]
    async fn failed_assert_in_main_propagates_as_error() {
        let bytes = compile(r#"function main(): void { assert(false, "boom"); }"#);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("failed assert should bubble");
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("boom"),
            "expected failed assert to surface its thrown message, got: {rendered}",
        );
    }

    #[tokio::test]
    async fn uncaught_throw_surfaces_error_message() {
        let bytes = compile(r#"function main(): void { throw new Error("boom message"); }"#);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught throw should surface as an error");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("boom message"),
            "uncaught exception should surface its message, got: {rendered}",
        );
        assert!(
            !rendered.contains("thrown Wasm exception"),
            "the opaque placeholder should be replaced, got: {rendered}",
        );
    }

    #[tokio::test]
    async fn uncaught_throw_renders_source_backtrace() {
        // Tier 2: a user `throw` carries a backtrace captured at the throw site.
        let src = "function deep(): void {\n  throw new Error(\"boom\");\n}\nfunction main(): void {\n  deep();\n}";
        let bytes = crate::codegen::tests::compile(src);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught throw");
        let (sources, file) = crate::Sources::single("script.subm", src).unwrap();
        let rendered =
            crate::backtrace::render(&err, &sources, file, crate::backtrace::BacktraceMode::Full)
                .expect("an uncaught throw should render a backtrace");
        assert!(
            rendered.contains("error: Error: boom"),
            "message header missing: {rendered}"
        );
        assert!(
            rendered.contains("at deep ("),
            "throw-site frame: {rendered}"
        );
        assert!(rendered.contains("at main ("), "caller frame: {rendered}");
        assert!(
            rendered.contains("thrown here"),
            "frame-0 label: {rendered}"
        );
        assert!(
            rendered.contains("throw new Error"),
            "source context: {rendered}"
        );
        assert!(
            !rendered.contains("fields:"),
            "a plain Error has no extra data fields: {rendered}"
        );
    }

    #[tokio::test]
    async fn uncaught_class_error_renders_data_fields() {
        let src = r#"class ApiError extends Error {
            code: string;
            status: number;
            retryable: boolean;
            requestId: string | null;
            constructor(code: string, message: string, status: number) {
                super(message);
                this.name = "ApiError";
                this.code = code;
                this.status = status;
                this.retryable = false;
                this.requestId = null;
            }
        }
        function main(): void { throw new ApiError("badRequest", "Bad Request", 400); }"#;
        let bytes = compile(src);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught throw");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("ApiError: Bad Request"),
            "header: {rendered}"
        );
        assert!(
            rendered.contains(
                "  fields: code = \"badRequest\", requestId = null, retryable = false, status = 400"
            ),
            "fields line: {rendered}"
        );
    }

    #[tokio::test]
    async fn error_fields_skip_methods_and_objects() {
        let src = r#"class DetailError extends Error {
            code: string;
            detail: number[];
            constructor() {
                super("nope");
                this.name = "DetailError";
                this.code = "bad";
                this.detail = [1, 2];
            }
            describe(): string { return this.code; }
        }
        function main(): void { throw new DetailError(); }"#;
        let bytes = compile(src);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught throw");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("fields: code = \"bad\""),
            "primitive field renders: {rendered}"
        );
        assert!(
            !rendered.contains("detail") && !rendered.contains("describe"),
            "object fields and methods stay out of the dump: {rendered}"
        );
    }

    #[tokio::test]
    async fn error_field_strings_truncate() {
        let src = r#"class BigError extends Error {
            body: string;
            constructor() {
                super("big");
                this.name = "BigError";
                this.body = "x".repeat(500);
            }
        }
        function main(): void { throw new BigError(); }"#;
        let bytes = compile(src);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught throw");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("…") && !rendered.contains(&"x".repeat(200)),
            "long string field truncates: {rendered}"
        );
    }

    #[tokio::test]
    async fn uncaught_host_throw_renders_clean_backtrace() {
        // Tier 1: a Temporal host error, captured in the host throw path. The
        // message is Temporal-native (no backing-crate leak) and a backtrace shows.
        let src = "function main(): void {\n  Temporal.Instant.from(\"nope\");\n}";
        let bytes = crate::codegen::tests::compile(src);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught host throw");
        let (sources, file) = crate::Sources::single("script.subm", src).unwrap();
        let rendered =
            crate::backtrace::render(&err, &sources, file, crate::backtrace::BacktraceMode::Full)
                .expect("a host throw should render a backtrace");
        assert!(
            rendered.contains("not a valid ISO 8601 instant"),
            "clean message: {rendered}"
        );
        assert!(!rendered.contains("jiff"), "must not leak jiff: {rendered}");
        assert!(rendered.contains("at main ("), "frame: {rendered}");
    }

    #[tokio::test]
    async fn uncaught_host_throw_surfaces_message() {
        // JSON.parse failure throws host-side; uncaught, it must
        // still surface serde's detail rather than the opaque placeholder.
        let bytes = compile(r#"function main(): void { JSON.parse("not json") as number; }"#);
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("uncaught host throw should surface as an error");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("JSON.parse:"),
            "uncaught host exception should surface its message, got: {rendered}",
        );
    }

    #[tokio::test]
    async fn uncaught_json_shape_mismatch_surfaces_message() {
        // Codegen-side throw (`as` validator), uncaught: must surface its
        // mismatch message rather than the opaque placeholder.
        let bytes = compile(
            r#"function main(): number { const n = JSON.parse("\"x\"") as number; return n; }"#,
        );
        let err = RuntimeConfig::default()
            .run(&bytes)
            .await
            .expect_err("shape mismatch should surface as an error");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("type mismatch: expected number, got string"),
            "uncaught shape mismatch should surface its message, got: {rendered}",
        );
    }

    #[tokio::test]
    async fn number_to_string_arithmetic() {
        let bytes = compile("function main(): string { return (1 + 2 * 3).toString(); }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("7"));
    }

    #[tokio::test]
    async fn number_to_string_double_precision() {
        // ECMAScript shortest round-trip: 0.1 + 0.2 yields the
        // canonical "0.30000000000000004" form.
        let bytes = compile("function main(): string { return (0.1 + 0.2).toString(); }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("0.30000000000000004"));
    }

    #[tokio::test]
    async fn string_coercion_alias_for_to_string() {
        let bytes = compile("function main(): string { return String(42); }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn number_coercion_parses_decimal() {
        let bytes = compile(r#"function main(): number { return Number("1.3"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("1.3"));
    }

    #[tokio::test]
    async fn number_coercion_empty_string_is_zero() {
        // JS-compat: Number("") returns 0 (not NaN).
        let bytes = compile(r#"function main(): number { return Number(""); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("0"));
    }

    #[tokio::test]
    async fn number_coercion_invalid_returns_nan() {
        let bytes = compile(r#"function main(): number { return Number("abc"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("NaN"));
    }

    #[tokio::test]
    async fn parse_int_prefix_decimal() {
        let bytes = compile(r#"function main(): number { return parseInt("42px"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn parse_int_hex_with_explicit_radix() {
        let bytes = compile(r#"function main(): number { return parseInt("0xff", 16); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("255"));
    }

    #[tokio::test]
    async fn parse_int_no_digits_is_nan() {
        let bytes = compile(r#"function main(): number { return parseInt("abc"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("NaN"));
    }

    #[tokio::test]
    async fn parse_float_prefix() {
        let bytes = compile(r#"function main(): number { return parseFloat("1.5x"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("1.5"));
    }

    #[tokio::test]
    async fn parse_float_leading_decimal() {
        let bytes = compile(r#"function main(): number { return parseFloat(".5"); }"#);
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("0.5"));
    }

    #[tokio::test]
    async fn number_to_string_round_trip() {
        let bytes = compile("function main(): boolean { return Number((42).toString()) === 42; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn object_structural_equality_distinct_allocations() {
        let bytes = compile(
            "function main(): boolean { let p = { a: 1 }; let q = { a: 1 }; return p === q; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn object_structural_inequality_different_field_values() {
        let bytes = compile(
            "function main(): boolean { let p = { a: 1 }; let q = { a: 2 }; return p === q; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn object_ref_eq_fast_path() {
        let bytes = compile("function main(): boolean { let p = { a: 1 }; return p === p; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn object_with_string_field_structural_equals() {
        let bytes = compile(
            r#"function main(): boolean { let p = { n: "x" }; let q = { n: "x" }; return p === q; }"#,
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn nested_object_equal() {
        let bytes = compile(
            "function main(): boolean { let p = { inner: { v: 1 } }; let q = { inner: { v: 1 } }; return p === q; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn nested_object_inner_differs() {
        let bytes = compile(
            "function main(): boolean { let p = { inner: { v: 1 } }; let q = { inner: { v: 2 } }; return p === q; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn array_structural_equality_distinct_allocations() {
        let bytes = compile(
            "function main(): boolean { let a = [1, 2, 3]; let b = [1, 2, 3]; return a === b; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn array_inequality_same_length_different_elements() {
        let bytes = compile(
            "function main(): boolean { let a = [1, 2, 3]; let b = [1, 2, 4]; return a === b; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn array_inequality_different_lengths() {
        let bytes = compile(
            "function main(): boolean { let a = [1, 2]; let b = [1, 2, 3]; return a === b; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn array_of_strings_structural_equals() {
        let bytes = compile(
            r#"function main(): boolean { let a = ["x", "y"]; let b = ["x", "y"]; return a === b; }"#,
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn array_of_objects_structural_equals() {
        let bytes = compile(
            "function main(): boolean { let a = [{ v: 1 }, { v: 2 }]; let b = [{ v: 1 }, { v: 2 }]; return a === b; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn double_eq_same_as_strict_eq() {
        let bytes = compile(
            "function main(): boolean { return (1 === 1) && (1 == 1) && (1 !== 2) && (1 != 2); }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn object_not_eq_round_trip() {
        let bytes = compile(
            "function main(): boolean { let p = { a: 1 }; let q = { a: 2 }; return p !== q; }",
        );
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn null_self_equality() {
        let bytes = compile("function main(): boolean { return null === null; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("true"));
    }

    #[tokio::test]
    async fn null_not_equal_to_self_is_false() {
        let bytes = compile("function main(): boolean { return null !== null; }");
        let result = RuntimeConfig::default().run(&bytes).await.expect("runs");
        assert_eq!(result.value.as_deref(), Some("false"));
    }
}
