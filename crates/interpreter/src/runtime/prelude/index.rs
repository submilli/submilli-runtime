//! Diagnostics for failed bracket indexing and for reads that find no value
//! (an index out of range, a key a record lacks, a regex group that did not
//! take part). Formatting runs only after a miss.

use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, fatal_host_error, intrinsic_string_type, missing_value_error, range_error,
    read_string_arg, register_host_fn,
};
use crate::runtime::number::format_number_js;
use crate::{PackageDeclaration, Param, Type};

use super::{MODULE_NAME, declare_method};

#[derive(Clone, Copy)]
pub(crate) enum IndexAccess {
    Read = 0,
    Write = 1,
    FixedWrite = 2,
    /// A regex match group that did not take part in the match.
    Group = 3,
}

impl IndexAccess {
    pub(crate) fn message(self, index: f64, length: usize, receiver: &str) -> String {
        let relative_index = if index.is_nan() { 0.0 } else { index.trunc() };
        let at_is_missing = relative_index >= length as f64 || relative_index < -(length as f64);
        let can_append = index.is_finite() && index.fract() == 0.0 && index >= length as f64;
        let index = format_number_js(index);
        let subject = if receiver.is_empty() {
            String::new()
        } else {
            format!(" for `{receiver}`")
        };
        let detail = format!("index {index} is out of range{subject} (length {length})");
        match self {
            Self::Read if receiver.is_empty() => {
                format!("{detail}; use `?? …` at the read, or `.at({index})`")
            }
            Self::Read if at_is_missing => {
                format!("{detail}; use `{receiver}[{index}] ?? …` or `{receiver}.at({index})`")
            }
            Self::Read => format!(
                "{detail}; use `{receiver}[{index}] ?? …` for a default, or `{receiver}.at({index})` to count from the end"
            ),
            Self::Write if can_append => {
                format!("{detail}; use `{receiver}.push(value)` to append instead")
            }
            Self::Write | Self::FixedWrite => {
                format!("{detail}; indexed writes require an existing element")
            }
            Self::Group if receiver.is_empty() => {
                format!("group {index} did not take part in the match; use `?? …` at the read")
            }
            Self::Group => format!(
                "group {index} of `{receiver}` did not take part in the match; use `{receiver}[{index}] ?? …`"
            ),
        }
    }
}

/// The message for a read of `key` that `receiver` (a source label, possibly
/// empty) has no entry for.
pub(crate) fn missing_key_message(key: &str, receiver: &str) -> String {
    // A long key is cut, so the message stays readable.
    let key = if key.chars().count() > 64 {
        format!("{:?}…", key.chars().take(64).collect::<String>())
    } else {
        format!("{key:?}")
    };
    if receiver.is_empty() {
        format!("key {key} is missing; use `?? …` at the read, or check with `in` first")
    } else {
        format!(
            "key {key} is missing from `{receiver}`; use `{receiver}[key] ?? …` or check `key in {receiver}` first"
        )
    }
}

pub(crate) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("index_error"),
        FuncType::new(
            &engine,
            [ValType::F64, ValType::F64, string.clone(), ValType::F64],
            [],
        ),
        |caller, params, _| {
            let index = abi_arg(params, 0)?
                .f64()
                .ok_or_else(|| fatal_host_error("index_error: expected f64 index"))?;
            let length = abi_arg(params, 1)?
                .f64()
                .ok_or_else(|| fatal_host_error("index_error: expected f64 length"))?;
            if !(0.0..=u32::MAX as f64).contains(&length) || length.fract() != 0.0 {
                return Err(fatal_host_error("index_error: invalid length"));
            }
            let receiver = read_string_arg(caller, abi_arg(params, 2)?, "index_error")?;
            let access = match abi_arg(params, 3)?.f64() {
                Some(0.0) => IndexAccess::Read,
                Some(1.0) => IndexAccess::Write,
                Some(2.0) => IndexAccess::FixedWrite,
                Some(3.0) => IndexAccess::Group,
                _ => return Err(fatal_host_error("index_error: invalid access kind")),
            };
            fuel::charge(&mut *caller, fuel::PARSE, 1)?;
            let message = access.message(index, length as usize, &receiver);
            // A read of a missing element is a missing value; a write outside
            // the array stays a range violation.
            Err(match access {
                IndexAccess::Read | IndexAccess::Group => missing_value_error(message),
                IndexAccess::Write | IndexAccess::FixedWrite => range_error(message),
            })
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("missing_key_error"),
        FuncType::new(&engine, [string.clone(), string], []),
        |caller, params, _| {
            let key = read_string_arg(caller, abi_arg(params, 0)?, "missing_key_error key")?;
            let receiver =
                read_string_arg(caller, abi_arg(params, 1)?, "missing_key_error receiver")?;
            fuel::charge(&mut *caller, fuel::PARSE, 1)?;
            Err(missing_value_error(missing_key_message(&key, &receiver)))
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    declare_method(
        defs,
        "index_error",
        crate::mangle::prelude("index_error"),
        vec![
            Param::anon(Type::Number),
            Param::anon(Type::Number),
            Param::anon(Type::String),
            Param::anon(Type::Number),
        ],
        Type::Void,
    );
    declare_method(
        defs,
        "missing_key_error",
        crate::mangle::prelude("missing_key_error"),
        vec![Param::anon(Type::String), Param::anon(Type::String)],
        Type::Void,
    );
}
