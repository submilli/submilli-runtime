//! Diagnostics for failed bracket indexing. Formatting runs only after a miss.

use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, fatal_host_error, intrinsic_string_type, range_error, read_string_arg,
    register_host_fn,
};
use crate::runtime::number::format_number_js;
use crate::{PackageDeclaration, Param, Type};

use super::{MODULE_NAME, declare_method};

#[derive(Clone, Copy)]
pub(crate) enum IndexAccess {
    Read = 0,
    Write = 1,
    FixedWrite = 2,
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
            Self::Read if at_is_missing => {
                format!("{detail}; use `{receiver}.at({index})` to get `undefined` instead")
            }
            Self::Read => {
                format!("{detail}; use `{receiver}.at({index})` for a non-throwing read instead")
            }
            Self::Write if can_append => {
                format!("{detail}; use `{receiver}.push(value)` to append instead")
            }
            Self::Write | Self::FixedWrite => {
                format!("{detail}; indexed writes require an existing element")
            }
        }
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
            [ValType::F64, ValType::F64, string, ValType::F64],
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
                _ => return Err(fatal_host_error("index_error: invalid access kind")),
            };
            fuel::charge(&mut *caller, fuel::PARSE, 1)?;
            Err(range_error(access.message(
                index,
                length as usize,
                &receiver,
            )))
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
}
