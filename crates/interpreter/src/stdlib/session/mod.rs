//! `submilli:session` — a key-value store scoped to one session.
//!
//! Rust host functions registered directly under the package name. Each op runs
//! `check_security` before touching the store; the gated capabilities are
//! cataloged in [`crate::stdlib::capabilities`] — keep it in sync when adding
//! or removing a gate (see AGENTS.md).
//!
//! Storage is the embedder's: [`StoreData::session_kv`] holds the provider. A
//! runtime with none configured refuses every op rather than inventing state,
//! so a program never silently writes into a store nobody can read back.
//!
//! Keys and payloads stay UTF-16 code units from the guest `$string` to the
//! trait and back — see [`value`] for why.

use crate::runtime::host::{abi_arg, abi_result};
mod cursor;
pub mod declaration;
pub(crate) mod value;

use std::sync::Arc;

use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::call_log::{Payload, Side, record_payload};
use crate::runtime::decision::CallTicket;
use crate::runtime::fuel;
use crate::runtime::host::{register_host_fn, register_host_fn_async};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::session_kv::{SessionKvEntry, SessionKvError, SessionKvPage, SessionKvStore};
use crate::stdlib::abi::{self, backing_struct, f64_field, install_field_getters, string_field};
use crate::stdlib::shared::check_security_call;

pub const MODULE_NAME: &str = "submilli:session";

/// Entries one `list` page may hold. Matches the catalog's documented range.
pub(super) const MAX_LIST_LIMIT: u32 = 1000;

/// Keys one `list` call walks before it stops and hands back a cursor, however
/// few of them matched the prefix. A work bound, not a result bound: a prefix
/// matching nothing still costs at most this much, and the caller pages on.
const MAX_SCAN_PER_PAGE: usize = 512;

// `$EntryBacking` field indices (0 is the vtable).
const ENTRY_KEY: usize = 1;
const ENTRY_SIZE_BYTES: usize = 2;

// `$PageBacking` field indices.
const PAGE_ENTRIES: usize = 1;
const PAGE_NEXT_CURSOR: usize = 2;

pub use declaration::package_declaration;

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    // `key: string` takes the whole `$string` struct rather than the raw payload
    // array: the raw-string unwrap/rewrap in `emit_direct_call` is applied only
    // on the plain `Call` path, so a generic or indirect call site would hand us
    // the struct regardless. Same convention as `security.check`.
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    // `unknown` — a value and a `get` result alike — lowers to `(ref null $Object)`.
    let unknown = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "get"),
        FuncType::new(&engine, [string.clone()], [unknown.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let key = read_key(caller, abi_arg(params, 0)?, "get")?;
                let ticket = gate(caller, "session.read", &key)?;
                let store = provider(caller, "get")?;
                let found = store.get(&key).map_err(|e| trap(caller, None, &e))?;
                record_payload(&*caller, ticket, Side::Response, || match &found {
                    // The size is of the copy and digest's encoding: UTF-8 bytes.
                    Some(payload) => Payload::meta(serde_json::Value::Null)
                        .with_owned_body(String::from_utf16_lossy(payload).into_bytes()),
                    None => Payload::meta(serde_json::json!({ "found": false })),
                });
                let Some(payload) = found else {
                    *abi_result(results, 0)? = crate::runtime::prelude::undefined::value(caller)?;
                    return Ok(());
                };
                // Read from the store (code units, two bytes each), then
                // parsed back into values.
                fuel::charge(
                    &mut *caller,
                    fuel::IO,
                    (payload.len() as u64).saturating_mul(2),
                )?;
                fuel::charge(&mut *caller, fuel::PARSE, payload.len() as u64)?;
                *abi_result(results, 0)? = value::deserialize(caller, &payload)?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "has"),
        FuncType::new(&engine, [string.clone()], [ValType::I32]),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let key = read_key(caller, abi_arg(params, 0)?, "has")?;
                let ticket = gate(caller, "session.read", &key)?;
                let store = provider(caller, "has")?;
                let found = store.has(&key).map_err(|e| trap(caller, None, &e))?;
                record_payload(&*caller, ticket, Side::Response, || {
                    Payload::meta(serde_json::json!({ "found": found }))
                });
                *abi_result(results, 0)? = Val::I32(i32::from(found));
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "set"),
        FuncType::new(&engine, [string.clone(), unknown.clone()], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            Box::pin(async move {
                let key = read_key(caller, abi_arg(params, 0)?, "set")?;
                let ticket = gate(caller, "session.write", &key)?;
                // Serialization runs before the provider is consulted: a value
                // with no JSON form must leave the previous entry intact.
                let payload = value::serialize(caller, abi_arg(params, 1)?).await?;
                // Written to the store: code units, two bytes each.
                fuel::charge(
                    &mut *caller,
                    fuel::IO,
                    (payload.len() as u64).saturating_mul(2),
                )?;
                let store = provider(caller, "set")?;
                store
                    .set(&key, &payload)
                    .map_err(|e| trap(caller, ticket, &e))
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "remove"),
        FuncType::new(&engine, [string.clone()], [ValType::I32]),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let key = read_key(caller, abi_arg(params, 0)?, "remove")?;
                gate(caller, "session.remove", &key)?;
                let store = provider(caller, "remove")?;
                *abi_result(results, 0)? = Val::I32(i32::from(
                    store.remove(&key).map_err(|e| trap(caller, None, &e))?,
                ));
                Ok(())
            })
        },
    )?;

    // `list` re-enters no guest code, so it registers synchronously.
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "list"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64, unknown.clone()],
            [unknown.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            *abi_result(results, 0)? = list(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                abi_arg(params, 2)?,
            )?;
            Ok(())
        },
    )?;

    install_getters(linker, &engine, &intr, string, unknown)?;

    Ok(())
}

/// `Entry` and `Page` are `Dispatch::Direct`, so each property is a host getter
/// under `submilli:session#<Iface>#<prop>` whose result must be exactly what
/// codegen lowers the declared type to — there is no coercion in between.
fn install_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    string: ValType,
    nullable_object: ValType,
) -> wasmtime::Result<()> {
    // A Direct receiver is the non-null `(ref $Object)`; the guest holds these
    // backings as the nullable `unknown` lowering and codegen bridges the two.
    let receiver = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    // `entries: Entry[]` lowers to a non-null `$Array`; `nextCursor: string |
    // undefined` is a mixed union, so it lowers to the universal `(ref null $Object)`.
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));
    install_field_getters(
        linker,
        MODULE_NAME,
        "Entry",
        engine,
        &receiver,
        &[
            ("key", ENTRY_KEY, string),
            ("sizeBytes", ENTRY_SIZE_BYTES, ValType::F64),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "Page",
        engine,
        &receiver,
        &[
            ("entries", PAGE_ENTRIES, array),
            ("nextCursor", PAGE_NEXT_CURSOR, nullable_object),
        ],
    )
}

fn entry_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // key
            f64_field(),         // sizeBytes
        ],
    )
}

fn page_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            abi::array_field(&intr),           // entries
            abi::nullable_object_field(&intr), // nextCursor
        ],
    )
}

/// One `list` call: gate the operation, then walk the keyspace gating each
/// candidate key, until the page fills or the scan bound stops the walk.
fn list(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    prefix_val: &Val,
    limit_val: &Val,
    cursor_val: &Val,
) -> wasmtime::Result<Val> {
    let prefix = value::read_units(caller, prefix_val, "session.list (prefix)")?;
    let limit = read_limit(limit_val)?;
    let ticket = check_security_call(
        &mut *caller,
        "session.list",
        serde_json::json!({ "prefix": String::from_utf16_lossy(&prefix) }),
    )?;
    let resume_after = read_cursor(caller, cursor_val, &prefix)?;
    let store = provider(caller, "list")?;

    let mut scanned = store
        .scan(resume_after.as_deref(), &prefix, MAX_SCAN_PER_PAGE)
        .map_err(|e| trap(caller, None, &e))?;

    // The `limit` applies to candidates, not to survivors of the filter: taking
    // `limit` candidates and only then filtering keeps the cursor independent of
    // what the policy hid. Refilling the page from further keys would make the
    // number of keys consumed a function of how many were denied, which is the
    // disclosure the filter exists to prevent.
    scanned.entries.truncate(limit);
    let considered = std::mem::take(&mut scanned.entries);
    let next_cursor = next_cursor(&prefix, &scanned, &considered, limit).map_err(cursor_trap)?;

    let mut visible = Vec::new();
    for entry in considered {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        if may_read(caller, &entry.key)? {
            visible.push(entry);
        }
    }
    record_payload(&*caller, ticket, Side::Response, || {
        let keys: Vec<_> = visible
            .iter()
            .map(|entry| String::from_utf16_lossy(&entry.key))
            .collect();
        Payload::meta(serde_json::json!({ "keys": keys, "cursor": next_cursor.is_some() }))
    });
    build_page(caller, &visible, next_cursor.as_deref())
}

/// Where the next page resumes, or `None` once nothing matching can remain.
///
/// Two things leave keys behind and both must mint a cursor: the `limit` cut the
/// matches short, or the scan bound stopped the walk with the keyspace
/// unfinished. The second is the subtle one — it can leave a page empty while
/// matches sit just past `last_scanned` — so an empty page is never on its own
/// evidence that a listing is done.
///
/// The cursor is derived from the keys *considered*, never from the ones that
/// survived the filter, so two callers under different policies get the same
/// page boundary and neither learns what the stricter one hid. The key it
/// resumes after is therefore routinely one the caller was denied — see
/// [`cursor`] for why that means the cursor must be sealed.
fn next_cursor(
    prefix: &[u16],
    scanned: &SessionKvPage,
    considered: &[SessionKvEntry],
    limit: usize,
) -> Result<Option<String>, cursor::CursorError> {
    let filled = considered.len() == limit;
    if !filled && scanned.scanned_all {
        return Ok(None);
    }
    let resume_from = if filled {
        considered.last().map(|entry| &entry.key)
    } else {
        scanned.last_scanned.as_ref()
    };
    resume_from
        .map(|key| cursor::encode(prefix, key))
        .transpose()
}

fn read_limit(val: &Val) -> wasmtime::Result<usize> {
    let Val::F64(bits) = val else {
        return Err(crate::runtime::host::type_error(
            "session.list: `limit` must be a number".to_string(),
        ));
    };
    let limit = f64::from_bits(*bits);
    if !(limit.fract() == 0.0 && (1.0..=f64::from(MAX_LIST_LIMIT)).contains(&limit)) {
        return Err(crate::runtime::host::range_error(format!(
            "session.list: `limit` must be a whole number from 1 to {MAX_LIST_LIMIT}, got \
             {limit} — pass a smaller page size and follow `nextCursor` for the rest"
        )));
    }
    Ok(limit as usize)
}

fn read_cursor(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
    prefix: &[u16],
) -> wasmtime::Result<Option<Vec<u16>>> {
    if crate::runtime::prelude::undefined::is_undefined(caller, val)? {
        return Ok(None);
    }
    let max_units = cursor::max_cursor_units(provider(caller, "list")?.limits().max_key_units);
    let units = value::read_units_bounded(caller, val, "session.list (cursor)", max_units)?;
    cursor::decode(prefix, &units)
        .map(Some)
        .map_err(cursor_trap)
}

/// The per-key half of the double gate. A denial omits the key rather than
/// failing the call: a listing that threw on the first forbidden key would
/// itself disclose that the key exists.
fn may_read(caller: &mut wasmtime::Caller<'_, StoreData>, key: &[u16]) -> wasmtime::Result<bool> {
    let Err(err) = gate(caller, "session.read", key) else {
        return Ok(true);
    };
    // Only the policy's own answer filters. An invariant denial means the check
    // could not be made at all, and swallowing it would turn a runtime refusal
    // into a silently short listing.
    match err.downcast_ref::<crate::runtime::host::PermissionDenied>() {
        Some(denial) if denial.is_policy() => {
            crate::stdlib::shared::mark_filtered(&*caller);
            Ok(false)
        }
        _ => Err(err),
    }
}

fn build_page(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    entries: &[SessionKvEntry],
    next_cursor: Option<&str>,
) -> wasmtime::Result<Val> {
    let entry_ty = entry_backing_struct(caller.engine())?;
    let mut built = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = crate::runtime::host::write_submilli_string_struct_units(caller, &entry.key)?;
        built.push(abi::new_backing(
            caller,
            entry_ty.clone(),
            &[
                Val::AnyRef(Some(key.to_anyref())),
                Val::F64((entry.size_bytes as f64).to_bits()),
            ],
        )?);
    }
    let array = value::build_array(caller, built)?;
    let cursor = match next_cursor {
        Some(text) => {
            let units: Vec<u16> = text.encode_utf16().collect();
            let st = crate::runtime::host::write_submilli_string_struct_units(caller, &units)?;
            Val::AnyRef(Some(st.to_anyref()))
        }
        None => crate::runtime::prelude::undefined::value(caller)?,
    };
    let page_ty = page_backing_struct(caller.engine())?;
    abi::new_backing(caller, page_ty, &[array, cursor])
}

fn read_key(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
    op: &str,
) -> wasmtime::Result<Vec<u16>> {
    value::read_units(caller, val, &format!("session.{op} (key)"))
}

/// The policy sees the key as text. A lone surrogate is not expressible in a
/// JSON string, so the context renders it lossily — matching on such a key is
/// not something a rule can express, and the store keeps the exact units.
fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    capability: &str,
    key: &[u16],
) -> wasmtime::Result<Option<CallTicket>> {
    check_security_call(
        caller,
        capability,
        serde_json::json!({ "key": String::from_utf16_lossy(key) }),
    )
}

/// Clone the provider out of the store before any `await` or guest re-entry:
/// the borrow on `caller.data()` cannot be held across either.
fn provider(
    caller: &wasmtime::Caller<'_, StoreData>,
    op: &str,
) -> wasmtime::Result<Arc<dyn SessionKvStore>> {
    caller.data().session_kv.clone().ok_or_else(|| {
        crate::runtime::host::type_error(format!(
            "session.{op}: this runtime has no session store configured, so session state \
             cannot be read or written. The embedder installs one; nothing in the program \
             can create it."
        ))
    })
}

/// Both cursor paths — minting one and reading one back — map through here, so
/// a caller cannot tell the two apart by the shape of the failure. No variant
/// names a key, so the message passes through whole.
fn cursor_trap(error: cursor::CursorError) -> wasmtime::Error {
    match error {
        cursor::CursorError::Internal(_) => {
            wasmtime::Error::new(wasmtime::Trap::UnreachableCodeReached).context(error.to_string())
        }
        cursor::CursorError::Malformed
        | cursor::CursorError::PrefixMismatch
        | cursor::CursorError::NoEntropy => crate::runtime::host::type_error(error.to_string()),
    }
}

/// Every store failure reaches the guest as a catchable error. The `Display`
/// impl already excludes the stored value, so the message can be passed
/// through whole.
///
/// A write-quota refusal continues the `session.write` call `ticket` names; any other
/// operation passes `None`, so the refusal is recorded as a call of its own.
fn trap(
    caller: &wasmtime::Caller<'_, StoreData>,
    ticket: Option<CallTicket>,
    error: &SessionKvError,
) -> wasmtime::Error {
    match error {
        SessionKvError::InvalidKey { .. } => crate::runtime::host::type_error(error.to_string()),
        SessionKvError::LimitExceeded {
            limit:
                crate::runtime::session_kv::SessionKvLimitKind::KeyUnits { .. }
                | crate::runtime::session_kv::SessionKvLimitKind::ValueBytes { .. },
            ..
        } => crate::runtime::host::range_error(error.to_string()),
        SessionKvError::LimitExceeded { .. } => {
            let who = crate::stdlib::shared::running_package(caller)
                .or_else(crate::stdlib::shared::PrincipalError::label_or_error);
            let who = match who {
                Ok(who) => who,
                Err(error) => return error,
            };
            crate::stdlib::shared::audit_denial_in(
                caller,
                ticket,
                &who,
                "session.write",
                &serde_json::json!({}),
                "quota",
                "session-state budget exceeded",
            );
            crate::runtime::host::quota_exceeded_error(error.to_string())
        }
        SessionKvError::Backend { .. } => wasmtime::Error::msg(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::Engine as _;

    use super::SessionKvStore;
    use crate::runtime::session_kv::{InMemorySessionKv, SessionKvLimits};
    use crate::runtime::{
        CheckOutcome, RuntimeConfig, SecurityCheck, StoreData, Vfs, dispatch_main_async,
        install_runtime_async,
    };

    async fn run(source: &str, kv: Option<Arc<dyn SessionKvStore>>) -> wasmtime::Result<()> {
        run_with_policy(source, kv, None).await
    }

    async fn run_with_policy(
        source: &str,
        kv: Option<Arc<dyn SessionKvStore>>,
        policy: Option<Arc<dyn SecurityCheck>>,
    ) -> wasmtime::Result<()> {
        run_returning(source, kv, policy).await.map(|_| ())
    }

    /// `main`'s output, for the tests that assert on what the program observed
    /// rather than only that it completed.
    async fn run_returning(
        source: &str,
        kv: Option<Arc<dyn SessionKvStore>>,
        policy: Option<Arc<dyn SecurityCheck>>,
    ) -> wasmtime::Result<String> {
        let compiled = crate::compile_script(source, "test.ts", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.session_kv = kv;
        if let Some(policy) = policy {
            data.security_check = policy;
        }
        let mut store = cfg.store_async(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
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

    #[tokio::test]
    async fn top_level_undefined_does_not_replace_stored_value() {
        let kv = Arc::new(InMemorySessionKv::default());
        let key: Vec<u16> = "kept".encode_utf16().collect();
        let original: Vec<u16> = r#"{"step":1}"#.encode_utf16().collect();
        kv.set(&key, &original).expect("seed stored value");

        let error = run(
            r#"
            import session from "submilli:session";
            function main(): void {
                session.set("kept", undefined);
            }
            "#,
            Some(kv.clone()),
        )
        .await
        .expect_err("top-level undefined has no stored JSON representation");

        assert!(format!("{error:#}").contains("top-level undefined"));
        assert_eq!(kv.get(&key).expect("read original"), Some(original));
    }

    #[tokio::test]
    async fn nested_undefined_uses_json_object_and_array_rules() {
        let kv = Arc::new(InMemorySessionKv::default());
        run(
            r#"
            import session from "submilli:session";
            function main(): void {
                const items: unknown[] = [1, undefined, null];
                session.set("nested", {
                    kept: 1,
                    omitted: undefined,
                    inner: { omitted: undefined, kept: null },
                    items,
                });
            }
            "#,
            Some(kv.clone()),
        )
        .await
        .expect("nested undefined has JSON object and array representations");

        let key: Vec<u16> = "nested".encode_utf16().collect();
        let payload = kv
            .get(&key)
            .expect("read stored JSON")
            .expect("stored value");
        let text = String::from_utf16(&payload).expect("valid JSON text");
        let actual: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(
            actual,
            serde_json::json!({
                "kept": 1,
                "inner": { "kept": null },
                "items": [1, null, null],
            })
        );
    }

    #[tokio::test]
    async fn missing_value_is_undefined_and_stored_null_is_preserved() {
        run(
            r#"
            import session from "submilli:session";
            function main(): void {
                const missing = session.get<string>("missing");
                assert(missing === undefined, "missing keys yield undefined");
                session.set("null", null);
                const stored = session.get<null>("null");
                assert(stored === null, "stored null stays null");
                assert(session.has("null"), "stored null is present");
                assert(!session.has("missing"), "missing key stays absent");
            }
            "#,
            Some(Arc::new(InMemorySessionKv::default())),
        )
        .await
        .expect("undefined and null remain distinct");
    }

    #[tokio::test]
    async fn narrowed_host_carriers() {
        let source = r#"
import session from "submilli:session";
import { Entry, Page } from "submilli:session";
import { info } from "submilli:fs";

class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
function rejects(read: () => void): void {
 let caught = false;
 try { read(); } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "unrelated carrier must throw TypeError");
}

class PageField extends Parent { value: Page | null = null; }
class EntryField extends Parent { value: Entry | null = null; }
function main(): void {
 session.set("key", "value");
 const p = new PageField(); p.reset(session.list("", 10));
 assert(p.value!.entries.length === 1, "Page");
 const e = new EntryField(); e.reset(p.value!.entries[0]);
 assert(e.value!.key === "key", "Entry");
 p.reset(e.value); rejects(() => { const v = p.value; });
 e.reset(info()); rejects(() => { const v = e.value; });
}
"#;
        run(source, Some(Arc::new(InMemorySessionKv::default())))
            .await
            .expect("host guards");
    }

    /// An unconfigured runtime must refuse rather than fabricate a store: a
    /// program that silently wrote into per-run state would look like it
    /// persisted and lose everything.
    #[tokio::test]
    async fn a_runtime_without_a_provider_refuses_every_op() {
        for op in [
            r#"session.set("k", 1)"#,
            r#"const _ = session.get("k")"#,
            r#"const _ = session.has("k")"#,
            r#"const _ = session.remove("k")"#,
        ] {
            let source = format!(
                "import session from \"submilli:session\";\n\
                 function main(): void {{ {op}; }}\n"
            );
            let err = run(&source, None).await.expect_err("must refuse");
            let message = format!("{err}");
            assert!(
                message.contains("no session store configured"),
                "{op}: {message}"
            );
            assert!(
                message.contains("The embedder installs one"),
                "the message must name who can fix it; got: {message}"
            );
        }
    }

    /// The refusal is catchable, not an uncatchable trap — a program can fall
    /// back to running without session state.
    #[tokio::test]
    async fn the_missing_provider_refusal_is_catchable() {
        let source = r#"
            import session from "submilli:session";

            function main(): void {
                let caught = false;
                try {
                    session.set("k", 1);
                } catch (e: Error) {
                    caught = true;
                }
                assert(caught, "the refusal is catchable");
            }
        "#;
        run(source, None).await.expect("program completes");
    }

    #[tokio::test]
    async fn session_budgets_throw_quota_exceeded_error() {
        use crate::runtime::session_kv::SharedKvBudget;

        let defaults = SessionKvLimits::default();
        let stores = [
            InMemorySessionKv::new(SessionKvLimits {
                max_entries: 0,
                ..defaults
            }),
            InMemorySessionKv::new(SessionKvLimits {
                max_session_bytes: 1,
                ..defaults
            }),
            InMemorySessionKv::with_shared_budget(defaults, SharedKvBudget::new(1)),
        ];
        for kv in stores {
            let source = r#"
                import session from "submilli:session";
                function main(): void {
                    let caught = false;
                    try { session.set("k", "value"); }
                    catch (e: QuotaExceededError) {
                        caught = e instanceof Error && !((e as unknown) instanceof RangeError);
                        assert(e.name === "QuotaExceededError", "quota name");
                    }
                    assert(caught, "session budget refusal is a quota error");
                    assert(!session.has("k"), "refused write leaves no value");
                }
            "#;
            run(source, Some(Arc::new(kv)))
                .await
                .expect("catch quota refusal");
        }
    }

    #[tokio::test]
    async fn session_argument_bounds_remain_range_errors() {
        let defaults = SessionKvLimits::default();
        for limits in [
            SessionKvLimits {
                max_key_units: 1,
                ..defaults
            },
            SessionKvLimits {
                max_value_bytes: 1,
                ..defaults
            },
        ] {
            let source = r#"
                import session from "submilli:session";
                function main(): void {
                    let caught = false;
                    try { session.set("key", "value"); }
                    catch (e: RangeError) {
                        caught = !((e as unknown) instanceof QuotaExceededError);
                    }
                    assert(caught, "individual key/value caps remain argument errors");
                }
            "#;
            run(source, Some(Arc::new(InMemorySessionKv::new(limits))))
                .await
                .expect("catch argument bounds");
        }
    }

    /// A quota refusal names the limit and the key, and never the value — the
    /// property `SessionKvError` maintains must survive the mapping to a trap.
    #[tokio::test]
    async fn a_limit_refusal_never_carries_the_stored_value() {
        let kv: Arc<dyn SessionKvStore> = Arc::new(InMemorySessionKv::new(SessionKvLimits {
            max_value_bytes: 8,
            ..SessionKvLimits::default()
        }));
        let source = r#"
            import session from "submilli:session";

            function main(): void {
                session.set("k", "SUPER_SECRET_PAYLOAD");
            }
        "#;
        let err = run(source, Some(kv)).await.expect_err("over the limit");
        let message = format!("{err}");
        assert!(message.contains("value size limit"), "{message}");
        assert!(
            message.contains("\"k\""),
            "the key names the entry: {message}"
        );
        assert!(
            !message.contains("SUPER_SECRET_PAYLOAD"),
            "the value must never reach a diagnostic: {message}"
        );
    }

    /// The per-key filter the catalog's `key glob "triage/*"` example describes:
    /// `session.read` allowed for some keys and denied for others. The fixture
    /// harness denies a whole capability, so only a context-reading policy can
    /// exercise the selective case.
    struct AllowPrefix(&'static str);

    impl SecurityCheck for AllowPrefix {
        fn check(
            &self,
            _caller: &str,
            capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            if capability != "session.read" {
                return CheckOutcome::Allow { rule: None };
            }
            let key = context.get("key").and_then(|k| k.as_str()).unwrap_or("");
            if key.starts_with(self.0) {
                CheckOutcome::Allow { rule: None }
            } else {
                CheckOutcome::Deny {
                    rule: None,
                    reason: format!("only {} is readable", self.0),
                }
            }
        }
    }

    /// Denied keys must be invisible in every channel a listing has: the
    /// entries, the count, and the cursor. A cursor that pointed past a hidden
    /// key would let a caller detect that something sat between two keys it can
    /// see.
    #[tokio::test]
    async fn a_filtered_listing_reveals_neither_the_hidden_keys_nor_their_count() {
        let source = r#"
            import session from "submilli:session";

            function main(): void {
                session.set("aaa/1", 1);
                session.set("triage/a", 1);
                session.set("triage/b", 2);
                session.set("zzz/9", 9);

                // A page sized to the whole keyspace: every key is a candidate,
                // so anything the policy hides can only vanish silently.
                const all = session.list("", 100);
                assert(all.entries.length === 2, "only the permitted keys are counted");
                assert(all.entries[0].key === "triage/a", "first permitted key");
                assert(all.entries[1].key === "triage/b", "second permitted key");
                assert(all.nextCursor === undefined, "the cursor does not point past hidden keys");

                // Listing a wholly denied prefix is empty and final, not an
                // error: a throw would itself confirm the keys exist.
                const denied = session.list("zzz/", 10);
                assert(denied.entries.length === 0, "a denied prefix lists as empty");
                assert(denied.nextCursor === undefined, "and reports itself finished");
            }
        "#;
        let kv: Arc<dyn SessionKvStore> = Arc::new(InMemorySessionKv::default());
        run_with_policy(source, Some(kv), Some(Arc::new(AllowPrefix("triage/"))))
            .await
            .expect("program completes");
    }

    /// The exfiltration a decodable cursor allowed, run the way a guest would.
    ///
    /// Paging at `limit: 1` makes every cursor resume after exactly one key, and
    /// the keys considered include the ones the filter hid — so if a cursor can
    /// be read, the denied key names come back in order. The listing itself is
    /// correct here and always was: it shows only `triage/*`. What this asserts
    /// is that the cursors it hands out carry nothing further.
    #[tokio::test]
    async fn cursors_from_a_filtered_listing_do_not_carry_the_denied_keys() {
        let source = r#"
            import session from "submilli:session";

            function main(): string {
                session.set("secret/alpha", 1);
                session.set("triage/a", 1);
                session.set("zzz-denied", 1);

                let cursors = "";
                let shown = 0;
                let cursor: string | undefined;
                let guard = 0;
                while (guard < 10) {
                    guard = guard + 1;
                    const page = session.list("", 1, cursor);
                    shown = shown + page.entries.length;
                    const next = page.nextCursor;
                    if (next === undefined) { break; }
                    cursors = cursors + next;
                    cursor = next;
                }
                assert(shown === 1, "only the permitted key is listed");
                return cursors;
            }
        "#;
        let kv: Arc<dyn SessionKvStore> = Arc::new(InMemorySessionKv::default());
        let cursors = run_returning(source, Some(kv), Some(Arc::new(AllowPrefix("triage/"))))
            .await
            .expect("program completes");

        // The cursors are base64url, which a guest decodes as easily as we do.
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursors.as_bytes())
            .unwrap_or_default();
        for denied in ["secret/alpha", "zzz-denied"] {
            let utf16: Vec<u8> = denied.encode_utf16().flat_map(u16::to_be_bytes).collect();
            assert!(
                !cursors.contains(denied),
                "a denied key is spelled in a cursor: {cursors}"
            );
            assert!(
                !decoded.windows(utf16.len()).any(|w| w == utf16.as_slice()),
                "a denied key is recoverable from a cursor: {cursors}"
            );
        }
    }

    /// `list` discloses metadata only. The value is reachable through `get`,
    /// which is gated per key on its own — a listing must not be a way around
    /// that.
    #[tokio::test]
    async fn a_listing_carries_no_value_contents() {
        let kv: Arc<dyn SessionKvStore> = Arc::new(InMemorySessionKv::default());
        let source = r#"
            import session from "submilli:session";

            function main(): string {
                session.set("k", "SUPER_SECRET_PAYLOAD");
                const page = session.list("", 10);
                let rendered = "";
                for (const entry of page.entries) {
                    rendered = rendered + entry.key + ":" + entry.sizeBytes.toString();
                }
                // 20 characters plus the two JSON quotes, two bytes per UTF-16
                // code unit.
                assert(rendered === "k:44", "the entry carries the key and the size only");
                return JSON.stringify(page.entries);
            }
        "#;
        run(source, Some(kv)).await.expect("program completes");
    }
}
