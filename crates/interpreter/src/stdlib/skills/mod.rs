//! `submilli:skills` — read the skills a harness offers from inside a Submilli
//! program.
//!
//! An optional package: it exists only for an embedder that enables
//! [`OptionalPackage::Skills`](crate::stdlib::OptionalPackage::Skills). The
//! harness serves skills through [`StoreData::skill_provider`]; a runtime with
//! none refuses every op with a catchable error.
//!
//! **One capability.** `list()`, `load` and `readFile` are all `skill.load`,
//! filtered by `name`. `list()` gates each candidate with the same filter, so a
//! listing never offers a skill the caller would be denied.

pub mod declaration;

use std::sync::Arc;

use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::call_log::{Payload, Side, record_payload};
use crate::runtime::decision::CallTicket;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, abi_result, range_error, read_string_arg, register_host_fn_async,
    write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::skills::{Skill, SkillError, SkillInfo, SkillProvider};
use crate::stdlib::abi::{
    self, backing_struct, install_field_getters, nullable_object_field, string_field,
};
use crate::stdlib::shared::{
    check_security_call, permitted_candidates, preflight_listing, sanitize_description, truncated,
};

pub use declaration::package_declaration;

pub const MODULE_NAME: &str = crate::runtime::skills::SKILLS_MODULE_NAME;

/// The capability gating every op in this package.
pub const CAPABILITY: &str = "skill.load";

// Backing field indices (0 is the vtable). `Skill` extends `SkillInfo`'s layout.
const S_NAME: usize = 1;
const S_DESCRIPTION: usize = 2;
const S_CONTENT: usize = 3;

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "list"),
        FuncType::new(&engine, [], [array]),
        /* deterministic = */ false,
        |caller, _params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = list(caller).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "load"),
        // A `Skill` crosses as the universal `(ref null $Object)` lowering.
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let name =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "skills.load (name)")?;
                *abi_result(results, 0)? = load(caller, name).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "readFile"),
        FuncType::new(&engine, [string.clone(), string.clone()], [string.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let name =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "skills.readFile (name)")?;
                let path =
                    read_string_arg(&mut *caller, abi_arg(params, 1)?, "skills.readFile (path)")?;
                *abi_result(results, 0)? = read_file(caller, name, path).await?;
                Ok(())
            })
        },
    )?;

    let info_rows = [
        ("name", S_NAME, string.clone()),
        ("description", S_DESCRIPTION, nullable_object),
    ];
    install_field_getters(
        linker,
        MODULE_NAME,
        "SkillInfo",
        &engine,
        &object,
        &info_rows,
    )?;
    let mut skill_rows = info_rows.to_vec();
    skill_rows.push(("content", S_CONTENT, string));
    install_field_getters(linker, MODULE_NAME, "Skill", &engine, &object, &skill_rows)
}

/// The capability check, run before anything reaches the harness.
fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    name: &str,
) -> wasmtime::Result<Option<CallTicket>> {
    check_security_call(caller, CAPABILITY, serde_json::json!({ "name": name }))
}

/// `list()`: every skill the harness offers that the caller may load. A denied
/// candidate is left out, never reported. Whether a provider is wired is not
/// hidden here: a listing has no candidate to gate before asking it.
async fn list(caller: &mut wasmtime::Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    preflight_listing(caller, CAPABILITY, &serde_json::json!({ "name": "" }))?;
    let provider = provider(caller).map_err(|error| throw("list", &error))?;
    let candidates = provider.list().await.map_err(|e| throw("list", &e))?;
    // A name `load` would refuse is no use to the program, so it is not offered.
    let loadable = candidates
        .into_iter()
        .filter(|skill| is_skill_name(&skill.name))
        .collect();
    let visible = permitted_candidates(
        caller,
        loadable,
        |skill| skill.name.as_str(),
        |caller, name| gate(caller, name).map(|_| ()),
    )?;
    let mut built = Vec::with_capacity(visible.len());
    for skill in visible {
        built.push(build_skill_info(caller, skill)?);
    }
    abi::new_array(caller, &built)
}

/// `load(name)`: gate, refuse what cannot be sent, record the request, then ask
/// the provider. A refusal before the provider is asked is recorded as the
/// call's error, with no request.
async fn load(caller: &mut wasmtime::Caller<'_, StoreData>, name: String) -> wasmtime::Result<Val> {
    let ticket = gate(caller, &name)?;
    if !is_skill_name(&name) {
        return Err(fail(
            caller,
            ticket,
            "load",
            &SkillError::InvalidName { name },
        ));
    }
    let provider = provider(caller).map_err(|error| fail(caller, ticket, "load", &error))?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "load", "name": name }))
    });
    match provider.load(&name).await {
        // The policy decided on `name`; a provider that answered with another
        // skill would hand the program one the policy never saw.
        Ok(skill) if skill.name != name => {
            // The read happened, so it is paid for; its content is not kept.
            fuel::settle(&mut *caller, fuel::IO, skill.content.len() as u64)?;
            Err(fail(
                caller,
                ticket,
                "load",
                &SkillError::WrongSkill { name },
            ))
        }
        Ok(skill) => {
            record_text(caller, ticket, &skill.content);
            fuel::settle(&mut *caller, fuel::IO, skill.content.len() as u64)?;
            fuel::settle_result(caller, |caller| build_skill(caller, skill))
        }
        Err(error) => Err(fail(caller, ticket, "load", &error)),
    }
}

/// `readFile(name, path)`: in the order `load` follows, with the path checked
/// beside the name.
async fn read_file(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    name: String,
    path: String,
) -> wasmtime::Result<Val> {
    let ticket = gate(caller, &name)?;
    if !is_skill_name(&name) {
        return Err(fail(
            caller,
            ticket,
            "readFile",
            &SkillError::InvalidName { name },
        ));
    }
    if !is_skill_path(&path) {
        return Err(fail(
            caller,
            ticket,
            "readFile",
            &SkillError::InvalidPath { path },
        ));
    }
    let provider = provider(caller).map_err(|error| fail(caller, ticket, "readFile", &error))?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "readFile", "name": name, "path": path }))
    });
    match provider.read_file(&name, &path).await {
        Ok(text) => {
            record_text(caller, ticket, &text);
            fuel::settle(&mut *caller, fuel::IO, text.len() as u64)?;
            fuel::settle_result(caller, |caller| {
                let text = write_submilli_string_struct(caller, &text)?;
                Ok(Val::AnyRef(Some(text.to_anyref())))
            })
        }
        Err(error) => Err(fail(caller, ticket, "readFile", &error)),
    }
}

/// The longest skill name, and the longest path, the engine passes to a
/// provider.
const MAX_SKILL_ARG_BYTES: usize = 4096;

/// A skill name: one segment that is not `.` or `..`, with no `/`, `\\` or NUL.
/// `:` is allowed, for namespaced names such as `plugin:skill`; a provider must
/// not read a name as a path.
fn is_skill_name(name: &str) -> bool {
    name.len() <= MAX_SKILL_ARG_BYTES
        && !name.contains(['/', '\\', '\0'])
        && !matches!(name, "" | "." | "..")
}

/// A path inside a skill: relative, `/`-separated, with no empty, `.` or `..`
/// segment, and none of the characters a provider could read as structure — a
/// backslash or `:` (Windows separators, drives and streams) or NUL.
fn is_skill_path(path: &str) -> bool {
    path.len() <= MAX_SKILL_ARG_BYTES
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn record_text(caller: &wasmtime::Caller<'_, StoreData>, ticket: Option<CallTicket>, text: &str) {
    record_payload(caller, ticket, Side::Response, || {
        Payload::meta(serde_json::Value::Null)
            .with_owned_body(text.as_bytes().to_vec())
            .with_size(text.len() as u64)
    });
}

/// Record `error` as the call's response and turn it into the guest's error.
fn fail(
    caller: &wasmtime::Caller<'_, StoreData>,
    ticket: Option<CallTicket>,
    op: &str,
    error: &SkillError,
) -> wasmtime::Error {
    record_payload(caller, ticket, Side::Response, || {
        Payload::meta(serde_json::json!({ "call_error": call_error_record(error) }))
    });
    throw(op, error)
}

/// Clone the provider out of the store before any `await`. `load` and
/// `readFile` look it up after the capability check, so a denied caller cannot
/// learn whether a harness is wired.
fn provider(
    caller: &wasmtime::Caller<'_, StoreData>,
) -> Result<Arc<dyn SkillProvider>, SkillError> {
    caller
        .data()
        .skill_provider
        .clone()
        .ok_or(SkillError::NotConfigured)
}

/// A name or path outside the skills is an argument the program got wrong; every
/// other failure keeps the base error type.
fn throw(op: &str, error: &SkillError) -> wasmtime::Error {
    let message = format!("skills.{op}: {error}");
    if matches!(
        error,
        SkillError::InvalidName { .. } | SkillError::InvalidPath { .. }
    ) {
        range_error(message)
    } else {
        wasmtime::Error::msg(message)
    }
}

/// A failure as the call log keeps it: a stable kind and what it was about.
fn call_error_record(error: &SkillError) -> serde_json::Value {
    match error {
        SkillError::NotConfigured => serde_json::json!({ "kind": "not-configured" }),
        SkillError::NotFound { name } => serde_json::json!({ "kind": "not-found", "name": name }),
        SkillError::FileNotFound { name, path } => {
            serde_json::json!({ "kind": "file-not-found", "name": name, "path": path })
        }
        SkillError::InvalidName { name } => {
            serde_json::json!({ "kind": "invalid-name", "name": truncated(name) })
        }
        SkillError::WrongSkill { name } => {
            serde_json::json!({ "kind": "wrong-skill", "name": name })
        }
        SkillError::InvalidPath { path } => {
            serde_json::json!({ "kind": "invalid-path", "path": truncated(path) })
        }
        SkillError::Failed { message } => {
            serde_json::json!({ "kind": "failed", "message": message })
        }
    }
}

fn info_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![string_field(&intr), nullable_object_field(&intr)],
    )
}

fn skill_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr),
            nullable_object_field(&intr),
            string_field(&intr),
        ],
    )
}

fn build_skill_info(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    skill: SkillInfo,
) -> wasmtime::Result<Val> {
    let name = write_submilli_string_struct(caller, &skill.name)?;
    let description = listing_description(caller, skill.description.as_deref())?;
    let ty = info_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[Val::AnyRef(Some(name.to_anyref())), description],
    )
}

fn build_skill(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    skill: Skill,
) -> wasmtime::Result<Val> {
    let name = write_submilli_string_struct(caller, &skill.name)?;
    let description = optional_text(caller, skill.description.as_deref())?;
    let content = write_submilli_string_struct(caller, &skill.content)?;
    let ty = skill_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(name.to_anyref())),
            description,
            Val::AnyRef(Some(content.to_anyref())),
        ],
    )
}

/// A listing shows a description as one bounded line, because it steers which
/// skill a program picks.
fn listing_description(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    text: Option<&str>,
) -> wasmtime::Result<Val> {
    optional_text(caller, text.and_then(sanitize_description).as_deref())
}

/// `text` as a string, or `undefined` when absent.
fn optional_text(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    text: Option<&str>,
) -> wasmtime::Result<Val> {
    match text {
        Some(text) => Ok(Val::AnyRef(Some(
            write_submilli_string_struct(caller, text)?.to_anyref(),
        ))),
        None => crate::runtime::prelude::undefined::value(caller),
    }
}

#[cfg(test)]
mod path_tests {
    use super::{MAX_SKILL_ARG_BYTES, is_skill_name, is_skill_path};
    use crate::stdlib::shared::truncated;

    #[test]
    fn only_relative_paths_inside_the_skill_are_accepted() {
        let longest = "a".repeat(MAX_SKILL_ARG_BYTES);
        for ok in [
            "SKILL.md",
            "templates/report.md",
            "a/b/c.txt",
            ".hidden",
            "notes/résumé ✓.md",
            "%2e%2e/x",
            longest.as_str(),
        ] {
            assert!(is_skill_path(ok), "{ok}");
        }
        let too_long = "a".repeat(MAX_SKILL_ARG_BYTES + 1);
        // 4095 ASCII bytes and a two-byte character: 4097 bytes, 4096 chars.
        let straddling = format!("{}é", "a".repeat(MAX_SKILL_ARG_BYTES - 1));
        for bad in [
            "",
            "/etc/passwd",
            "a//b",
            "./a",
            "a/.",
            "../x",
            "a/../b",
            "a/",
            "a\\b",
            "a\0b",
            "a:b",
            "C:/x",
            "C:x",
            "SKILL.md:stream",
            too_long.as_str(),
            straddling.as_str(),
        ] {
            assert!(!is_skill_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_skill_name_is_one_segment_and_may_be_namespaced() {
        let longest = "n".repeat(MAX_SKILL_ARG_BYTES);
        for ok in [
            "code-review",
            "code.review",
            "plugin:skill",
            "C:",
            longest.as_str(),
        ] {
            assert!(is_skill_name(ok), "{ok}");
        }
        let too_long = "n".repeat(MAX_SKILL_ARG_BYTES + 1);
        for bad in [
            "",
            ".",
            "..",
            "../other",
            "a/b",
            "a\\b",
            "a\0b",
            too_long.as_str(),
        ] {
            assert!(!is_skill_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn an_echoed_value_is_cut_on_a_character_boundary() {
        assert_eq!(truncated("short"), "short");
        let exactly = "a".repeat(200);
        assert_eq!(truncated(&exactly), exactly.as_str());
        // Three-byte characters: byte 200 falls inside one, so the cut is at 198.
        let cut = truncated(&"€".repeat(100)).into_owned();
        assert_eq!(cut, format!("{}…", "€".repeat(66)));
    }
}
