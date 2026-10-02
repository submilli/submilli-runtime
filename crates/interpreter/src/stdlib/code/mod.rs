//! Contained workspace navigation and anchor-based editing, gated by existing fs grants.
mod budget;
pub mod declaration;
mod patch;
#[cfg(test)]
mod tests;
mod text;
mod walk;

use crate::runtime::fuel;
use crate::runtime::{
    StoreData,
    host::{register_host_fn, write_submilli_string_struct_units},
    intrinsic_types::build_intrinsic_types,
    prelude::vtable::read_string_units,
};
use crate::stdlib::shared::{
    atomic_write, check_security, contain_trap, require_writable, resolve_content_or_trap,
};
use budget::{Budget, OutputBudget};
use serde_json::{Value, json};
use std::io::Read;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Result, Val, ValType, bail};
pub const MODULE_NAME: &str = "submilli:code";
pub use declaration::package_declaration;

pub fn install(linker: &mut Linker<StoreData>) -> Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object_shape),
    ));
    for (name, params, result) in [
        (
            "read",
            vec![string.clone(), ValType::F64, ValType::F64],
            object.clone(),
        ),
        ("search", vec![string.clone(), options], object.clone()),
        ("glob", vec![string.clone()], object.clone()),
        ("tree", vec![string.clone(), ValType::F64], object.clone()),
        (
            "edit",
            vec![
                string.clone(),
                string.clone(),
                string.clone(),
                ValType::I32,
                ValType::F64,
            ],
            object.clone(),
        ),
        (
            "insertAt",
            vec![string.clone(), ValType::F64, string.clone()],
            object.clone(),
        ),
        (
            "diffText",
            vec![string.clone(), string.clone()],
            string.clone(),
        ),
        (
            "diffFiles",
            vec![string.clone(), string.clone()],
            string.clone(),
        ),
        ("applyPatch", vec![string.clone(), string], object),
    ] {
        register_host_fn(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, name),
            FuncType::new(&engine, params, [result]),
            name == "diffText",
            move |caller, params, results| {
                let mut budget = Budget::new(caller);
                results[0] = invoke(caller, &mut budget, name, params)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}
fn invoke(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    op: &str,
    params: &[Val],
) -> Result<Val> {
    if op == "diffText" {
        let a = units(caller, budget, &params[0])?;
        let b = units(caller, budget, &params[1])?;
        let result = diff(caller, budget, &a, &b)?;
        return Ok(Val::AnyRef(Some(
            write_submilli_string_struct_units(caller, &result)?.to_anyref(),
        )));
    }
    let first = argument(caller, budget, &params[0])?;
    let result = match op {
        "search" => walk::search(caller, budget, &first, &params[1])?,
        "glob" => walk::glob(caller, budget, &first)?,
        "tree" => walk::tree(
            caller,
            budget,
            &normalize(&first)?,
            integer(&params[1], "depth", 0)?,
        )?,
        "read" => read_window(caller, budget, &normalize(&first)?, params)?,
        "diffFiles" => {
            let second = argument(caller, budget, &params[1])?;
            let a = read_file(caller, budget, &normalize(&first)?, op)?;
            let b = read_file(caller, budget, &normalize(&second)?, op)?;
            budget.charge(caller, (a.len() + b.len()).saturating_mul(2))?;
            let result = diff(
                caller,
                budget,
                &a.encode_utf16().collect::<Vec<_>>(),
                &b.encode_utf16().collect::<Vec<_>>(),
            )?;
            return Ok(Val::AnyRef(Some(
                write_submilli_string_struct_units(caller, &result)?.to_anyref(),
            )));
        }
        "edit" | "insertAt" | "applyPatch" => {
            return mutate(caller, budget, &normalize(&first)?, op, params);
        }
        _ => unreachable!("registered code operation"),
    };
    encode(caller, budget, &result)
}
fn encode(caller: &mut Caller<'_, StoreData>, budget: &mut Budget, result: &Value) -> Result<Val> {
    let encoded = serde_json::to_string(result)?;
    budget.check_size(encoded.len())?;
    budget.charge(caller, encoded.len().saturating_mul(4))?;
    crate::stdlib::session::value::deserialize(caller, &encoded.encode_utf16().collect::<Vec<_>>())
}
fn read_window(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    params: &[Val],
) -> Result<Value> {
    let start = integer(&params[1], "offset", 1)? - 1;
    let limit = integer(&params[2], "limit", 1)?;
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
fn mutate(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    op: &str,
    params: &[Val],
) -> Result<Val> {
    let original = read_file(caller, budget, path, op)?;
    budget.charge(caller, original.len().saturating_mul(64))?;
    // Splicing the change back into the original rewrites the whole text.
    fuel::charge(&mut *caller, fuel::SCAN, original.len() as u64)?;
    let change = match op {
        "edit" => prepare_edit(caller, budget, &original, params)?,
        "insertAt" => text::insert(
            &original,
            integer(&params[1], "line", 1)?,
            &argument(caller, budget, &params[2])?,
        )?,
        "applyPatch" => {
            let patch = argument(caller, budget, &params[1])?;
            budget.charge(caller, patch.len().saturating_mul(32))?;
            // Each hunk is located by scanning the original.
            fuel::charge(
                &mut *caller,
                fuel::SCAN,
                (original.len() as u64).saturating_mul(patch.lines().count().max(1) as u64),
            )?;
            fuel::charge(&mut *caller, fuel::PARSE, patch.len() as u64)?;
            patch::apply(&original, &patch)?
        }
        _ => unreachable!(),
    };
    budget.check_size(change.text.len())?;
    let success = change.diagnostics.is_empty();
    let changed = success && change.text != original;
    let patch = if changed {
        String::from_utf16(&diff(
            caller,
            budget,
            &original.encode_utf16().collect::<Vec<_>>(),
            &change.text.encode_utf16().collect::<Vec<_>>(),
        )?)?
    } else {
        String::new()
    };
    check_security(
        &*caller,
        "fs.write",
        json!({"path":path, "length": change.text.len(), "diff": patch}),
    )?;
    // Allocate the return value before commit so a guest allocation failure cannot hide a write.
    let result =
        json!({"success":success,"changed":changed,"diff":patch,"diagnostics":change.diagnostics});
    let result = encode(caller, budget, &result)?;
    if changed {
        let resolved = resolve_content_or_trap(caller.data(), path, op)?;
        require_writable(&*caller, resolved.placement(), "fs.write", path)?;
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
    params: &[Val],
) -> Result<text::Edit> {
    let old = argument(caller, budget, &params[1])?;
    let new = argument(caller, budget, &params[2])?;
    // Naive substring search: every position against the whole needle.
    fuel::charge(
        &mut *caller,
        fuel::SCAN,
        (original.len() as u64).saturating_mul(old.len().max(1) as u64),
    )?;
    if old.is_empty() {
        bail!("code.edit: oldString must not be empty; use insertAt");
    }
    let matches = original.match_indices(&old).count();
    let selected = if params[3].unwrap_i32() != 0 {
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
    text::replace(
        original,
        &old,
        &new,
        params[3].unwrap_i32() != 0,
        integer(&params[4], "nearLine", 0)?,
        maximum,
    )
}

fn normalize(path: &str) -> Result<String> {
    crate::runtime::fs::guest_normalize("/", path)
        .map_err(|e| wasmtime::Error::msg(format!("code: {e}")))
}
fn gate(caller: &Caller<'_, StoreData>, capability: &str, path: &str) -> Result<()> {
    check_security(caller, capability, json!({"path":path,"recursive":true}))
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
    gate(caller, "fs.read", path)?;
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
    if skip_binary && bytes.contains(&0) {
        return Ok(String::new());
    }
    String::from_utf8(bytes)
        .map_err(|_| wasmtime::Error::msg(format!("code.{op}: {path} is not valid UTF-8")))
}
fn argument(caller: &mut Caller<'_, StoreData>, budget: &mut Budget, val: &Val) -> Result<String> {
    String::from_utf16(&units(caller, budget, val)?).map_err(|_| {
        wasmtime::Error::msg("code: filesystem text must not contain lone UTF-16 surrogates")
    })
}
fn units(caller: &mut Caller<'_, StoreData>, budget: &mut Budget, val: &Val) -> Result<Vec<u16>> {
    let value = read_string_units(caller, val, "code argument")?;
    budget.check_size(value.len().saturating_mul(2))?;
    budget.charge(caller, value.len().saturating_mul(4))?;
    Ok(value)
}
fn diff(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    a: &[u16],
    b: &[u16],
) -> Result<Vec<u16>> {
    budget.charge(caller, (a.len() + b.len()).saturating_mul(16))?;
    let na = text::line_count(a);
    let nb = text::line_count(b);
    budget.charge(caller, (na + nb).saturating_mul(96))?;
    text::check_diff_size(na, nb)?;
    // Line-based LCS: a cell per line pair, plus a scan of both texts.
    fuel::charge(
        &mut *caller,
        fuel::PARSE,
        (na as u64).saturating_mul(nb as u64),
    )?;
    fuel::charge(&mut *caller, fuel::SCAN, (a.len() + b.len()) as u64)?;
    let result = text::diff(a, b)?;
    budget.check_size(result.len().saturating_mul(2))?;
    Ok(result)
}
fn integer(val: &Val, name: &str, min: usize) -> Result<usize> {
    let n = val.unwrap_f64();
    if !n.is_finite() || n.fract() != 0.0 || n < min as f64 || n > u32::MAX as f64 {
        bail!(
            "code: {name} must be an integer between {min} and {}",
            u32::MAX
        );
    }
    Ok(n as usize)
}
