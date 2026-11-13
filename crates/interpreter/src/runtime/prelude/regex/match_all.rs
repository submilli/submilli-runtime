//! Lazy match iteration retains the charged decoded input and its own cursor.
use super::{FlagSet, as_struct, build_match_box, exec_at, flag_bits, input, last_index};
use crate::runtime::prelude::iterator::{self, IterKind, IteratorSource};
use crate::runtime::{
    StoreData,
    gc_singleton::singleton_struct,
    host::{abi_arg, abi_result, fatal_host_error},
    intrinsic_types::intrinsic_types,
};
use std::sync::Arc;
use wasmtime::{
    Caller, ExternRef, FieldType, Finality, Mutability, RefType, StorageType, StructRef,
    StructRefPre, Val, ValType,
};

pub(super) fn create(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let input_value = *abi_arg(params, 0)?;
    let pattern = *abi_arg(params, 1)?;
    let regex = if super::arg_is_string(caller, &pattern)? {
        let flags = crate::runtime::host::write_submilli_string_struct(caller, "g")?;
        let value = super::construct(caller, &pattern, &Val::AnyRef(Some(flags.to_anyref())))?;
        as_struct(caller, &value, "matchAll regex")?
    } else {
        as_struct(caller, &pattern, "matchAll regex")?
    };
    if flag_bits(caller, &regex)? & FlagSet::G.bits() as i32 == 0 {
        return Err(crate::runtime::host::type_error(
            "String.matchAll requires a global RegExp",
        ));
    }
    let position = last_index(caller, &regex)? as i64;
    let decoded = input::read(caller, &input_value)?;
    let data = ExternRef::new(&mut *caller, decoded)?;
    let intr = intrinsic_types(&mut *caller)?;
    let reference = ValType::Ref(RefType::new(true, intr.object.clone().into()));
    let field = |ty, mutable| FieldType::new(mutable, StorageType::ValType(ty));
    let env_type = singleton_struct(
        caller.engine(),
        Finality::Final,
        None,
        vec![
            field(reference.clone(), Mutability::Const),
            field(reference, Mutability::Const),
            field(ValType::EXTERNREF, Mutability::Const),
            field(ValType::I64, Mutability::Var),
        ],
    )?;
    let pre = StructRefPre::new(&mut *caller, env_type);
    let env = StructRef::new(
        &mut *caller,
        &pre,
        &[
            Val::AnyRef(Some(regex.to_anyref())),
            input_value,
            Val::ExternRef(Some(data)),
            Val::I64(position),
        ],
    )?;
    let (next_type, closure_type) = iterator::next_closure_type(caller.engine(), &intr)?;
    let next = iterator::shared_next(
        caller,
        IteratorSource::Regex,
        IterKind::Values,
        next_type,
        step,
    )?;
    iterator::build_iterator(
        caller,
        iterator::IteratorTag::RegExpString,
        closure_type,
        next,
        Val::AnyRef(Some(env.to_anyref())),
    )
}

fn step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let env = as_struct(caller, abi_arg(params, 0)?, "matchAll cursor")?;
    let Val::I64(position) = env.field(&mut *caller, 3)? else {
        return Err(fatal_host_error("invalid matchAll cursor"));
    };
    if position < 0 {
        *abi_result(results, 0)? = iterator::iter_done(caller)?;
        return Ok(());
    }
    let regex_value = env.field(&mut *caller, 0)?;
    let regex = as_struct(caller, &regex_value, "matchAll regex")?;
    let input_value = env.field(&mut *caller, 1)?;
    let Val::ExternRef(Some(data)) = env.field(&mut *caller, 2)? else {
        return Err(fatal_host_error("missing matchAll input"));
    };
    let input = data
        .data(&*caller)?
        .and_then(|data| data.downcast_ref::<Arc<input::DecodedInput>>())
        .cloned()
        .ok_or_else(|| fatal_host_error("invalid matchAll input"))?;
    let position = usize::try_from(position).map_err(fatal_host_error)?;
    let Some(found) = exec_at(caller, &regex, &input, position)? else {
        env.set_field(&mut *caller, 3, Val::I64(-1))?;
        *abi_result(results, 0)? = iterator::iter_done(caller)?;
        return Ok(());
    };
    let end = input.byte_to_unit(found.next_last_index)?;
    let next = if found.match_start == found.match_end {
        input.advance(
            end,
            flag_bits(caller, &regex)? & FlagSet::U.bits() as i32 != 0,
        )?
    } else {
        end
    };
    let value = build_match_box(caller, &input, &input_value, &found)?;
    env.set_field(
        &mut *caller,
        3,
        Val::I64(i64::try_from(next).map_err(fatal_host_error)?),
    )?;
    *abi_result(results, 0)? = iterator::iter_yield(caller, value)?;
    Ok(())
}
