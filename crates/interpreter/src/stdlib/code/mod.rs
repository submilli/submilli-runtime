//! Contained workspace navigation and anchor-based editing, gated by existing fs grants.
mod budget;
pub mod declaration;
mod install;
mod myers;
mod patch;
#[cfg(test)]
mod tests;
mod text;
mod walk;

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::stdlib::shared::{
    atomic_write, check_security, check_security_call, contain_trap, require_writable,
    resolve_content_or_trap,
};
use budget::{Budget, OutputBudget};
use serde_json::{Value, json};
use std::io::Read;
use wasmtime::{Caller, Result, bail};

pub const MODULE_NAME: &str = "submilli:code";
pub use declaration::package_declaration;
pub use install::install;
use walk::{glob, search, tree};

fn read(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    start: usize,
    limit: usize,
) -> Result<Value> {
    let source = read_file(caller, budget, path, "read")?;
    budget.charge(
        caller,
        source
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            .saturating_add(1)
            .saturating_mul(32),
    )?;
    let lines = text::lines(source.strip_prefix('\u{feff}').unwrap_or(&source));
    let mut output = OutputBudget::new();
    let mut values = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(start).take(limit) {
        if !output.reserve(caller, budget, OutputBudget::line_bytes(line))? {
            break;
        }
        values.push(json!({"line":index+1,"text":text::line_text(line)}));
    }
    Ok(
        json!({"path":path,"truncated":start.saturating_add(values.len()) < lines.len(),"lines":values}),
    )
}

fn edit(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    old: &str,
    new: &str,
    replace_all: bool,
    near_line: usize,
) -> Result<Value> {
    let original = read_file(caller, budget, path, "edit")?;
    budget.charge(caller, original.len().saturating_mul(64))?;
    fuel::charge(&mut *caller, fuel::SCAN, original.len() as u64)?;
    let change = prepare_edit(caller, budget, &original, old, new, replace_all, near_line)?;
    write_change(caller, budget, path, "edit", &original, change)
}

fn insert_at(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    line: usize,
    text: &str,
) -> Result<Value> {
    let original = read_file(caller, budget, path, "insertAt")?;
    budget.charge(caller, original.len().saturating_mul(64))?;
    fuel::charge(&mut *caller, fuel::SCAN, original.len() as u64)?;
    let change = text::insert(&original, line, text)?;
    write_change(caller, budget, path, "insertAt", &original, change)
}

fn diff_files(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    first: &str,
    second: &str,
) -> Result<Vec<u16>> {
    let first = read_file(caller, budget, first, "diffFiles")?;
    let second = read_file(caller, budget, second, "diffFiles")?;
    budget.charge(caller, (first.len() + second.len()).saturating_mul(2))?;
    diff_text(
        caller,
        budget,
        &first.encode_utf16().collect::<Vec<_>>(),
        &second.encode_utf16().collect::<Vec<_>>(),
    )
}

fn apply_patch(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    patch: &str,
) -> Result<Value> {
    let original = read_file(caller, budget, path, "applyPatch")?;
    budget.charge(caller, original.len().saturating_mul(64))?;
    fuel::charge(&mut *caller, fuel::SCAN, original.len() as u64)?;
    budget.charge(caller, patch.len().saturating_mul(32))?;
    fuel::charge(
        &mut *caller,
        fuel::SCAN,
        (original.len() as u64).saturating_mul(patch.lines().count().max(1) as u64),
    )?;
    fuel::charge(&mut *caller, fuel::PARSE, patch.len() as u64)?;
    let change = patch::apply(&original, patch)?;
    write_change(caller, budget, path, "applyPatch", &original, change)
}

fn write_change(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    op: &str,
    original: &str,
    change: text::Edit,
) -> Result<Value> {
    budget.check_size(change.text.len())?;
    let success = change.diagnostics.is_empty();
    let changed = success && change.text != original;
    let patch = if changed {
        String::from_utf16(&diff_text(
            caller,
            budget,
            &original.encode_utf16().collect::<Vec<_>>(),
            &change.text.encode_utf16().collect::<Vec<_>>(),
        )?)?
    } else {
        String::new()
    };
    let ticket = check_security_call(
        &mut *caller,
        "fs.write",
        json!({"path":path, "length": change.text.len(), "diff": patch}),
    )?;
    // Build the native result before commit; history captures it before guest conversion.
    let result =
        json!({"success":success,"changed":changed,"diff":patch,"diagnostics":change.diagnostics});
    budget.check_size(serde_json::to_string(&result)?.len())?;
    if changed {
        let resolved = resolve_content_or_trap(caller.data(), path, op)?;
        require_writable(&*caller, ticket, resolved.placement(), "fs.write", path)?;
        let permissions = resolved
            .metadata()
            .map_err(|e| contain_trap(op, path, &e))?
            .permissions();
        let quota = resolved.placement().quota().cloned();
        atomic_write(
            &resolved,
            change.text.as_bytes(),
            Some(permissions),
            path,
            op,
            quota,
        )?;
    }
    Ok(result)
}
fn prepare_edit(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    original: &str,
    old: &str,
    new: &str,
    replace_all: bool,
    near_line: usize,
) -> Result<text::Edit> {
    // Naive substring search: every position against the whole needle.
    fuel::charge(
        &mut *caller,
        fuel::SCAN,
        (original.len() as u64).saturating_mul(old.len().max(1) as u64),
    )?;
    if old.is_empty() {
        bail!("code.edit: oldString must not be empty; use insertAt");
    }
    let matches = original.match_indices(old).count();
    let selected = if replace_all {
        matches
    } else {
        usize::from(matches > 0)
    };
    budget.charge(
        caller,
        text::MAX_DIAGNOSTICS
            .min(original.len().saturating_add(1))
            .saturating_mul(1024),
    )?;
    let length = original
        .len()
        .saturating_sub(selected.saturating_mul(old.len()))
        .saturating_add(selected.saturating_mul(new.len()));
    budget.check_size(length)?;
    budget.charge(caller, length.saturating_mul(2))?;
    let maximum = if matches == 0 {
        budget.max_bytes.min(
            original
                .len()
                .saturating_mul(16)
                .saturating_add(new.len().saturating_mul(4)),
        )
    } else {
        budget.max_bytes
    };
    text::replace(original, old, new, replace_all, near_line, maximum)
}

fn normalize(cwd: &str, path: &str) -> Result<String> {
    crate::runtime::fs::guest_normalize(cwd, path)
        .map_err(|e| wasmtime::Error::msg(format!("code: {e}")))
}
fn gate(caller: &mut Caller<'_, StoreData>, capability: &str, path: &str) -> Result<()> {
    check_security(caller, capability, json!({"path":path,"recursive":true}))
}
fn gate_call(
    caller: &mut Caller<'_, StoreData>,
    capability: &str,
    path: &str,
) -> Result<Option<crate::runtime::CallTicket>> {
    check_security_call(caller, capability, json!({"path":path,"recursive":true}))
}
fn read_file(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    op: &str,
) -> Result<String> {
    read_contents(caller, budget, path, op, false)
}
fn read_contents(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    op: &str,
    skip_binary: bool,
) -> Result<String> {
    let ticket = gate_call(caller, "fs.read", path)?;
    let resolved = resolve_content_or_trap(caller.data(), path, op)?;
    let (file, metadata) = resolved
        .open_regular()
        .map_err(|e| contain_trap(op, path, &e))?;
    let len = usize::try_from(metadata.len())?;
    budget.check_size(len)?;
    budget.charge(caller, len.saturating_mul(8).saturating_add(1024))?;
    fuel::charge(&mut *caller, fuel::IO, len as u64)?;
    fuel::charge(&mut *caller, fuel::SCAN, len as u64)?;
    let mut bytes = Vec::with_capacity(len);
    file.take((len as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > len {
        bail!("code.{op}: file grew during read; retry");
    }
    crate::runtime::call_log::record_payload(
        &*caller,
        ticket,
        crate::runtime::call_log::Side::Response,
        || crate::runtime::call_log::Payload::meta(Value::Null).with_body(&bytes),
    );
    if skip_binary && bytes.contains(&0) {
        return Ok(String::new());
    }
    String::from_utf8(bytes)
        .map_err(|_| wasmtime::Error::msg(format!("code.{op}: {path} is not valid UTF-8")))
}
fn diff_text(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    a: &[u16],
    b: &[u16],
) -> Result<Vec<u16>> {
    budget.charge(caller, (a.len() + b.len()).saturating_mul(16))?;
    let na = text::line_count(a);
    let nb = text::line_count(b);
    budget.charge(caller, (na + nb).saturating_mul(1024))?;
    text::check_diff_size(na, nb)?;
    fuel::charge(&mut *caller, fuel::SCAN, (a.len() + b.len()) as u64)?;
    let result = text::diff_charged(a, b, |steps| fuel::charge(&mut *caller, fuel::PARSE, steps))?;
    budget.check_size(result.len().saturating_mul(2))?;
    Ok(result)
}
