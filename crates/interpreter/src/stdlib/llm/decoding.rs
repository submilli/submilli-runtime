//! Invoke the compiler-supplied decoder without absorbing execution failures.
use crate::runtime::{
    StoreData, fuel,
    host::{
        fatal_host_error, range_error, read_string_arg, syntax_error, type_error,
        write_submilli_string_struct,
    },
    prelude::{
        closure::{self, Closure},
        collection::unbox_bool,
        object::read_property,
        undefined,
    },
};
use wasmtime::{Caller, Val};

pub(super) fn read_decoder(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    typed: bool,
) -> wasmtime::Result<Option<Closure>> {
    let absent = undefined::is_undefined(caller, value)?;
    if absent == typed {
        return Err(fatal_host_error(
            "LLM schema and decoder must be supplied together",
        ));
    }
    if absent {
        return Ok(None);
    }
    closure::read(caller, value, "LLM decoder")
        .map(Some)
        .map_err(fatal_host_error)
}

pub(super) struct DecodeFailure {
    pub reason: String,
    kind: DecodeFailureKind,
}

enum DecodeFailureKind {
    Syntax,
    Type,
    Range,
}

impl DecodeFailure {
    pub fn into_error(self) -> wasmtime::Error {
        match self.kind {
            DecodeFailureKind::Syntax => syntax_error(self.reason),
            DecodeFailureKind::Type => type_error(self.reason),
            DecodeFailureKind::Range => range_error(self.reason),
        }
    }
}

pub(super) async fn decode(
    caller: &mut Caller<'_, StoreData>,
    decoder: &Closure,
    text: &str,
) -> wasmtime::Result<Result<Val, DecodeFailure>> {
    let text = fuel::settle_result(caller, |caller| write_submilli_string_struct(caller, text))?;
    // The callback catches only JSON syntax/numeric range and type mismatches. Fuel, resource,
    // cancellation, and internal failures propagate through this await unchanged.
    fuel::charge_call(&mut *caller)?;
    let result = decoder
        .call(caller, &[Val::AnyRef(Some(text.to_anyref()))])
        .await?;
    let ok = read_property(caller, &result, "ok").await?;
    if unbox_bool(caller, &ok)? {
        return Ok(Ok(read_property(caller, &result, "value").await?));
    }
    let reason = read_property(caller, &result, "reason").await?;
    let reason = read_string_arg(&mut *caller, &reason, "decoder failure")?;
    let kind = read_property(caller, &result, "kind").await?;
    let kind = read_string_arg(&mut *caller, &kind, "decoder failure kind")?;
    let kind = match kind.as_str() {
        "SyntaxError" => DecodeFailureKind::Syntax,
        "TypeError" => DecodeFailureKind::Type,
        "RangeError" => DecodeFailureKind::Range,
        _ => return Err(fatal_host_error("Invalid LLM decoder failure kind")),
    };
    Ok(Err(DecodeFailure { reason, kind }))
}
