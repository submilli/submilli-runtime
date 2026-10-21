//! Wasm registrations, argument decoding and effect boundaries for `submilli:code`.
use super::{MODULE_NAME, budget::Budget, walk};
use crate::runtime::StoreData;
use crate::runtime::effects;
use crate::runtime::host::{
    abi_arg, invariant_trap, register_host_value_async, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::vtable::read_string_units;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Result, Val, ValType, bail};

pub fn install(linker: &mut Linker<StoreData>) -> Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape),
    ));
    let options = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "read"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64, ValType::F64],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let path = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let request = ReadArguments {
                    path: super::normalize(caller.data().vfs.cwd(), &path)?,
                    start: integer(abi_arg(params, 1)?, "offset", 1)? - 1,
                    limit: integer(abi_arg(params, 2)?, "limit", 1)?,
                };
                let result =
                    effects::execute(caller, "code.read", request, move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::read(
                                caller,
                                &mut budget,
                                &request.path,
                                request.start,
                                request.limit,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    })
                    .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "search"),
        FuncType::new(&engine, [string.clone(), options], [object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let request = SearchArguments {
                    pattern: argument(caller, &mut budget, abi_arg(params, 0)?)?,
                    options: walk::Options::read(caller, &mut budget, abi_arg(params, 1)?)?,
                };
                let result =
                    effects::execute(caller, "code.search", request, move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::search(
                                caller,
                                &mut budget,
                                &request.pattern,
                                &request.options,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    })
                    .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "glob"),
        FuncType::new(&engine, [string.clone()], [object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let request = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let result =
                    effects::execute(caller, "code.glob", request, move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::glob(caller, &mut budget, &request)?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    })
                    .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "tree"),
        FuncType::new(&engine, [string.clone(), ValType::F64], [object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let path = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let request = TreeArguments {
                    path: super::normalize(caller.data().vfs.cwd(), &path)?,
                    depth: integer(abi_arg(params, 1)?, "depth", 0)?,
                };
                let result =
                    effects::execute(caller, "code.tree", request, move |caller, _, request| {
                        Box::pin(async move {
                            let value =
                                super::tree(caller, &mut budget, &request.path, request.depth)?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    })
                    .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "edit"),
        FuncType::new(
            &engine,
            [
                string.clone(),
                string.clone(),
                string.clone(),
                ValType::I32,
                ValType::F64,
            ],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let path = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let old = argument(caller, &mut budget, abi_arg(params, 1)?)?;
                let new = argument(caller, &mut budget, abi_arg(params, 2)?)?;
                let Val::I32(replace_all) = abi_arg(params, 3)? else {
                    return Err(invariant_trap("code.edit: invalid boolean argument"));
                };
                let near_line = integer(abi_arg(params, 4)?, "nearLine", 0)?;
                let request = EditArguments {
                    path: super::normalize(caller.data().vfs.cwd(), &path)?,
                    old,
                    new,
                    replace_all: *replace_all != 0,
                    near_line,
                };
                let result =
                    effects::execute(caller, "code.edit", request, move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::edit(
                                caller,
                                &mut budget,
                                &request.path,
                                &request.old,
                                &request.new,
                                request.replace_all,
                                request.near_line,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    })
                    .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "insertAt"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64, string.clone()],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let path = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let line = integer(abi_arg(params, 1)?, "line", 1)?;
                let text = argument(caller, &mut budget, abi_arg(params, 2)?)?;
                let request = InsertArguments {
                    path: super::normalize(caller.data().vfs.cwd(), &path)?,
                    line,
                    text,
                };
                let result = effects::execute(
                    caller,
                    "code.insertAt",
                    request,
                    move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::insert_at(
                                caller,
                                &mut budget,
                                &request.path,
                                request.line,
                                &request.text,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    },
                )
                .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "diffText"),
        FuncType::new(&engine, [string.clone(), string.clone()], [string.clone()]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let first = units(caller, &mut budget, abi_arg(params, 0)?)?;
                let second = units(caller, &mut budget, abi_arg(params, 1)?)?;
                let result = super::diff_text(caller, &mut budget, &first, &second)?;
                Ok(Val::AnyRef(Some(
                    write_submilli_string_struct_units(caller, &result)?.to_anyref(),
                )))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "diffFiles"),
        FuncType::new(&engine, [string.clone(), string.clone()], [string.clone()]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let first = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let second = argument(caller, &mut budget, abi_arg(params, 1)?)?;
                let request = DiffFilesArguments {
                    first: super::normalize(caller.data().vfs.cwd(), &first)?,
                    second: super::normalize(caller.data().vfs.cwd(), &second)?,
                };
                let result = effects::execute(
                    caller,
                    "code.diffFiles",
                    request,
                    move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::diff_files(
                                caller,
                                &mut budget,
                                &request.first,
                                &request.second,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    },
                )
                .await?;
                Ok(Val::AnyRef(Some(
                    write_submilli_string_struct_units(caller, &result.value)?.to_anyref(),
                )))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "applyPatch"),
        FuncType::new(&engine, [string.clone(), string], [object]),
        |caller, params| {
            Box::pin(async move {
                let mut budget = Budget::new(caller);
                let path = argument(caller, &mut budget, abi_arg(params, 0)?)?;
                let patch = argument(caller, &mut budget, abi_arg(params, 1)?)?;
                let request = PatchArguments {
                    path: super::normalize(caller.data().vfs.cwd(), &path)?,
                    patch,
                };
                let result = effects::execute(
                    caller,
                    "code.applyPatch",
                    request,
                    move |caller, _, request| {
                        Box::pin(async move {
                            let value = super::apply_patch(
                                caller,
                                &mut budget,
                                &request.path,
                                &request.patch,
                            )?;
                            Ok(CodeResult {
                                value,
                                _budget: Some(budget),
                            })
                        })
                    },
                )
                .await?;
                encode_json(caller, &result.value)
            })
        },
    )?;
    Ok(())
}

#[derive(Serialize)]
struct ReadArguments {
    path: String,
    start: usize,
    limit: usize,
}

#[derive(Serialize)]
struct SearchArguments {
    pattern: String,
    options: walk::Options,
}

#[derive(Serialize)]
struct TreeArguments {
    path: String,
    depth: usize,
}

#[derive(Serialize)]
struct EditArguments {
    path: String,
    old: String,
    new: String,
    replace_all: bool,
    near_line: usize,
}

#[derive(Serialize)]
struct InsertArguments {
    path: String,
    line: usize,
    text: String,
}

#[derive(Serialize)]
struct DiffFilesArguments {
    first: String,
    second: String,
}

#[derive(Serialize)]
struct PatchArguments {
    path: String,
    patch: String,
}

#[derive(Serialize, Deserialize)]
struct CodeResult<T> {
    value: T,
    // Keep live native buffers charged through history encoding and Wasm conversion.
    #[serde(skip)]
    _budget: Option<Budget>,
}

fn encode_json(caller: &mut Caller<'_, StoreData>, result: &Value) -> Result<Val> {
    let mut budget = Budget::new(caller);
    let encoded = serde_json::to_string(result)?;
    budget.check_size(encoded.len())?;
    budget.charge(caller, encoded.len().saturating_mul(4))?;
    crate::stdlib::session::value::deserialize(caller, &encoded.encode_utf16().collect::<Vec<_>>())
}

pub(super) fn argument(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    val: &Val,
) -> Result<String> {
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
fn integer(val: &Val, name: &str, min: usize) -> Result<usize> {
    let n = match val {
        Val::F64(bits) => f64::from_bits(*bits),
        _ => {
            return Err(crate::runtime::host::invariant_trap(
                "code: invalid numeric argument",
            ));
        }
    };
    if !n.is_finite() || n.fract() != 0.0 || n < min as f64 || n > u32::MAX as f64 {
        bail!(
            "code: {name} must be an integer between {min} and {}",
            u32::MAX
        );
    }
    Ok(n as usize)
}
