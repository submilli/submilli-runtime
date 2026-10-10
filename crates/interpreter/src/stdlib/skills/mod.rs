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
    check_security_call, filters_candidate, mark_filtered, preflight_listing, sanitize_description,
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

    for iface in ["SkillInfo", "Skill"] {
        let mut rows = vec![
            ("name", S_NAME, string.clone()),
            ("description", S_DESCRIPTION, nullable_object.clone()),
        ];
        if iface == "Skill" {
            rows.push(("content", S_CONTENT, string.clone()));
        }
        install_field_getters(linker, MODULE_NAME, iface, &engine, &object, &rows)?;
    }
    Ok(())
}

/// The capability check, run before anything reaches the harness.
fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    name: &str,
) -> wasmtime::Result<Option<CallTicket>> {
    check_security_call(caller, CAPABILITY, serde_json::json!({ "name": name }))
}

/// `list()`: every skill the harness offers that the caller may load. A denied
/// candidate is left out, never reported.
async fn list(caller: &mut wasmtime::Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    preflight_listing(caller, CAPABILITY, &serde_json::json!({ "name": "" }))?;
    let provider = provider(caller, "list")?;
    let candidates = provider.list().await.map_err(|e| throw("list", &e))?;
    let mut visible = Vec::new();
    for candidate in candidates {
        let keeps = filters_candidate(gate(caller, &candidate.name).map(|_| ()))?;
        if keeps {
            visible.push(candidate);
        } else {
            mark_filtered(&*caller);
        }
    }
    let mut built = Vec::with_capacity(visible.len());
    for skill in visible {
        built.push(build_skill_info(caller, skill)?);
    }
    abi::new_array(caller, &built)
}

async fn load(caller: &mut wasmtime::Caller<'_, StoreData>, name: String) -> wasmtime::Result<Val> {
    let ticket = gate(caller, &name)?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "load", "name": name }))
    });
    let provider = provider(caller, "load")?;
    match provider.load(&name).await {
        Ok(skill) => {
            record_text(caller, ticket, &skill.content);
            fuel::settle(&mut *caller, fuel::IO, skill.content.len() as u64)?;
            fuel::settle_result(caller, |caller| build_skill(caller, skill))
        }
        Err(error) => Err(fail(caller, ticket, "load", &error)),
    }
}

async fn read_file(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    name: String,
    path: String,
) -> wasmtime::Result<Val> {
    let ticket = gate(caller, &name)?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "readFile", "name": name, "path": path }))
    });
    if !is_skill_path(&path) {
        return Err(fail(
            caller,
            ticket,
            "readFile",
            &SkillError::InvalidPath { path },
        ));
    }
    let provider = provider(caller, "readFile")?;
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

/// A path inside a skill: relative, `/`-separated, with no empty, `.` or `..`
/// segment, and no backslash or NUL a provider could read as structure.
fn is_skill_path(path: &str) -> bool {
    !path.contains(['\\', '\0'])
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
        Payload::meta(serde_json::json!({ "call_error": error_record(error) }))
    });
    throw(op, error)
}

/// Clone the provider out of the store before any `await`. Runs after the
/// capability check, so a denied caller cannot learn whether a harness is wired.
fn provider(
    caller: &wasmtime::Caller<'_, StoreData>,
    op: &str,
) -> wasmtime::Result<Arc<dyn SkillProvider>> {
    caller
        .data()
        .skill_provider
        .clone()
        .ok_or_else(|| throw(op, &SkillError::NotConfigured))
}

/// A path outside the skill is an argument the program got wrong; every other
/// failure keeps the base error type.
fn throw(op: &str, error: &SkillError) -> wasmtime::Error {
    let message = format!("skills.{op}: {error}");
    if matches!(error, SkillError::InvalidPath { .. }) {
        range_error(message)
    } else {
        wasmtime::Error::msg(message)
    }
}

fn error_record(error: &SkillError) -> serde_json::Value {
    match error {
        SkillError::NotConfigured => serde_json::json!({ "kind": "not-configured" }),
        SkillError::NotFound { name } => serde_json::json!({ "kind": "not-found", "name": name }),
        SkillError::FileNotFound { name, path } => {
            serde_json::json!({ "kind": "file-not-found", "name": name, "path": path })
        }
        SkillError::InvalidPath { path } => {
            serde_json::json!({ "kind": "invalid-path", "path": path })
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
    let description = description(caller, skill.description.as_deref(), true)?;
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
    let description = description(caller, skill.description.as_deref(), false)?;
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
/// skill a program picks; a loaded skill's description is part of what the
/// program asked to read.
fn description(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    text: Option<&str>,
    in_listing: bool,
) -> wasmtime::Result<Val> {
    let text = if in_listing {
        text.and_then(sanitize_description)
    } else {
        text.map(str::to_string)
    };
    match text {
        Some(text) => Ok(Val::AnyRef(Some(
            write_submilli_string_struct(caller, &text)?.to_anyref(),
        ))),
        None => crate::runtime::prelude::undefined::value(caller),
    }
}

#[cfg(test)]
mod path_tests {
    use super::is_skill_path;

    #[test]
    fn only_relative_paths_inside_the_skill_are_accepted() {
        for ok in ["SKILL.md", "templates/report.md", "a/b/c.txt", ".hidden"] {
            assert!(is_skill_path(ok), "{ok}");
        }
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
        ] {
            assert!(!is_skill_path(bad), "{bad:?}");
        }
    }
}
