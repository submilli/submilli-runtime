//! Shared Temporal ABI and object construction; the per-type method surfaces
//! live in the sibling type modules.

use crate::runtime::host::{abi_arg, abi_result};
use std::collections::BTreeMap;

use jiff::{
    RoundMode, Span, SpanRelativeTo, SpanRound, Timestamp, TimestampDifference, Unit, Zoned,
    ZonedDifference, civil, tz::TimeZone,
};
use wasmtime::{
    Caller, FieldType, Func, FuncType, Global, GlobalType, HeapType, Linker, Mutability, RefType,
    Rooted, StorageType, Store, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel::host_func_async;
use crate::runtime::host::{read_string_arg, register_host_fn, write_submilli_string_struct};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::collection::object_field;
use crate::{
    MangledName, NamespaceSymbol, ObjectField, Package, PackageDeclaration, Param,
    Span as DiagSpan, Type, ValueKind, ValueSymbol,
};

pub const TEMPORAL_MODULE_NAME: &str = "submilli:temporal";
const I64_BOUND_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
const PLAIN_MONTH_DAY_MARKER: i64 = 0x4d4f_4e54_4844_4159;

#[derive(Clone)]
pub(crate) struct TemporalAbi {
    pub(crate) instant: StructType,
    pub(crate) duration: StructType,
    pub(crate) zoned_date_time: StructType,
    pub(crate) plain_date: StructType,
    pub(crate) plain_time: StructType,
    pub(crate) plain_date_time: StructType,
    pub(crate) plain_year_month: StructType,
    pub(crate) plain_month_day: StructType,
    pub(crate) instant_vtable: Global,
    pub(crate) duration_vtable: Global,
    pub(crate) zoned_date_time_vtable: Global,
    pub(crate) plain_date_vtable: Global,
    pub(crate) plain_time_vtable: Global,
    pub(crate) plain_date_time_vtable: Global,
    pub(crate) plain_year_month_vtable: Global,
    pub(crate) plain_month_day_vtable: Global,
}

pub(crate) fn install_abi(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    module: &str,
) -> wasmtime::Result<TemporalAbi> {
    use crate::runtime::gc_singleton::singleton_struct;
    use wasmtime::Finality;

    let imm = Mutability::Const;
    let vtable_field = FieldType::new(
        imm,
        StorageType::ValType(ValType::Ref(RefType::new(
            false,
            intr.vtable.clone().into(),
        ))),
    );
    let i32_field = FieldType::new(imm, StorageType::ValType(ValType::I32));
    let i64_field = FieldType::new(imm, StorageType::ValType(ValType::I64));

    let plain_date = singleton_struct(
        store.engine(),
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![
            vtable_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
        ],
    )?;
    let plain_time = singleton_struct(
        store.engine(),
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![
            vtable_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
        ],
    )?;
    let plain_date_time = singleton_struct(
        store.engine(),
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![
            vtable_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
            i32_field.clone(),
        ],
    )?;
    let plain_year_month = singleton_struct(
        store.engine(),
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![vtable_field.clone(), i32_field.clone(), i32_field.clone()],
    )?;
    let plain_month_day = singleton_struct(
        store.engine(),
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![vtable_field, i32_field.clone(), i32_field, i64_field],
    )?;

    let instant_slots = plain_vtable_slots(
        store,
        intr,
        intr.temporal_instant.clone(),
        instant_to_string_host,
        instant_equals_host,
    )?;
    let instant_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_instant_host_vtable",
        instant_slots,
    )?;
    let duration_slots = plain_vtable_slots(
        store,
        intr,
        intr.temporal_duration.clone(),
        duration_to_string_host,
        duration_equals_host,
    )?;
    let duration_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_duration_host_vtable",
        duration_slots,
    )?;
    let zdt_slots = plain_vtable_slots(
        store,
        intr,
        intr.temporal_zdt.clone(),
        zoned_date_time_to_string_host,
        zoned_date_time_equals_host,
    )?;
    let zoned_date_time_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_zoned_date_time_host_vtable",
        zdt_slots,
    )?;
    let plain_date_slots = plain_vtable_slots(
        store,
        intr,
        plain_date.clone(),
        plain_to_string_date,
        plain_equals_date,
    )?;
    let plain_date_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_plain_date_vtable",
        plain_date_slots,
    )?;
    let plain_time_slots = plain_vtable_slots(
        store,
        intr,
        plain_time.clone(),
        plain_to_string_time,
        plain_equals_time,
    )?;
    let plain_time_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_plain_time_vtable",
        plain_time_slots,
    )?;
    let plain_date_time_slots = plain_vtable_slots(
        store,
        intr,
        plain_date_time.clone(),
        plain_to_string_date_time,
        plain_equals_date_time,
    )?;
    let plain_date_time_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_plain_date_time_vtable",
        plain_date_time_slots,
    )?;
    let plain_year_month_slots = plain_vtable_slots(
        store,
        intr,
        plain_year_month.clone(),
        plain_to_string_year_month,
        plain_equals_year_month,
    )?;
    let plain_year_month_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_plain_year_month_vtable",
        plain_year_month_slots,
    )?;
    let plain_month_day_slots = plain_vtable_slots(
        store,
        intr,
        plain_month_day.clone(),
        plain_to_string_month_day,
        plain_equals_month_day,
    )?;
    let plain_month_day_vtable = define_plain_vtable(
        linker,
        store,
        intr,
        module,
        "temporal_plain_month_day_vtable",
        plain_month_day_slots,
    )?;

    Ok(TemporalAbi {
        instant: intr.temporal_instant.clone(),
        duration: intr.temporal_duration.clone(),
        zoned_date_time: intr.temporal_zdt.clone(),
        plain_date,
        plain_time,
        plain_date_time,
        plain_year_month,
        plain_month_day,
        instant_vtable,
        duration_vtable,
        zoned_date_time_vtable,
        plain_date_vtable,
        plain_time_vtable,
        plain_date_time_vtable,
        plain_year_month_vtable,
        plain_month_day_vtable,
    })
}

fn define_plain_vtable(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    module: &str,
    name: &str,
    slots: [Func; 4],
) -> wasmtime::Result<Global> {
    let pre = StructRefPre::new(&mut *store, intr.vtable.clone());
    let vtable = StructRef::new(
        &mut *store,
        &pre,
        &[
            Val::FuncRef(Some(slots[0])),
            Val::FuncRef(Some(slots[1])),
            Val::FuncRef(Some(slots[2])),
            Val::FuncRef(Some(slots[3])),
        ],
    )?;
    let gty = GlobalType::new(
        ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.vtable.clone()),
        )),
        Mutability::Const,
    );
    let global = Global::new(&mut *store, gty, Val::AnyRef(Some(vtable.to_anyref())))?;
    let field = crate::mangle::prelude(name);
    linker.define(&mut *store, module, field.as_str(), global)?;
    Ok(global)
}

pub(super) type PlainStringer =
    fn(&mut Caller<'_, StoreData>, Rooted<StructRef>) -> wasmtime::Result<String>;
pub(super) type PlainEquals =
    fn(&mut Caller<'_, StoreData>, Rooted<StructRef>, Rooted<StructRef>) -> wasmtime::Result<bool>;

fn plain_vtable_slots(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    ty: StructType,
    stringer: PlainStringer,
    equals_op: PlainEquals,
) -> wasmtime::Result<[Func; 4]> {
    let to_string_ty = intr.to_string_fn.clone();
    let to_json_ty = intr.to_json_fn.clone();
    let equals_ty = intr.equals_fn.clone();
    let hash_ty = intr.hash_fn.clone();

    let st_ty = ty.clone();
    let to_string = host_func_async(
        &mut *store,
        to_string_ty,
        move |mut caller, params, results| {
            let st_ty = st_ty.clone();
            Box::new(async move {
                let st = cast_struct(
                    &mut caller,
                    abi_arg(params, 0)?,
                    &st_ty,
                    "Temporal.Plain.toString",
                )?;
                let out = stringer(&mut caller, st)?;
                let s = write_submilli_string_struct(&mut caller, &out)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(s.to_anyref()));
                Ok(())
            })
        },
    );

    let st_ty = ty.clone();
    let to_json = host_func_async(
        &mut *store,
        to_json_ty,
        move |mut caller, params, results| {
            let st_ty = st_ty.clone();
            Box::new(async move {
                let st = cast_struct(
                    &mut caller,
                    abi_arg(params, 0)?,
                    &st_ty,
                    "Temporal.Plain.toJSON",
                )?;
                let out = stringer(&mut caller, st)?;
                let json = serde_json::to_string(&out).map_err(|_| {
                    wasmtime::Error::msg(
                        "Temporal.Plain.toJSON: could not encode the ISO value as JSON; use a value within the supported date and time range",
                    )
                })?;
                let s = write_submilli_string_struct(&mut caller, &json)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(s.to_anyref()));
                Ok(())
            })
        },
    );

    let st_ty = ty.clone();
    let equals = host_func_async(
        &mut *store,
        equals_ty,
        move |mut caller, params, results| {
            let st_ty = st_ty.clone();
            Box::new(async move {
                let a = cast_struct(
                    &mut caller,
                    abi_arg(params, 0)?,
                    &st_ty,
                    "Temporal.Plain.equals",
                )?;
                let Some(b) = try_cast_struct(&mut caller, abi_arg(params, 1)?, &st_ty)? else {
                    *abi_result(results, 0)? = Val::I32(0);
                    return Ok(());
                };
                *abi_result(results, 0)? = Val::I32(equals_op(&mut caller, a, b)? as i32);
                Ok(())
            })
        },
    );

    let hash = host_func_async(&mut *store, hash_ty, move |_caller, _params, results| {
        Box::new(async move {
            *abi_result(results, 0)? = Val::I32(0);
            Ok(())
        })
    });

    Ok([to_string, to_json, equals, hash])
}

pub(super) fn as_struct_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    match val {
        Val::AnyRef(Some(any)) => any
            .as_struct(&mut *caller)?
            .ok_or_else(|| wasmtime::Error::msg(format!("{label}: expected object"))),
        other => Err(wasmtime::Error::msg(format!(
            "{label}: expected object, got {other:?}"
        ))),
    }
}

fn cast_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    _ty: &StructType,
    label: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    as_struct_val(caller, val, label)
}

fn try_cast_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    _ty: &StructType,
) -> wasmtime::Result<Option<Rooted<StructRef>>> {
    match val {
        Val::AnyRef(Some(any)) => any.as_struct(&mut *caller),
        _ => Ok(None),
    }
}

pub(super) fn st_i32(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    field: u32,
    label: &str,
) -> wasmtime::Result<i32> {
    match st.field(&mut *caller, field as usize)? {
        Val::I32(v) => Ok(v),
        other => Err(wasmtime::Error::msg(format!(
            "{label}: field {field} is {other:?}"
        ))),
    }
}

pub(super) fn plain_to_string_date(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let y = st_i32(caller, st, 1, "PlainDate")?;
    let m = st_i32(caller, st, 2, "PlainDate")?;
    let d = st_i32(caller, st, 3, "PlainDate")?;
    let date = y_m_d_date(
        i64::from(y),
        i64::from(m),
        i64::from(d),
        "PlainDate.toString",
    )?;
    Ok(super::plain_date::to_string(date))
}

pub(super) fn plain_to_string_time(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let h = st_i32(caller, st, 1, "PlainTime")?;
    let m = st_i32(caller, st, 2, "PlainTime")?;
    let s = st_i32(caller, st, 3, "PlainTime")?;
    let ns = st_i32(caller, st, 4, "PlainTime")?;
    let t = h_m_s_ns_time(
        i64::from(h),
        i64::from(m),
        i64::from(s),
        i64::from(ns),
        "PlainTime.toString",
    )?;
    Ok(super::plain_time::to_string(t))
}

pub(super) fn plain_to_string_date_time(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let date = y_m_d_date(
        i64::from(st_i32(caller, st, 1, "PlainDateTime")?),
        i64::from(st_i32(caller, st, 2, "PlainDateTime")?),
        i64::from(st_i32(caller, st, 3, "PlainDateTime")?),
        "PlainDateTime.toString",
    )?;
    let time = h_m_s_ns_time(
        i64::from(st_i32(caller, st, 4, "PlainDateTime")?),
        i64::from(st_i32(caller, st, 5, "PlainDateTime")?),
        i64::from(st_i32(caller, st, 6, "PlainDateTime")?),
        i64::from(st_i32(caller, st, 7, "PlainDateTime")?),
        "PlainDateTime.toString",
    )?;
    Ok(super::plain_date_time::to_string(date.to_datetime(time)))
}

pub(super) fn plain_to_string_year_month(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    Ok(super::plain_year_month::to_string(
        st_i32(caller, st, 1, "PlainYearMonth")?,
        st_i32(caller, st, 2, "PlainYearMonth")?,
    ))
}

pub(super) fn plain_to_string_month_day(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    Ok(super::plain_month_day::to_string(
        st_i32(caller, st, 1, "PlainMonthDay")?,
        st_i32(caller, st, 2, "PlainMonthDay")?,
    ))
}

fn fields_equal(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
    fields: &[u32],
    label: &str,
) -> wasmtime::Result<bool> {
    for &field in fields {
        if st_i32(caller, a, field, label)? != st_i32(caller, b, field, label)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn plain_equals_date(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    Ok(super::plain_date::equals(
        plain_date_from_struct(caller, a, "PlainDate.equals")?,
        plain_date_from_struct(caller, b, "PlainDate.equals")?,
    ))
}

pub(super) fn plain_equals_time(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    Ok(super::plain_time::equals(
        plain_time_from_struct(caller, a, 0, "PlainTime.equals")?,
        plain_time_from_struct(caller, b, 0, "PlainTime.equals")?,
    ))
}

pub(super) fn plain_equals_date_time(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    let a = plain_date_from_struct(caller, a, "PlainDateTime.equals")?.to_datetime(
        plain_time_from_struct(caller, a, 3, "PlainDateTime.equals")?,
    );
    let b = plain_date_from_struct(caller, b, "PlainDateTime.equals")?.to_datetime(
        plain_time_from_struct(caller, b, 3, "PlainDateTime.equals")?,
    );
    Ok(super::plain_date_time::equals(a, b))
}

pub(super) fn plain_equals_year_month(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    Ok(super::plain_year_month::equals(
        (
            st_i32(caller, a, 1, "PlainYearMonth.equals")?,
            st_i32(caller, a, 2, "PlainYearMonth.equals")?,
        ),
        (
            st_i32(caller, b, 1, "PlainYearMonth.equals")?,
            st_i32(caller, b, 2, "PlainYearMonth.equals")?,
        ),
    ))
}

pub(super) fn plain_equals_month_day(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    Ok(super::plain_month_day::equals(
        (
            st_i32(caller, a, 1, "PlainMonthDay.equals")?,
            st_i32(caller, a, 2, "PlainMonthDay.equals")?,
        ),
        (
            st_i32(caller, b, 1, "PlainMonthDay.equals")?,
            st_i32(caller, b, 2, "PlainMonthDay.equals")?,
        ),
    ))
}

fn instant_to_string_host(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    Ok(instant_from_struct(caller, st, "Instant.toString")?.to_string())
}

pub(super) fn instant_equals_host(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    Ok(
        st_i64(caller, a, 1, "Instant.equals")? == st_i64(caller, b, 1, "Instant.equals")?
            && st_i32(caller, a, 2, "Instant.equals")? == st_i32(caller, b, 2, "Instant.equals")?,
    )
}

fn duration_to_string_host(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let span = span_from_duration_struct(caller, st, "Duration.toString")?;
    duration_to_string_with_options(&span, None, None, None)
}

fn duration_equals_host(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    fields_equal(
        caller,
        a,
        b,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "Duration.equals",
    )
}

fn zoned_date_time_to_string_host(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let z = zoned_date_time_from_struct(caller, st, "ZonedDateTime.toString")?;
    Ok(z.to_string())
}

fn zoned_date_time_equals_host(
    caller: &mut Caller<'_, StoreData>,
    a: Rooted<StructRef>,
    b: Rooted<StructRef>,
) -> wasmtime::Result<bool> {
    let a_secs = st_i64(caller, a, 1, "ZonedDateTime.equals")?;
    let a_nanos = st_i32(caller, a, 2, "ZonedDateTime.equals")?;
    let a_tz = st_string(caller, a, 3, "ZonedDateTime.equals")?;
    let b_secs = st_i64(caller, b, 1, "ZonedDateTime.equals")?;
    let b_nanos = st_i32(caller, b, 2, "ZonedDateTime.equals")?;
    let b_tz = st_string(caller, b, 3, "ZonedDateTime.equals")?;
    Ok(a_secs == b_secs
        && a_nanos == b_nanos
        && super::zoned_date_time::time_zone_ids_equal(caller, &a_tz, &b_tz)?)
}

pub(super) fn reg_plain_date_to_year_month(
    linker: &mut Linker<StoreData>,
    iface: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, "toPlainYearMonth"),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "toPlainYearMonth")?;
            let y = st_i32(caller, st, 1, "toPlainYearMonth")?;
            let m = st_i32(caller, st, 2, "toPlainYearMonth")?;
            *abi_result(results, 0)? = make_plain_year_month(caller, y, m)?;
            Ok(())
        },
    )
}

pub(super) fn reg_plain_date_to_month_day(
    linker: &mut Linker<StoreData>,
    iface: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, "toPlainMonthDay"),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "toPlainMonthDay")?;
            let m = st_i32(caller, st, 2, "toPlainMonthDay")?;
            let d = st_i32(caller, st, 3, "toPlainMonthDay")?;
            *abi_result(results, 0)? = make_plain_month_day(caller, m, d)?;
            Ok(())
        },
    )
}

pub(super) fn prelude_key(iface: &str, method: &str) -> MangledName {
    crate::mangle::extend(
        &crate::mangle::extend(&crate::mangle::prelude("Temporal"), iface),
        method,
    )
}

pub(super) fn reg_plain_from(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    string: &ValType,
    obj: &ValType,
    ctor_iface: &str,
    label: &'static str,
    op: fn(&str, &mut Caller<'_, StoreData>) -> wasmtime::Result<Val>,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(ctor_iface, "from"),
        FuncType::new(engine, [string.clone()], [obj.clone()]),
        true,
        move |caller, params, results| {
            let s = read_string_arg(caller, abi_arg(params, 0)?, label)?;
            *abi_result(results, 0)? = op(&s, caller)?;
            Ok(())
        },
    )
}

pub(super) fn reg_plain_string(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    string: &ValType,
    iface: &str,
    method: &str,
    op: PlainStringer,
) -> wasmtime::Result<()> {
    let label = format!("{iface}.{method}");
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, method),
        FuncType::new(engine, [obj.clone()], [string.clone()]),
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let out = op(caller, st)?;
            let s = write_submilli_string_struct(caller, &out)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(s.to_anyref()));
            Ok(())
        },
    )
}

pub(super) fn reg_plain_equals(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    iface: &str,
    op: PlainEquals,
) -> wasmtime::Result<()> {
    let label = format!("{iface}.equals");
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, "equals"),
        FuncType::new(engine, [obj.clone(), obj.clone()], [ValType::I32]),
        true,
        move |caller, params, results| {
            let a = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let b = as_struct_val(caller, abi_arg(params, 1)?, &label)?;
            *abi_result(results, 0)? = Val::I32(op(caller, a, b)? as i32);
            Ok(())
        },
    )
}

pub(super) fn reg_plain_compare(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    ctor_iface: &str,
    fields: &'static [u32],
) -> wasmtime::Result<()> {
    let label = format!("{ctor_iface}.compare");
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(ctor_iface, "compare"),
        FuncType::new(engine, [obj.clone(), obj.clone()], [ValType::F64]),
        true,
        move |caller, params, results| {
            let a = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let b = as_struct_val(caller, abi_arg(params, 1)?, &label)?;
            let mut ord = 0;
            for &field in fields {
                let av = st_i32(caller, a, field, &label)?;
                let bv = st_i32(caller, b, field, &label)?;
                if av != bv {
                    ord = if av < bv { -1 } else { 1 };
                    break;
                }
            }
            *abi_result(results, 0)? = Val::F64((ord as f64).to_bits());
            Ok(())
        },
    )
}

pub(super) fn reg_plain_i32_getter(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    iface: &str,
    name: &str,
    field: u32,
) -> wasmtime::Result<()> {
    let label = format!("{iface}.{name}");
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, name),
        FuncType::new(engine, [obj.clone()], [ValType::F64]),
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            *abi_result(results, 0)? =
                Val::F64((st_i32(caller, st, field, &label)? as f64).to_bits());
            Ok(())
        },
    )
}

pub(super) fn reg_plain_time_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    iface: &str,
) -> wasmtime::Result<()> {
    let ns_field = if iface == "PlainTime" { 4 } else { 7 };
    for (name, div) in [
        ("millisecond", 1_000_000),
        ("microsecond", 1_000),
        ("nanosecond", 1),
    ] {
        let label = format!("{iface}.{name}");
        register_host_fn(
            linker,
            crate::runtime::prelude::MODULE_NAME,
            prelude_key(iface, name),
            FuncType::new(engine, [obj.clone()], [ValType::F64]),
            true,
            move |caller, params, results| {
                let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
                let ns = st_i32(caller, st, ns_field, &label)?;
                let out = if div == 1 { ns } else { (ns / div) % 1000 };
                *abi_result(results, 0)? = Val::F64((out as f64).to_bits());
                Ok(())
            },
        )?;
    }
    Ok(())
}

pub(super) fn reg_plain_date_derived_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    iface: &str,
    year_month_only: i32,
) -> wasmtime::Result<()> {
    let specs = [
        ("dayOfWeek", 0),
        ("monthCode", 0),
        ("dayOfYear", 1),
        ("weekOfYear", 2),
        ("yearOfWeek", 3),
        ("daysInWeek", 4),
        ("daysInMonth", 5),
        ("daysInYear", 6),
        ("monthsInYear", 7),
        ("inLeapYear", 8),
    ];
    for (name, selector) in specs {
        if year_month_only == 1
            && matches!(
                name,
                "dayOfWeek" | "dayOfYear" | "weekOfYear" | "yearOfWeek" | "daysInWeek"
            )
        {
            continue;
        }
        if name == "monthCode" {
            continue;
        }
        let label = format!("{iface}.{name}");
        let ret = if name == "inLeapYear" {
            ValType::I32
        } else {
            ValType::F64
        };
        register_host_fn(
            linker,
            crate::runtime::prelude::MODULE_NAME,
            prelude_key(iface, name),
            FuncType::new(engine, [obj.clone()], [ret]),
            true,
            move |caller, params, results| {
                let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
                let y = i64::from(st_i32(caller, st, 1, &label)?);
                let m = i64::from(st_i32(caller, st, 2, &label)?);
                let d = if year_month_only == 1 {
                    1
                } else {
                    i64::from(st_i32(caller, st, 3, &label)?)
                };
                let date = y_m_d_date(y, m, d, "Temporal.Plain.derivedField")?;
                let val = date_derived_field(date, selector);
                if selector == 8 {
                    *abi_result(results, 0)? = Val::I32(val);
                } else {
                    *abi_result(results, 0)? = Val::F64((val as f64).to_bits());
                }
                Ok(())
            },
        )?;
    }
    reg_plain_month_code_getter(
        linker,
        engine,
        obj,
        &ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(
                crate::runtime::intrinsic_types::build_intrinsic_types(engine)?.string,
            ),
        )),
        iface,
        2,
    )
}

pub(super) fn reg_plain_month_code_getter(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    obj: &ValType,
    string: &ValType,
    iface: &str,
    month_field: u32,
) -> wasmtime::Result<()> {
    let label = format!("{iface}.monthCode");
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key(iface, "monthCode"),
        FuncType::new(engine, [obj.clone()], [string.clone()]),
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let month = st_i32(caller, st, month_field, &label)?;
            let s = write_submilli_string_struct(caller, &format!("M{month:02}"))?;
            *abi_result(results, 0)? = Val::AnyRef(Some(s.to_anyref()));
            Ok(())
        },
    )
}

fn temporal_abi<'a>(caller: &'a Caller<'_, StoreData>) -> wasmtime::Result<&'a TemporalAbi> {
    caller
        .data()
        .host_abi
        .as_ref()
        .map(|abi| &abi.temporal)
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))
}

fn make_plain_struct(
    caller: &mut Caller<'_, StoreData>,
    ty: StructType,
    vtable: Global,
    fields: &[i32],
) -> wasmtime::Result<Val> {
    let mut vals = Vec::with_capacity(fields.len() + 1);
    vals.push(vtable.get(&mut *caller));
    vals.extend(fields.iter().copied().map(Val::I32));
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(&mut *caller, &pre, &vals)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn make_temporal_struct(
    caller: &mut Caller<'_, StoreData>,
    ty: StructType,
    vtable: Global,
    fields: &[Val],
) -> wasmtime::Result<Val> {
    let mut vals = Vec::with_capacity(fields.len() + 1);
    vals.push(vtable.get(&mut *caller));
    vals.extend_from_slice(fields);
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(&mut *caller, &pre, &vals)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

pub(super) fn make_duration(
    caller: &mut Caller<'_, StoreData>,
    span: &Span,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.duration.clone(), abi.duration_vtable)
    };
    let mut fields = vec![Val::I32(0); 10];
    write_span(&mut fields, span)?;
    make_temporal_struct(caller, ty, vtable, &fields)
}

pub(super) fn make_instant(
    caller: &mut Caller<'_, StoreData>,
    ts: Timestamp,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.instant.clone(), abi.instant_vtable)
    };
    make_temporal_struct(
        caller,
        ty,
        vtable,
        &[Val::I64(ts.as_second()), Val::I32(ts.subsec_nanosecond())],
    )
}

pub(super) fn instant_from_struct(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    label: &'static str,
) -> wasmtime::Result<Timestamp> {
    Timestamp::new(st_i64(caller, st, 1, label)?, st_i32(caller, st, 2, label)?)
        .map_err(temporal_err(label))
}

pub(super) fn instant_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<Timestamp> {
    let st = as_struct_val(caller, val, label)?;
    instant_from_struct(caller, st, label)
}

pub(super) fn epoch_milliseconds(ts: Timestamp) -> f64 {
    ts.as_nanosecond().div_euclid(1_000_000) as f64
}

pub(super) fn make_zoned_date_time(
    caller: &mut Caller<'_, StoreData>,
    zoned: &Zoned,
    tz_id: &str,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.zoned_date_time.clone(), abi.zoned_date_time_vtable)
    };
    let ts = zoned.timestamp();
    let tz = write_submilli_string_struct(caller, tz_id)?;
    make_temporal_struct(
        caller,
        ty,
        vtable,
        &[
            Val::I64(ts.as_second()),
            Val::I32(ts.subsec_nanosecond()),
            Val::AnyRef(Some(tz.to_anyref())),
        ],
    )
}

pub(super) fn make_plain_date(
    caller: &mut Caller<'_, StoreData>,
    y: i32,
    m: i32,
    d: i32,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_date.clone(), abi.plain_date_vtable)
    };
    make_plain_struct(caller, ty, vtable, &[y, m, d])
}

pub(super) fn make_plain_time(
    caller: &mut Caller<'_, StoreData>,
    h: i32,
    m: i32,
    s: i32,
    ns: i32,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_time.clone(), abi.plain_time_vtable)
    };
    make_plain_struct(caller, ty, vtable, &[h, m, s, ns])
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make_plain_date_time(
    caller: &mut Caller<'_, StoreData>,
    y: i32,
    m: i32,
    d: i32,
    h: i32,
    mi: i32,
    s: i32,
    ns: i32,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_date_time.clone(), abi.plain_date_time_vtable)
    };
    make_plain_struct(caller, ty, vtable, &[y, m, d, h, mi, s, ns])
}

pub(super) fn make_plain_year_month(
    caller: &mut Caller<'_, StoreData>,
    y: i32,
    m: i32,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_year_month.clone(), abi.plain_year_month_vtable)
    };
    make_plain_struct(caller, ty, vtable, &[y, m])
}

pub(super) fn make_plain_month_day(
    caller: &mut Caller<'_, StoreData>,
    m: i32,
    d: i32,
) -> wasmtime::Result<Val> {
    let (ty, vtable) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_month_day.clone(), abi.plain_month_day_vtable)
    };
    make_temporal_struct(
        caller,
        ty,
        vtable,
        &[Val::I32(m), Val::I32(d), Val::I64(PLAIN_MONTH_DAY_MARKER)],
    )
}

pub(super) fn plain_date_from_struct(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    label: &str,
) -> wasmtime::Result<civil::Date> {
    y_m_d_date(
        i64::from(st_i32(caller, st, 1, label)?),
        i64::from(st_i32(caller, st, 2, label)?),
        i64::from(st_i32(caller, st, 3, label)?),
        "PlainDate",
    )
}

pub(super) fn plain_time_from_struct(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    field_offset: u32,
    label: &str,
) -> wasmtime::Result<civil::Time> {
    h_m_s_ns_time(
        i64::from(st_i32(caller, st, field_offset + 1, label)?),
        i64::from(st_i32(caller, st, field_offset + 2, label)?),
        i64::from(st_i32(caller, st, field_offset + 3, label)?),
        i64::from(st_i32(caller, st, field_offset + 4, label)?),
        label,
    )
}

pub(super) fn st_i32_or(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    field: u32,
) -> i32 {
    match st.field(caller, field as usize) {
        Ok(Val::I32(v)) => v,
        _ => 0,
    }
}

fn st_i64(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    field: u32,
    label: &str,
) -> wasmtime::Result<i64> {
    match st.field(caller, field as usize)? {
        Val::I64(v) => Ok(v),
        other => wasmtime::bail!("Temporal.{label}: expected i64 field {field}, got {other:?}"),
    }
}

fn st_string(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    field: u32,
    label: &str,
) -> wasmtime::Result<String> {
    let val = st.field(&mut *caller, field as usize)?;
    read_string_arg(caller, &val, label)
}

pub(super) fn span_from_duration_struct(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    label: &str,
) -> wasmtime::Result<Span> {
    let mut fields = Vec::with_capacity(10);
    for i in 1..=10 {
        fields.push(st.field(&mut *caller, i)?);
    }
    let fields = read_span_fields(&fields)?;
    super::duration::from_fields(fields, label).map_err(crate::runtime::host::range_error)
}

pub(super) fn zoned_date_time_from_struct(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    label: &'static str,
) -> wasmtime::Result<Zoned> {
    let secs = st_i64(caller, st, 1, label)?;
    let nanos = st_i32(caller, st, 2, label)?;
    let tz_id = st_string(caller, st, 3, label)?;
    make_zoned(caller, secs, nanos, &tz_id, label)
}

pub(super) fn zoned_date_time_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<Zoned> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    zoned_date_time_from_struct(caller, st, label)
}

pub(super) fn zoned_date_time_timestamp_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<Timestamp> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    instant_from_struct(caller, st, label)
}

pub(super) fn zoned_date_time_identity_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<(Timestamp, String)> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    let timestamp = instant_from_struct(caller, st, label)?;
    let time_zone_id = st_string(caller, st, 3, label)?;
    Ok((timestamp, time_zone_id))
}

pub(super) fn zoned_date_time_time_zone_id_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<String> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    st_string(caller, st, 3, label)
}

pub(super) fn zoned_date_time_parts_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<(Zoned, String)> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    let secs = st_i64(caller, st, 1, label)?;
    let nanos = st_i32(caller, st, 2, label)?;
    let tz_id = st_string(caller, st, 3, label)?;
    let zoned = make_zoned(caller, secs, nanos, &tz_id, label)?;
    Ok((zoned, tz_id))
}

pub(super) fn read_duration_like(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    intr: &IntrinsicTypes,
    label: &str,
) -> wasmtime::Result<Span> {
    if let Val::AnyRef(Some(any)) = val
        && let Some(st) = any.as_struct(&mut *caller)?
        && st.matches_ty(&*caller, &intr.temporal_duration)?
    {
        return span_from_duration_struct(caller, st, label);
    }

    let mut fields = [0; 10];
    for (index, name) in DURATION_FIELD_NAMES.iter().enumerate() {
        fields[index] = object_duration_field(caller, val, name, index, label)?.unwrap_or(0);
    }
    super::duration::from_fields(fields, label).map_err(crate::runtime::host::range_error)
}

pub(super) fn duration_field_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    index: usize,
    label: &'static str,
) -> wasmtime::Result<i64> {
    let ty = temporal_abi(caller)?.duration.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    duration_struct_field(caller, st, index, label)
}

pub(super) fn duration_sign_from_val(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    label: &'static str,
) -> wasmtime::Result<i32> {
    let ty = temporal_abi(caller)?.duration.clone();
    let st = cast_struct(caller, val, &ty, label)?;
    for index in 0..DURATION_FIELD_NAMES.len() {
        let value = duration_struct_field(caller, st, index, label)?;
        if value != 0 {
            return Ok(value.signum() as i32);
        }
    }
    Ok(0)
}

fn duration_struct_field(
    caller: &mut Caller<'_, StoreData>,
    st: Rooted<StructRef>,
    index: usize,
    label: &str,
) -> wasmtime::Result<i64> {
    let field = u32::try_from(index + 1)
        .map_err(|_| wasmtime::Error::msg("Temporal.Duration: invalid field index"))?;
    if index < 4 {
        st_i32(caller, st, field, label).map(i64::from)
    } else {
        st_i64(caller, st, field, label)
    }
}

pub(super) const DURATION_FIELD_NAMES: [&str; 10] = [
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];

pub(super) fn object_time_bag(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    label: &'static str,
) -> wasmtime::Result<[Option<i64>; 6]> {
    Ok([
        object_i64_field(caller, obj, "hour", label)?,
        object_i64_field(caller, obj, "minute", label)?,
        object_i64_field(caller, obj, "second", label)?,
        object_i64_field(caller, obj, "millisecond", label)?,
        object_i64_field(caller, obj, "microsecond", label)?,
        object_i64_field(caller, obj, "nanosecond", label)?,
    ])
}

pub(super) fn object_i64_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    label: &str,
) -> wasmtime::Result<Option<i64>> {
    let Some(f) = object_integer_f64_field(caller, obj, name, label)? else {
        return Ok(None);
    };
    if f <= -I64_BOUND_EXCLUSIVE || f >= I64_BOUND_EXCLUSIVE {
        return Err(crate::runtime::host::range_error(format!(
            "Temporal.{label}{{{name}}}: value {f} is outside the supported integer range; use a smaller absolute value"
        )));
    }
    Ok(Some(f as i64))
}

pub(super) fn object_duration_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    index: usize,
    label: &str,
) -> wasmtime::Result<Option<i64>> {
    let Some(value) = object_integer_f64_field(caller, obj, name, label)? else {
        return Ok(None);
    };
    super::duration::field_from_f64(value, index, label)
        .map(Some)
        .map_err(crate::runtime::host::range_error)
}

fn object_integer_f64_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    label: &str,
) -> wasmtime::Result<Option<f64>> {
    let Some(value) = object_f64_field(caller, obj, name, label)? else {
        return Ok(None);
    };
    if !value.is_finite() {
        return Err(crate::runtime::host::range_error(format!(
            "Temporal.{label}{{{name}}}: non-finite value"
        )));
    }
    if value.fract() != 0.0 {
        return Err(crate::runtime::host::range_error(format!(
            "Temporal.{label}{{{name}}}: non-integer value {value}"
        )));
    }
    Ok(Some(value))
}

pub(super) fn object_string_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    label: &str,
) -> wasmtime::Result<Option<String>> {
    let Some(v) = object_field(caller, obj, name)? else {
        return Ok(None);
    };
    match v {
        Val::AnyRef(None) => Ok(None),
        _ => read_string_arg(caller, &v, label).map(Some),
    }
}

pub(super) fn object_diff_options(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    label: &'static str,
) -> wasmtime::Result<DiffOptions> {
    let smallest = object_string_field(caller, obj, "smallestUnit", label)?
        .map(|unit| unit_from_str(&unit, label))
        .transpose()?;
    let largest = object_string_field(caller, obj, "largestUnit", label)?
        .map(|unit| unit_from_str(&unit, label))
        .transpose()?;
    let mode = object_string_field(caller, obj, "roundingMode", label)?
        .map(|mode| round_mode_from_str(&mode, label))
        .transpose()?;
    let increment = object_i64_field(caller, obj, "roundingIncrement", label)?;
    Ok(DiffOptions {
        smallest,
        largest,
        mode,
        increment,
    })
}

fn object_f64_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    label: &str,
) -> wasmtime::Result<Option<f64>> {
    let Some(v) = object_field(caller, obj, name)? else {
        return Ok(None);
    };
    let boxed_number_type = caller
        .data()
        .host_abi
        .as_ref()
        .map(|abi| abi.boxed_number_type.clone())
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
    let Val::AnyRef(Some(any)) = v else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        wasmtime::bail!("Temporal.{label}{{{name}}}: expected boxed number");
    };
    if !st.matches_ty(&*caller, &boxed_number_type)? {
        wasmtime::bail!("Temporal.{label}{{{name}}}: expected boxed number");
    }
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(Some(f64::from_bits(bits))),
        other => wasmtime::bail!("Temporal.{label}{{{name}}}: expected f64, got {other:?}"),
    }
}

pub fn temporal_module_package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(TEMPORAL_MODULE_NAME);
    let temporal_prefix = crate::mangle::host(TEMPORAL_MODULE_NAME, "Temporal");

    let temporal = NamespaceSymbol {
        name: "Temporal".to_string(),
        mangled_prefix: temporal_prefix.clone(),
        declaration_span: DiagSpan::at(crate::FileId::TEMPORAL),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    };

    // No symbols here; the prelude is the user surface. Inserting an empty
    // Temporal namespace lets load_namespaces merge it with the prelude's as a no-op.
    let _ = &temporal_prefix;

    defs.namespaces.insert("Temporal".to_string(), temporal);
    defs
}

pub(super) fn temporal_type(name: &str) -> Type {
    Type::interface_ref(
        Package::prelude(),
        format!("Temporal.{name}"),
        crate::mangle::extend(&crate::mangle::prelude("Temporal"), name),
        Vec::new(),
    )
}

pub(super) fn object_shape_type(optional_numbers: &[&str]) -> Type {
    let fields = optional_numbers
        .iter()
        .map(|name| (name.to_string(), ObjectField::optional(Type::Number)))
        .collect();
    Type::Object {
        index: None,
        fields,
    }
}

pub(super) fn declare_direct_fn(
    defs: &mut PackageDeclaration,
    name: &str,
    mangled_name: MangledName,
    params: Vec<Param>,
    ret: Type,
) {
    defs.values.insert(
        mangled_name.as_str().to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name,
            declaration_span: DiagSpan::at(crate::FileId::TEMPORAL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: None,
            },
        },
    );
}

pub(super) fn clamp_month(m: i64) -> i64 {
    m.clamp(1, 12)
}

pub(super) fn days_in_month(year: i64, month: i64, label: &'static str) -> wasmtime::Result<i64> {
    let d = y_m_d_date(year, month, 1, label)?;
    Ok(i64::from(d.days_in_month()))
}

fn date_derived_field(d: civil::Date, selector: i32) -> i32 {
    match selector {
        0 => i32::from(d.weekday().to_monday_one_offset()),
        1 => i32::from(d.day_of_year()),
        2 => i32::from(d.iso_week_date().week()),
        3 => i32::from(d.iso_week_date().year()),
        4 => 7,
        5 => i32::from(d.days_in_month()),
        6 => i32::from(d.days_in_year()),
        7 => 12,
        8 => i32::from(d.in_leap_year()),
        _ => 0,
    }
}

pub(super) fn y_m_d_date(
    year: i64,
    month: i64,
    day: i64,
    label: &'static str,
) -> wasmtime::Result<civil::Date> {
    let year = checked_field(year, -9_999, 9_999, label, "year")?;
    let month = checked_field(month, 1, 12, label, "month")?;
    let day = checked_field(day, 1, 31, label, "day")?;
    civil::Date::new(year, month, day).map_err(|_| {
        crate::runtime::host::range_error(format!(
            "Temporal.{label}: the resulting date is outside the representable range"
        ))
    })
}

fn checked_field<T>(
    value: i64,
    minimum: i64,
    maximum: i64,
    label: &str,
    field: &str,
) -> wasmtime::Result<T>
where
    T: TryFrom<i64>,
{
    let range_error = || {
        crate::runtime::host::range_error(format!(
            "Temporal.{label}{{{field}}}: value {value} is outside the supported range {minimum}..={maximum}; use a {field} within that range"
        ))
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(range_error());
    }
    T::try_from(value).map_err(|_| range_error())
}

fn h_m_s_ns_time(
    hour: i64,
    minute: i64,
    second: i64,
    nanosecond: i64,
    label: &str,
) -> wasmtime::Result<civil::Time> {
    let hour = checked_field(hour, 0, 23, label, "hour")?;
    let minute = checked_field(minute, 0, 59, label, "minute")?;
    let second = checked_field(second, 0, 59, label, "second")?;
    let nanosecond = checked_field(nanosecond, 0, 999_999_999, label, "nanosecond")?;
    civil::Time::new(hour, minute, second, nanosecond).map_err(|_| {
        crate::runtime::host::range_error(format!(
            "Temporal.{label}: the resulting time is outside the representable range"
        ))
    })
}

/// Merge + constrain a date: month -> [1,12], day -> [1, days-in-month].
pub(super) fn merge_date(
    recv: (i32, i32, i32),
    bag_year: Option<i64>,
    bag_month: Option<i64>,
    bag_day: Option<i64>,
    label: &'static str,
) -> wasmtime::Result<(i64, i64, i64)> {
    let year = bag_year.unwrap_or_else(|| i64::from(recv.0));
    let month = clamp_month(bag_month.unwrap_or_else(|| i64::from(recv.1)));
    let dim = days_in_month(year, month, label)?;
    let day = bag_day.unwrap_or_else(|| i64::from(recv.2)).clamp(1, dim);
    Ok((year, month, day))
}

/// Merge + constrain a time. The receiver's combined sub-second slot is split
/// into millisecond/microsecond/nanosecond components so the bag can override any
/// of them; the merged components are clamped and recombined.
pub(super) fn merge_time(
    recv: (i32, i32, i32, i32),
    bag: [Option<i64>; 6],
    label: &'static str,
) -> wasmtime::Result<civil::Time> {
    let hour = bag[0].unwrap_or_else(|| i64::from(recv.0)).clamp(0, 23);
    let minute = bag[1].unwrap_or_else(|| i64::from(recv.1)).clamp(0, 59);
    let second = bag[2].unwrap_or_else(|| i64::from(recv.2)).clamp(0, 59);
    let sub = i64::from(recv.3);
    let ms: i64 = checked_field(
        bag[3].unwrap_or(sub / 1_000_000).clamp(0, 999),
        0,
        999,
        label,
        "millisecond",
    )?;
    let us: i64 = checked_field(
        bag[4].unwrap_or((sub / 1_000) % 1_000).clamp(0, 999),
        0,
        999,
        label,
        "microsecond",
    )?;
    let ns: i64 = checked_field(
        bag[5].unwrap_or(sub % 1_000).clamp(0, 999),
        0,
        999,
        label,
        "nanosecond",
    )?;
    let subsec = ms * 1_000_000 + us * 1_000 + ns;
    h_m_s_ns_time(hour, minute, second, subsec, label)
}

/// The reference `civil::Date` for PlainYearMonth arithmetic: day 1 of the month,
/// or its last day when `negative` (Temporal anchors a backwards shift on the end
/// of the month so day-of-month rollover behaves).
pub(super) fn year_month_reference(
    y: i32,
    m: i32,
    negative: bool,
    label: &'static str,
) -> wasmtime::Result<civil::Date> {
    let first = y_m_d_date(i64::from(y), i64::from(m), 1, label)?;
    Ok(if negative {
        first.last_of_month()
    } else {
        first
    })
}

/// Exact `until` span from (ay, am) to (by, bm) as whole months split into
/// years + months.
/// The trailing (smallestUnit, largestUnit, roundingMode, roundingIncrement)
/// options every `until`/`since` host fn carries. Absent options leave jiff's
/// per-type defaults untouched, so option-free calls behave exactly as before.
pub(super) struct DiffOptions {
    smallest: Option<Unit>,
    largest: Option<Unit>,
    mode: Option<RoundMode>,
    increment: Option<i64>,
}

impl DiffOptions {
    fn is_default(&self) -> bool {
        self.smallest.is_none()
            && self.largest.is_none()
            && self.mode.is_none()
            && self.increment.is_none()
    }

    pub(super) fn timestamp(&self, to: Timestamp) -> TimestampDifference {
        let mut d = TimestampDifference::new(to);
        if let Some(u) = self.smallest {
            d = d.smallest(u);
        }
        if let Some(u) = self.largest {
            d = d.largest(u);
        }
        if let Some(m) = self.mode {
            d = d.mode(m);
        }
        if let Some(i) = self.increment {
            d = d.increment(i);
        }
        d
    }

    pub(super) fn zoned<'a>(&self, to: &'a Zoned) -> ZonedDifference<'a> {
        let mut d = ZonedDifference::new(to);
        if let Some(u) = self.smallest {
            d = d.smallest(u);
        }
        if let Some(u) = self.largest {
            d = d.largest(u);
        }
        if let Some(m) = self.mode {
            d = d.mode(m);
        }
        if let Some(i) = self.increment {
            d = d.increment(i);
        }
        d
    }

    pub(super) fn description(&self) -> String {
        round_options_description(self.smallest, self.largest, self.mode, self.increment)
    }

    pub(super) fn date(&self, to: civil::Date) -> civil::DateDifference {
        let mut d = civil::DateDifference::new(to);
        if let Some(u) = self.smallest {
            d = d.smallest(u);
        }
        if let Some(u) = self.largest {
            d = d.largest(u);
        }
        if let Some(m) = self.mode {
            d = d.mode(m);
        }
        if let Some(i) = self.increment {
            d = d.increment(i);
        }
        d
    }

    pub(super) fn time(&self, to: civil::Time) -> civil::TimeDifference {
        let mut d = civil::TimeDifference::new(to);
        if let Some(u) = self.smallest {
            d = d.smallest(u);
        }
        if let Some(u) = self.largest {
            d = d.largest(u);
        }
        if let Some(m) = self.mode {
            d = d.mode(m);
        }
        if let Some(i) = self.increment {
            d = d.increment(i);
        }
        d
    }

    pub(super) fn datetime(&self, to: civil::DateTime) -> civil::DateTimeDifference {
        let mut d = civil::DateTimeDifference::new(to);
        if let Some(u) = self.smallest {
            d = d.smallest(u);
        }
        if let Some(u) = self.largest {
            d = d.largest(u);
        }
        if let Some(m) = self.mode {
            d = d.mode(m);
        }
        if let Some(i) = self.increment {
            d = d.increment(i);
        }
        d
    }
}

/// Year-month difference with options. The option-free path keeps the exact
/// integer month split; with options the pair anchors on day 1 and rounds as
/// dates. Only year/month units are meaningful for year-months.
pub(super) fn year_month_span(
    ay: i32,
    am: i32,
    by: i32,
    bm: i32,
    o: &DiffOptions,
    label: &'static str,
) -> wasmtime::Result<Span> {
    if o.is_default() {
        return year_month_diff(ay, am, by, bm);
    }
    for unit in [o.smallest, o.largest].into_iter().flatten() {
        if !matches!(unit, Unit::Year | Unit::Month) {
            return Err(crate::runtime::host::range_error(format!(
                "{label}: largestUnit/smallestUnit must be \"years\" or \"months\" for a PlainYearMonth"
            )));
        }
    }
    let a = y_m_d_date(i64::from(ay), i64::from(am), 1, label)?;
    let b = y_m_d_date(i64::from(by), i64::from(bm), 1, label)?;
    let mut d = civil::DateDifference::new(b).largest(o.largest.unwrap_or(Unit::Year));
    if let Some(u) = o.smallest {
        d = d.smallest(u);
    }
    if let Some(m) = o.mode {
        d = d.mode(m);
    }
    if let Some(i) = o.increment {
        d = d.increment(i);
    }
    a.until(d).map_err(temporal_err(label))
}

fn year_month_diff(ay: i32, am: i32, by: i32, bm: i32) -> wasmtime::Result<Span> {
    let months = (by - ay) * 12 + (bm - am);
    Span::new()
        .try_years(months / 12)
        .and_then(|s| s.try_months(months % 12))
        .map_err(temporal_err("PlainYearMonth.until"))
}

/// Maps a `jiff` failure to a Temporal-native message. We deliberately drop the
/// underlying error text — it leaks crate internals (`jiff::Span`, …) that the
/// LLM consumer can't act on — and describe the failure in domain terms. These
/// operations only fail on out-of-range / overflow, so one phrasing fits — and
/// per the Temporal spec they throw `RangeError`.
pub(super) fn temporal_error(label: &str, detail: impl std::fmt::Display) -> String {
    format!("Temporal.{label}: {detail}")
}

pub(super) fn temporal_err(label: &'static str) -> impl Fn(jiff::Error) -> wasmtime::Error {
    move |_e| {
        crate::runtime::host::range_error(temporal_error(
            label,
            "a date or time value is out of the representable range",
        ))
    }
}

fn read_span_fields(slice: &[Val]) -> wasmtime::Result<[i64; 10]> {
    Ok([
        i64::from(read_i32(&slice[0], "Span.years")?),
        i64::from(read_i32(&slice[1], "Span.months")?),
        i64::from(read_i32(&slice[2], "Span.weeks")?),
        i64::from(read_i32(&slice[3], "Span.days")?),
        read_i64(&slice[4], "Span.hours")?,
        read_i64(&slice[5], "Span.minutes")?,
        read_i64(&slice[6], "Span.seconds")?,
        read_i64(&slice[7], "Span.milliseconds")?,
        read_i64(&slice[8], "Span.microseconds")?,
        read_i64(&slice[9], "Span.nanoseconds")?,
    ])
}

// `jiff::Span`'s per-unit getters return narrower types than our carrier
// slots (i16 for years, i32 for hours, etc.). Widening conversions are
// always lossless; suppress `clippy::useless_conversion` so future jiff
// versions that widen a getter don't silently lose information.
#[allow(clippy::useless_conversion)]
fn write_span(results: &mut [Val], span: &Span) -> wasmtime::Result<()> {
    *abi_result(results, 0)? = Val::I32(i32::from(span.get_years()));
    *abi_result(results, 1)? = Val::I32(i32::from(span.get_months()));
    *abi_result(results, 2)? = Val::I32(span.get_weeks());
    *abi_result(results, 3)? = Val::I32(span.get_days());
    *abi_result(results, 4)? = Val::I64(i64::from(span.get_hours()));
    *abi_result(results, 5)? = Val::I64(span.get_minutes());
    *abi_result(results, 6)? = Val::I64(span.get_seconds());
    *abi_result(results, 7)? = Val::I64(span.get_milliseconds());
    *abi_result(results, 8)? = Val::I64(span.get_microseconds());
    *abi_result(results, 9)? = Val::I64(span.get_nanoseconds());
    Ok(())
}

fn read_i32(v: &Val, name: &str) -> wasmtime::Result<i32> {
    match v {
        Val::I32(x) => Ok(*x),
        other => wasmtime::bail!("{name}: expected i32, got {other:?}"),
    }
}

fn read_i64(v: &Val, name: &str) -> wasmtime::Result<i64> {
    match v {
        Val::I64(x) => Ok(*x),
        other => wasmtime::bail!("{name}: expected i64, got {other:?}"),
    }
}

fn make_zoned(
    caller: &mut Caller<'_, StoreData>,
    secs: i64,
    nanos: i32,
    tz_id: &str,
    label: &'static str,
) -> wasmtime::Result<Zoned> {
    let (tz, _) = super::zoned_date_time::resolve_time_zone(caller, tz_id, label)?;
    let ts = Timestamp::new(secs, nanos).map_err(|_| {
        crate::runtime::host::range_error(format!(
            "Temporal.{label}: the instant is outside the representable range"
        ))
    })?;
    Ok(ts.to_zoned(tz))
}

/// A `relativeTo` anchor for calendar-aware Duration operations.
pub(super) enum Anchor {
    Date(civil::Date),
    Zoned(Zoned),
}

pub(super) fn object_relative_to(
    caller: &mut Caller<'_, StoreData>,
    options: &Val,
    label: &'static str,
) -> wasmtime::Result<Option<Anchor>> {
    let Some(value) = object_field(caller, options, "relativeTo")? else {
        return Ok(None);
    };
    let Val::AnyRef(Some(any)) = value else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        wasmtime::bail!("Temporal.{label}: relativeTo must be a PlainDate or ZonedDateTime");
    };
    let (plain_date, zoned_date_time) = {
        let abi = temporal_abi(caller)?;
        (abi.plain_date.clone(), abi.zoned_date_time.clone())
    };
    if st.matches_ty(&*caller, &plain_date)? {
        return plain_date_from_struct(caller, st, label)
            .map(Anchor::Date)
            .map(Some);
    }
    if st.matches_ty(&*caller, &zoned_date_time)? {
        return zoned_date_time_from_struct(caller, st, label)
            .map(Anchor::Zoned)
            .map(Some);
    }
    wasmtime::bail!("Temporal.{label}: relativeTo must be a PlainDate or ZonedDateTime")
}

pub(super) fn read_f64(v: &Val, name: &str) -> wasmtime::Result<f64> {
    match v {
        Val::F64(bits) => Ok(f64::from_bits(*bits)),
        other => wasmtime::bail!("{name}: expected f64, got {other:?}"),
    }
}

/// Map a Temporal unit option (singular or plural — both accepted) to `jiff::Unit`.
pub(super) fn unit_from_str(unit: &str, label: &str) -> wasmtime::Result<Unit> {
    Ok(match unit {
        "nanosecond" | "nanoseconds" => Unit::Nanosecond,
        "microsecond" | "microseconds" => Unit::Microsecond,
        "millisecond" | "milliseconds" => Unit::Millisecond,
        "second" | "seconds" => Unit::Second,
        "minute" | "minutes" => Unit::Minute,
        "hour" | "hours" => Unit::Hour,
        "day" | "days" => Unit::Day,
        "week" | "weeks" => Unit::Week,
        "month" | "months" => Unit::Month,
        "year" | "years" => Unit::Year,
        other => {
            return Err(crate::runtime::host::range_error(format!(
                "Temporal.{label}: unknown unit {other:?}; use year, month, week, day, hour, minute, second, millisecond, microsecond, or nanosecond"
            )));
        }
    })
}

pub(super) fn round_mode_from_str(mode: &str, label: &str) -> wasmtime::Result<RoundMode> {
    Ok(match mode {
        "ceil" => RoundMode::Ceil,
        "floor" => RoundMode::Floor,
        "expand" => RoundMode::Expand,
        "trunc" => RoundMode::Trunc,
        "halfCeil" => RoundMode::HalfCeil,
        "halfFloor" => RoundMode::HalfFloor,
        "halfExpand" => RoundMode::HalfExpand,
        "halfTrunc" => RoundMode::HalfTrunc,
        "halfEven" => RoundMode::HalfEven,
        other => {
            return Err(crate::runtime::host::range_error(format!(
                "Temporal.{label}: unknown roundingMode {other:?}; use ceil, floor, expand, trunc, halfCeil, halfFloor, halfExpand, halfTrunc, or halfEven"
            )));
        }
    })
}

fn temporal_option(value: Option<&str>) -> String {
    value.map_or_else(|| "omitted".to_string(), |value| format!("{value:?}"))
}

pub(super) fn string_option_description(value: Option<&str>) -> String {
    temporal_option(value)
}

pub(super) fn i64_option_description(value: Option<i64>) -> String {
    value.map_or_else(|| "omitted".to_string(), |value| value.to_string())
}

pub(super) fn temporal_unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Year => "year",
        Unit::Month => "month",
        Unit::Week => "week",
        Unit::Day => "day",
        Unit::Hour => "hour",
        Unit::Minute => "minute",
        Unit::Second => "second",
        Unit::Millisecond => "millisecond",
        Unit::Microsecond => "microsecond",
        Unit::Nanosecond => "nanosecond",
    }
}

fn temporal_round_mode_name(mode: RoundMode) -> &'static str {
    match mode {
        RoundMode::Ceil => "ceil",
        RoundMode::Floor => "floor",
        RoundMode::Expand => "expand",
        RoundMode::Trunc => "trunc",
        RoundMode::HalfCeil => "halfCeil",
        RoundMode::HalfFloor => "halfFloor",
        RoundMode::HalfExpand => "halfExpand",
        RoundMode::HalfTrunc => "halfTrunc",
        RoundMode::HalfEven => "halfEven",
        _ => "unsupported",
    }
}

pub(super) fn round_options_description(
    smallest: Option<Unit>,
    largest: Option<Unit>,
    mode: Option<RoundMode>,
    increment: Option<i64>,
) -> String {
    let smallest = temporal_option(smallest.map(temporal_unit_name));
    let largest = temporal_option(largest.map(temporal_unit_name));
    let mode = temporal_option(mode.map(temporal_round_mode_name));
    let increment = i64_option_description(increment);
    format!(
        "smallestUnit={smallest}, largestUnit={largest}, roundingMode={mode}, roundingIncrement={increment}"
    )
}

pub(super) fn duration_to_string_with_options(
    span: &Span,
    smallest: Option<&str>,
    mode: Option<&str>,
    fractional_digits: Option<u8>,
) -> wasmtime::Result<String> {
    let rounded = round_span_for_to_string(span, smallest, mode, fractional_digits)?;
    if let Some(digits) = fractional_digits {
        return format_duration_with_fractional_digits(&rounded, digits);
    }
    Ok(rounded.to_string())
}

fn round_span_for_to_string(
    span: &Span,
    smallest: Option<&str>,
    mode: Option<&str>,
    fractional_digits: Option<u8>,
) -> wasmtime::Result<Span> {
    if smallest.is_none() && mode.is_none() && fractional_digits.is_none() {
        return Ok(*span);
    }

    let mut round = SpanRound::new();
    let smallest_unit = smallest
        .map(|unit| unit_from_str(unit, "Duration.toString"))
        .transpose()?;
    let fractional_rounding = fractional_digits
        .map(rounding_for_fractional_second_digits)
        .transpose()?;
    let fractional_unit = fractional_rounding.map(|rounding| rounding.0);
    let effective_unit = match (smallest_unit, fractional_unit) {
        (Some(unit), Some(fractional)) => Some(more_specific_unit(unit, fractional)),
        (Some(unit), None) => Some(unit),
        (None, Some(unit)) => Some(unit),
        (None, None) => None,
    };
    if let Some(unit) = effective_unit {
        round = round.smallest(unit);
    }
    if let Some(mode) = mode {
        round = round.mode(round_mode_from_str(mode, "Duration.toString")?);
    }
    if let Some(digits) = fractional_digits {
        let (unit, increment) = rounding_for_fractional_second_digits(digits)?;
        if increment > 1 && Some(unit) == effective_unit {
            round = round.increment(increment);
        }
    }
    span.round(round).map_err(temporal_err("Duration.toString"))
}

fn rounding_for_fractional_second_digits(digits: u8) -> wasmtime::Result<(Unit, i64)> {
    Ok(match digits {
        0 => (Unit::Second, 1),
        1 => (Unit::Millisecond, 100),
        2 => (Unit::Millisecond, 10),
        3 => (Unit::Millisecond, 1),
        4 => (Unit::Microsecond, 100),
        5 => (Unit::Microsecond, 10),
        6 => (Unit::Microsecond, 1),
        7 => (Unit::Nanosecond, 100),
        8 => (Unit::Nanosecond, 10),
        9 => (Unit::Nanosecond, 1),
        _ => {
            return Err(crate::runtime::host::invariant_trap(
                "Temporal: fractionalSecondDigits outside 0..=9",
            ));
        }
    })
}

fn fractional_second_increment(digits: u8) -> wasmtime::Result<i64> {
    let exponent = 9u8.checked_sub(digits).ok_or_else(|| {
        crate::runtime::host::invariant_trap("Temporal: invalid fractional digits")
    })?;
    Ok(10_i64.pow(u32::from(exponent)))
}

fn more_specific_unit(a: Unit, b: Unit) -> Unit {
    if unit_specificity(a) <= unit_specificity(b) {
        a
    } else {
        b
    }
}

fn unit_specificity(unit: Unit) -> u8 {
    match unit {
        Unit::Nanosecond => 0,
        Unit::Microsecond => 1,
        Unit::Millisecond => 2,
        Unit::Second => 3,
        Unit::Minute => 4,
        Unit::Hour => 5,
        Unit::Day => 6,
        Unit::Week => 7,
        Unit::Month => 8,
        Unit::Year => 9,
    }
}

fn format_duration_with_fractional_digits(span: &Span, digits: u8) -> wasmtime::Result<String> {
    let divisor = u128::try_from(fractional_second_increment(digits)?)
        .map_err(|_| crate::runtime::host::invariant_trap("Temporal: invalid fraction divisor"))?;
    let negative = span_fields(span).into_iter().any(|value| value < 0);
    let years = span.get_years().unsigned_abs();
    let months = span.get_months().unsigned_abs();
    let weeks = span.get_weeks().unsigned_abs();
    let days = span.get_days().unsigned_abs();
    let hours = span.get_hours().unsigned_abs();
    let minutes = span.get_minutes().unsigned_abs();
    let seconds = span.get_seconds().unsigned_abs();
    let milliseconds = span.get_milliseconds().unsigned_abs();
    let microseconds = span.get_microseconds().unsigned_abs();
    let nanoseconds = span.get_nanoseconds().unsigned_abs();
    let has_date = years != 0 || months != 0 || weeks != 0 || days != 0;
    let has_time = hours != 0
        || minutes != 0
        || seconds != 0
        || milliseconds != 0
        || microseconds != 0
        || nanoseconds != 0;

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push('P');
    if years != 0 {
        out.push_str(&format!("{years}Y"));
    }
    if months != 0 {
        out.push_str(&format!("{months}M"));
    }
    if weeks != 0 {
        out.push_str(&format!("{weeks}W"));
    }
    if days != 0 {
        out.push_str(&format!("{days}D"));
    }
    if has_time || !has_date {
        out.push('T');
        if hours != 0 {
            out.push_str(&format!("{hours}H"));
        }
        if minutes != 0 {
            out.push_str(&format!("{minutes}M"));
        }
        let subsecond_nanos = u128::from(milliseconds) * 1_000_000
            + u128::from(microseconds) * 1_000
            + u128::from(nanoseconds);
        let include_seconds = seconds != 0 || subsecond_nanos != 0 || digits > 0 || !has_time;
        if include_seconds {
            out.push_str(&seconds.to_string());
            if digits > 0 {
                let fraction = subsecond_nanos / divisor;
                out.push('.');
                out.push_str(&format!("{fraction:0width$}", width = usize::from(digits)));
            }
            out.push('S');
        }
    }
    Ok(out)
}

pub(super) fn span_fields(span: &Span) -> [i64; 10] {
    [
        i64::from(span.get_years()),
        i64::from(span.get_months()),
        i64::from(span.get_weeks()),
        i64::from(span.get_days()),
        i64::from(span.get_hours()),
        span.get_minutes(),
        span.get_seconds(),
        span.get_milliseconds(),
        span.get_microseconds(),
        span.get_nanoseconds(),
    ]
}

pub(super) fn span_total(span: &Span, unit: &str, anchor: Option<Anchor>) -> wasmtime::Result<f64> {
    let unit_enum = unit_from_str(unit, "Duration.total")?;
    if anchor.is_none()
        && (super::duration::has_calendar_units(span)
            || super::duration::is_calendar_unit(unit_enum))
    {
        return Err(crate::runtime::host::range_error(
            super::duration::calendar_anchor_error("Duration.total", "compute this total"),
        ));
    }
    if span_fields(span).into_iter().all(|value| value == 0) {
        return Ok(0.0);
    }
    let total = match anchor {
        Some(Anchor::Zoned(z)) => span.total((unit_enum, &z)),
        Some(Anchor::Date(d)) => span.total((unit_enum, SpanRelativeTo::from(d))),
        None => {
            // Time/day units resolve against a sentinel UTC anchor (days = 24h,
            // no DST) once calendar-unit requests have been rejected above.
            let sentinel = civil::date(2000, 1, 1)
                .at(0, 0, 0, 0)
                .to_zoned(TimeZone::UTC)
                .map_err(|_| {
                    wasmtime::Error::msg(
                        "Temporal.Duration.total: internal error computing the total",
                    )
                })?;
            span.total((unit_enum, &sentinel))
        }
    };
    total.map_err(|_| {
        crate::runtime::host::range_error(
            "Temporal.Duration.total: cannot compute the total — the duration may be out of range",
        )
    })
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::runtime::prelude::declaration::{insert_temporal_plain_type, temporal_ref};
    use crate::{
        Dispatch, FileId, MethodSig, Param, PropertySig, Span, Type, TypeKind, TypeSymbol,
        ValueKind, ValueSymbol,
    };
    use std::collections::BTreeMap;
    let temporal_prefix = crate::mangle::prelude("Temporal");
    let temporal_instant_ref = || temporal_ref("Instant");
    let temporal_duration_ref = || temporal_ref("Duration");
    let temporal_zdt_ref = || temporal_ref("ZonedDateTime");
    let temporal_duration_fields_ref = || temporal_ref("DurationFields");
    // The argument type of every duration-consuming arithmetic method
    // (`add`/`subtract` on Instant, Duration, ZonedDateTime). Accepting the
    // `DurationFields` options bag alongside a constructed `Duration` mirrors
    // the Temporal proposal's DurationLike rule; the prelude wrapper normalizes
    // a bag into a `Duration` via `DurationConstructor#new` (see
    // `emit_coerce_duration_arg` in temporal.rs).
    let temporal_duration_like_ref = || {
        Type::union(vec![
            temporal_duration_ref(),
            temporal_duration_fields_ref(),
        ])
    };
    let temporal_to_json_sig = || MethodSig {
        generics: Vec::new(),
        params: Vec::new(),
        ret: Type::String,
        predicate: None,
        doc: doc("/** Returns the ISO string form used by JSON.stringify. */"),
    };
    let temporal_plain_date_ref = || temporal_ref("PlainDate");
    // `relativeTo` anchors calendar-aware Duration operations.
    let temporal_relative_to_ref =
        || Type::union(vec![temporal_plain_date_ref(), temporal_zdt_ref()]);
    let temporal_round_options_ref = || temporal_ref("DurationRoundOptions");
    let temporal_instant_round_options_ref = || temporal_ref("InstantRoundOptions");
    let temporal_zdt_round_options_ref = || temporal_ref("ZonedDateTimeRoundOptions");
    let temporal_since_until_options_ref = || temporal_ref("SinceUntilOptions");
    let temporal_total_options_ref = || temporal_ref("DurationTotalOptions");
    let temporal_compare_options_ref = || temporal_ref("DurationCompareOptions");
    let mut temporal = crate::NamespaceSymbol {
        name: "Temporal".to_string(),
        mangled_prefix: temporal_prefix.clone(),
        declaration_span: Span::at(crate::FileId::PRELUDE),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: doc(
            "/** Date/time primitives — subset of the JS Temporal proposal. Instant (timestamp), Duration (span), ZonedDateTime (instant + IANA tz), plus a non-deterministic `Now` sub-namespace. */",
        ),
    };

    // ---- Temporal.Instant ----
    temporal.types.insert(
        "Instant".to_string(),
        TypeSymbol {
            name: "Temporal.Instant".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "Instant"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "add".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc("/** Returns a new Instant `d` later. Only time-unit components contribute. `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */"),
                        },
                    ),
                    (
                        "subtract".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc("/** Returns a new Instant `d` earlier. `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */"),
                        },
                    ),
                    (
                        "until".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("other", temporal_instant_ref()),
                                Param::with_default(
                                    "options",
                                    temporal_since_until_options_ref(),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns the Duration from `this` to `other` (positive if `other` is later). `options` controls unit balancing and rounding. */"),
                        },
                    ),
                    (
                        "since".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("other", temporal_instant_ref()),
                                Param::with_default(
                                    "options",
                                    temporal_since_until_options_ref(),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns the Duration from `other` to `this` (positive if `this` is later). `options` controls unit balancing and rounding. */"),
                        },
                    ),
                    (
                        "round".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "roundTo",
                                Type::union(vec![
                                    Type::String,
                                    temporal_instant_round_options_ref(),
                                ]),
                            )],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Rounds this Instant to the requested time unit and increment. */",
                            ),
                        },
                    ),
                    (
                        "toZonedDateTimeISO".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("tz", Type::String)],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc("/** Project this Instant onto the IANA time zone `tz`. Throws on unknown zone id. */"),
                        },
                    ),
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns the ISO 8601 representation (e.g. `\"2024-03-09T15:30:45.123Z\"`). */",
                            ),
                        },
                    ),
                    ("toJSON".to_string(), temporal_to_json_sig()),
                    (
                        "equals".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", temporal_instant_ref())],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc("/** `true` if `this` and `other` refer to the same instant. */"),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    (
                        "epochMilliseconds".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Milliseconds since the Unix epoch (1970-01-01T00:00:00Z). */"),
                        },
                    ),
                    (
                        "epochNanoseconds".to_string(),
                        PropertySig {
                            ty: Type::BigInt,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Nanoseconds since the Unix epoch as a `bigint` (full precision). */"),
                        },
                    ),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** A point in time, to nanosecond precision. Independent of any calendar or time zone — see `ZonedDateTime` for wall-clock fields. */",
                ),
            },
        },
    );
    temporal.types.insert(
        "InstantConstructor".to_string(),
        TypeSymbol {
            name: "Temporal.InstantConstructor".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "InstantConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "from".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("iso", Type::String)],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Parse an ISO 8601 instant string (e.g. `\"2024-03-09T15:30:45.123Z\"`). Throws on malformed input. */",
                            ),
                        },
                    ),
                    (
                        "fromEpochMilliseconds".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("ms", Type::Number)],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc("/** Construct an Instant from milliseconds since the Unix epoch. */"),
                        },
                    ),
                    (
                        "fromEpochNanoseconds".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("ns", Type::BigInt)],
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Construct an Instant from nanoseconds since the Unix epoch (full precision). */",
                            ),
                        },
                    ),
                    (
                        "compare".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("a", temporal_instant_ref()),
                                Param::new("b", temporal_instant_ref()),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/** Orders two Instants: -1, 0, or 1. Usable as an Array#sort comparator. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Temporal.Instant`. Accessed via the global `Temporal.Instant` binding. */",
                ),
            },
        },
    );
    temporal.values.insert(
        "Instant".to_string(),
        ValueSymbol {
            name: "Instant".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "Instant"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: temporal_ref("InstantConstructor"),
                doc: doc("/** The `Temporal.Instant` constructor. */"),
            },
        },
    );

    let dur_field_prop = |doc_text: &'static str| PropertySig {
        ty: Type::Number,
        readonly: true,
        intrinsic: false,
        optional: true,
        doc: doc(doc_text),
    };
    let dur_unit_prop = |doc_text: &'static str| PropertySig {
        ty: Type::Number,
        readonly: true,
        intrinsic: false,
        optional: false,
        doc: doc(doc_text),
    };
    temporal.types.insert(
        "DurationFields".to_string(),
        TypeSymbol {
            name: "Temporal.DurationFields".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "DurationFields"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties: BTreeMap::from([
                    ("years".to_string(), dur_field_prop("/** Calendar years. */")),
                    ("months".to_string(), dur_field_prop("/** Calendar months. */")),
                    ("weeks".to_string(), dur_field_prop("/** Calendar weeks (7 days each). */")),
                    ("days".to_string(), dur_field_prop("/** Calendar days. */")),
                    ("hours".to_string(), dur_field_prop("/** Hours. */")),
                    ("minutes".to_string(), dur_field_prop("/** Minutes. */")),
                    ("seconds".to_string(), dur_field_prop("/** Seconds. */")),
                    ("milliseconds".to_string(), dur_field_prop("/** Milliseconds. */")),
                    ("microseconds".to_string(), dur_field_prop("/** Microseconds. */")),
                    ("nanoseconds".to_string(), dur_field_prop("/** Nanoseconds. */")),
                ]),
                dispatch: Dispatch::VTable,
                doc: doc(
                    "/** Options bag for `new Temporal.Duration({...})`. Every field is optional and defaults to 0. */",
                ),
            },
        },
    );
    // Partial all-optional fields bags for `with(...)` on each Plain*/ZonedDateTime
    // type. Field names are singular (year/month/day/hour/minute/second/
    // millisecond/microsecond/nanosecond) per the JS surface.
    {
        let mut insert_fields = |local: &str, names: &[&str]| {
            temporal.types.insert(
                local.to_string(),
                TypeSymbol {
                    name: format!("Temporal.{local}"),
                    mangled_name: crate::mangle::extend(&temporal_prefix, local),
                    declaration_span: Span::at(crate::FileId::PRELUDE),
                    kind: TypeKind::Interface { index: None,
                        generics: Vec::new(),
                        methods: BTreeMap::new(),
                        properties: names
                            .iter()
                            .map(|n| ((*n).to_string(), dur_field_prop("/** Optional field; omitted fields keep the receiver's value. */")))
                            .collect(),
                        dispatch: Dispatch::VTable,
                        doc: doc(
                            "/** Partial fields bag for `with(...)`. Every field is optional. */",
                        ),
                    },
                },
            );
        };
        insert_fields("PlainDateFields", &["year", "month", "day"]);
        insert_fields(
            "PlainTimeFields",
            &[
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ],
        );
        insert_fields(
            "PlainDateTimeFields",
            &[
                "year",
                "month",
                "day",
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ],
        );
        insert_fields("PlainYearMonthFields", &["year", "month"]);
        insert_fields("PlainMonthDayFields", &["month", "day"]);
        insert_fields(
            "ZonedDateTimeFields",
            &[
                "year",
                "month",
                "day",
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ],
        );
        // Required-field bags for the toPlainDate completion methods.
        let mut insert_required = |local: &str, field: &str, field_doc: &'static str| {
            temporal.types.insert(
                local.to_string(),
                TypeSymbol {
                    name: format!("Temporal.{local}"),
                    mangled_name: crate::mangle::extend(&temporal_prefix, local),
                    declaration_span: Span::at(crate::FileId::PRELUDE),
                    kind: TypeKind::Interface {
                        index: None,
                        generics: Vec::new(),
                        methods: BTreeMap::new(),
                        properties: BTreeMap::from([(
                            field.to_string(),
                            PropertySig {
                                ty: Type::Number,
                                readonly: true,
                                intrinsic: false,
                                optional: false,
                                doc: doc(field_doc),
                            },
                        )]),
                        dispatch: Dispatch::VTable,
                        doc: doc("/** The field that completes this value into a `PlainDate`. */"),
                    },
                },
            );
        };
        insert_required(
            "PlainYearMonthToDateFields",
            "day",
            "/** Day of the month (1–31); constrained to the month length. */",
        );
        insert_required(
            "PlainMonthDayToDateFields",
            "year",
            "/** Calendar year; a Feb-29 month-day constrains to Feb-28 in a non-leap year. */",
        );
        // Options object for PlainDate.toZonedDateTime({ timeZone, plainTime? }).
        temporal.types.insert(
            "PlainDateToZonedOptions".to_string(),
            TypeSymbol {
                name: "Temporal.PlainDateToZonedOptions".to_string(),
                mangled_name: crate::mangle::extend(&temporal_prefix, "PlainDateToZonedOptions"),
                declaration_span: Span::at(crate::FileId::PRELUDE),
                kind: TypeKind::Interface {
                    index: None,
                    generics: Vec::new(),
                    methods: BTreeMap::new(),
                    properties: BTreeMap::from([
                        (
                            "timeZone".to_string(),
                            PropertySig {
                                ty: Type::String,
                                readonly: true,
                                intrinsic: false,
                                optional: false,
                                doc: doc("/** IANA time-zone id (e.g. `\"America/New_York\"`). */"),
                            },
                        ),
                        (
                            "plainTime".to_string(),
                            PropertySig {
                                ty: temporal_ref("PlainTime"),
                                readonly: true,
                                intrinsic: false,
                                optional: true,
                                doc: doc(
                                    "/** Wall-clock time; defaults to midnight when omitted. */",
                                ),
                            },
                        ),
                    ]),
                    dispatch: Dispatch::VTable,
                    doc: doc("/** Options for `PlainDate.toZonedDateTime`. */"),
                },
            },
        );
    }
    let opt_string_prop = |doc_text: &'static str| PropertySig {
        ty: Type::String,
        readonly: true,
        intrinsic: false,
        optional: true,
        doc: doc(doc_text),
    };
    let relative_to_prop = |optional: bool, doc_text: &'static str| PropertySig {
        ty: temporal_relative_to_ref(),
        readonly: true,
        intrinsic: false,
        optional,
        doc: doc(doc_text),
    };
    let opt_interface = |name: &str,
                         properties: BTreeMap<String, PropertySig>,
                         doc_text: &'static str| TypeSymbol {
        name: format!("Temporal.{name}"),
        mangled_name: crate::mangle::extend(&temporal_prefix, name),
        declaration_span: Span::at(FileId::PRELUDE),
        kind: TypeKind::Interface {
            index: None,
            generics: Vec::new(),
            methods: BTreeMap::new(),
            properties,
            dispatch: Dispatch::VTable,
            doc: doc(doc_text),
        },
    };
    temporal.types.insert(
        "InstantRoundOptions".to_string(),
        opt_interface(
            "InstantRoundOptions",
            BTreeMap::from([
                (
                    "smallestUnit".to_string(),
                    PropertySig {
                        ty: Type::String,
                        readonly: true,
                        intrinsic: false,
                        optional: false,
                        doc: doc(
                            "/** Smallest time unit to retain, from hour through nanosecond. */",
                        ),
                    },
                ),
                (
                    "roundingMode".to_string(),
                    opt_string_prop("/** Rounding mode; defaults to `\"halfExpand\"`. */"),
                ),
                (
                    "roundingIncrement".to_string(),
                    dur_field_prop("/** Positive increment of `smallestUnit`; defaults to 1. */"),
                ),
            ]),
            "/** Options for `Temporal.Instant.round(...)`. */",
        ),
    );
    temporal.types.insert(
        "DurationRoundOptions".to_string(),
        opt_interface(
            "DurationRoundOptions",
            BTreeMap::from([
                ("smallestUnit".to_string(), opt_string_prop("/** Smallest unit to keep (e.g. `\"second\"`). */")),
                ("largestUnit".to_string(), opt_string_prop("/** Largest unit to balance into (e.g. `\"hour\"`). */")),
                ("roundingMode".to_string(), opt_string_prop("/** `\"halfExpand\"` (default), `\"ceil\"`, `\"floor\"`, `\"trunc\"`, `\"halfEven\"`, … */")),
                ("roundingIncrement".to_string(), dur_field_prop("/** Round to a multiple of this increment (default 1). */")),
                ("relativeTo".to_string(), relative_to_prop(true, "/** Anchor (PlainDate or ZonedDateTime) — required to round calendar units. */")),
            ]),
            "/** Options for `Temporal.Duration.round(...)`. */",
        ),
    );
    temporal.types.insert(
        "ZonedDateTimeRoundOptions".to_string(),
        opt_interface(
            "ZonedDateTimeRoundOptions",
            BTreeMap::from([
                (
                    "smallestUnit".to_string(),
                    PropertySig {
                        ty: Type::String,
                        readonly: true,
                        intrinsic: false,
                        optional: false,
                        doc: doc(
                            "/** Smallest wall-clock unit to retain, from day through nanosecond. */",
                        ),
                    },
                ),
                (
                    "roundingMode".to_string(),
                    opt_string_prop("/** Rounding mode; defaults to `\"halfExpand\"`. */"),
                ),
                (
                    "roundingIncrement".to_string(),
                    dur_field_prop("/** Positive increment of `smallestUnit`; defaults to 1. */"),
                ),
            ]),
            "/** Options for `Temporal.ZonedDateTime.round(...)`. */",
        ),
    );
    temporal.types.insert(
        "SinceUntilOptions".to_string(),
        opt_interface(
            "SinceUntilOptions",
            BTreeMap::from([
                ("largestUnit".to_string(), opt_string_prop("/** Largest unit to balance into (e.g. `\"hours\"`, `\"months\"`). */")),
                ("smallestUnit".to_string(), opt_string_prop("/** Smallest unit to keep; the result is rounded to it. */")),
                ("roundingMode".to_string(), opt_string_prop("/** `\"halfExpand\"` (default), `\"ceil\"`, `\"floor\"`, `\"trunc\"`, `\"halfEven\"`, … */")),
                ("roundingIncrement".to_string(), dur_field_prop("/** Round to a multiple of this increment of `smallestUnit` (default 1). */")),
            ]),
            "/** Options for `until`/`since` on every Temporal type. The receiver anchors calendar units — no `relativeTo` needed. */",
        ),
    );
    temporal.types.insert(
        "DurationTotalOptions".to_string(),
        opt_interface(
            "DurationTotalOptions",
            BTreeMap::from([
                (
                    "unit".to_string(),
                    PropertySig {
                        ty: Type::String,
                        readonly: true,
                        intrinsic: false,
                        optional: false,
                        doc: doc("/** The unit to total into (e.g. `\"hours\"`, `\"days\"`). */"),
                    },
                ),
                (
                    "relativeTo".to_string(),
                    relative_to_prop(
                        true,
                        "/** Anchor (PlainDate or ZonedDateTime) — required for calendar units. */",
                    ),
                ),
            ]),
            "/** Options for `Temporal.Duration.prototype.total(...)`. */",
        ),
    );
    temporal.types.insert(
        "DurationToStringOptions".to_string(),
        opt_interface(
            "DurationToStringOptions",
            BTreeMap::from([
                (
                    "smallestUnit".to_string(),
                    opt_string_prop("/** Smallest unit to keep while formatting. */"),
                ),
                (
                    "roundingMode".to_string(),
                    opt_string_prop(
                        "/** `\"trunc\"` (default), `\"ceil\"`, `\"floor\"`, `\"halfExpand\"`, … */",
                    ),
                ),
                (
                    "fractionalSecondDigits".to_string(),
                    dur_field_prop(
                        "/** Exact fractional second digits to print, from 0 through 9. */",
                    ),
                ),
            ]),
            "/** Options for `Temporal.Duration.prototype.toString(...)`. */",
        ),
    );
    temporal.types.insert(
        "DurationCompareOptions".to_string(),
        opt_interface(
            "DurationCompareOptions",
            BTreeMap::from([(
                "relativeTo".to_string(),
                relative_to_prop(true, "/** Anchor (PlainDate or ZonedDateTime) — required to compare calendar durations. */"),
            )]),
            "/** Options for `Temporal.Duration.compare(...)`. */",
        ),
    );
    temporal.types.insert(
        "Duration".to_string(),
        TypeSymbol {
            name: "Temporal.Duration".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "Duration"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "add".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns `this + d` as a new Duration. `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */"),
                        },
                    ),
                    (
                        "subtract".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns `this - d` as a new Duration. `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */"),
                        },
                    ),
                    (
                        "negated".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns a Duration with every field's sign flipped. */"),
                        },
                    ),
                    (
                        "total".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "totalOf",
                                Type::union(vec![Type::String, temporal_total_options_ref()]),
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/** Total length of this Duration in a unit — `total(\"hours\")` or `total({ unit, relativeTo })`. Calendar units (`\"weeks\"`/`\"months\"`/`\"years\"`) require a `relativeTo` anchor (a PlainDate or ZonedDateTime). */",
                            ),
                        },
                    ),
                    (
                        "round".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "roundTo",
                                Type::union(vec![
                                    Type::String,
                                    temporal_round_options_ref(),
                                ]),
                            )],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Rounds this Duration — `round(\"hour\")` (smallestUnit shorthand) or `round({ smallestUnit, largestUnit, roundingMode, roundingIncrement, relativeTo })`. Calendar units need a `relativeTo` anchor. */",
                            ),
                        },
                    ),
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "options",
                                temporal_ref("DurationToStringOptions"),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns the ISO 8601 duration form (e.g. `\"PT1H30M\"`), optionally rounded/formatted. */"),
                        },
                    ),
                    ("toJSON".to_string(), temporal_to_json_sig()),
                    (
                        "with".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("fields", temporal_duration_like_ref())],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Returns a copy of this Duration with the given slots replaced (e.g. `d.with({ hours: 5 })`); unlisted slots are kept. */",
                            ),
                        },
                    ),
                    (
                        "abs".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Returns a Duration with every slot's absolute value (a non-negative Duration). */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    ("years".to_string(), dur_unit_prop("/** Calendar-years slot. */")),
                    ("months".to_string(), dur_unit_prop("/** Calendar-months slot. */")),
                    ("weeks".to_string(), dur_unit_prop("/** Calendar-weeks slot. */")),
                    ("days".to_string(), dur_unit_prop("/** Calendar-days slot. */")),
                    ("hours".to_string(), dur_unit_prop("/** Hours slot. */")),
                    ("minutes".to_string(), dur_unit_prop("/** Minutes slot. */")),
                    ("seconds".to_string(), dur_unit_prop("/** Seconds slot. */")),
                    ("milliseconds".to_string(), dur_unit_prop("/** Milliseconds slot. */")),
                    ("microseconds".to_string(), dur_unit_prop("/** Microseconds slot. */")),
                    ("nanoseconds".to_string(), dur_unit_prop("/** Nanoseconds slot. */")),
                    (
                        "sign".to_string(),
                        dur_unit_prop(
                            "/** `-1`, `0`, or `1`: the Duration's overall direction (every slot shares this sign). */",
                        ),
                    ),
                    (
                        "blank".to_string(),
                        PropertySig {
                            ty: Type::Boolean,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(
                                "/** `true` when every slot is `0` (the Duration is empty / `sign === 0`). */",
                            ),
                        },
                    ),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** A span of time, expressed as separate calendar (`years`/`months`/`weeks`/`days`) and time (`hours`/…/`nanoseconds`) unit slots. Field reads return the literal slot value — use `total(unit)` for the computed total. */",
                ),
            },
        },
    );
    temporal.types.insert(
        "DurationConstructor".to_string(),
        TypeSymbol {
            name: "Temporal.DurationConstructor".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "DurationConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "new".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("fields", temporal_duration_fields_ref())],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Construct a Duration from a fields bag (e.g. `{ hours: 1, minutes: 30 }`). Every field defaults to 0. */",
                            ),
                        },
                    ),
                    (
                        "from".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "item",
                                Type::union(vec![
                                    Type::String,
                                    temporal_duration_ref(),
                                    temporal_duration_fields_ref(),
                                ]),
                            )],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Build a Duration from an ISO 8601 string (e.g. `\"PT1H30M\"`), an existing Duration, or a fields bag (`{ hours: 1 }`). */",
                            ),
                        },
                    ),
                    (
                        "compare".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("a", temporal_duration_like_ref()),
                                Param::new("b", temporal_duration_like_ref()),
                                Param::with_default(
                                    "options",
                                    temporal_compare_options_ref(),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/** Compares two Durations: `-1`, `0`, or `1`. Calendar durations need `options.relativeTo` (a PlainDate or ZonedDateTime). */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Temporal.Duration`. Accessed via the global `Temporal.Duration` binding. */",
                ),
            },
        },
    );
    temporal.values.insert(
        "Duration".to_string(),
        ValueSymbol {
            name: "Duration".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "Duration"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: temporal_ref("DurationConstructor"),
                doc: doc("/** The `Temporal.Duration` constructor. */"),
            },
        },
    );

    let zdt_field_prop = |doc_text: &'static str| PropertySig {
        ty: Type::Number,
        readonly: true,
        intrinsic: false,
        optional: false,
        doc: doc(doc_text),
    };
    temporal.types.insert(
        "ZonedDateTime".to_string(),
        TypeSymbol {
            name: "Temporal.ZonedDateTime".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "ZonedDateTime"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "add".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Returns a new ZonedDateTime `d` later. Calendar-aware (DST transitions respected). `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */",
                            ),
                        },
                    ),
                    (
                        "subtract".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("d", temporal_duration_like_ref())],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc("/** Returns a new ZonedDateTime `d` earlier. `d` may be a `Temporal.Duration` or a DurationLike bag (`{ hours: 1, minutes: 30 }`). */"),
                        },
                    ),
                    (
                        "until".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("other", temporal_zdt_ref()),
                                Param::with_default(
                                    "options",
                                    temporal_since_until_options_ref(),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns the calendar-aware Duration from `this` to `other`. `options` controls unit balancing and rounding. */"),
                        },
                    ),
                    (
                        "since".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("other", temporal_zdt_ref()),
                                Param::with_default(
                                    "options",
                                    temporal_since_until_options_ref(),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: temporal_duration_ref(),
                            predicate: None,
                            doc: doc("/** Returns the calendar-aware Duration from `other` to `this`. `options` controls unit balancing and rounding. */"),
                        },
                    ),
                    (
                        "withTimeZone".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("tz", Type::String)],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Same instant, different IANA time zone. Wall-clock fields shift accordingly. Throws on unknown zone id. */",
                            ),
                        },
                    ),
                    (
                        "with".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "fields",
                                temporal_ref("ZonedDateTimeFields"),
                            )],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Returns a copy with the listed wall-clock fields replaced (keeping the time zone); unlisted fields are kept. Out-of-range values are constrained. */",
                            ),
                        },
                    ),
                    (
                        "round".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "roundTo",
                                Type::union(vec![
                                    Type::String,
                                    temporal_zdt_round_options_ref(),
                                ]),
                            )],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Rounds this ZonedDateTime in its time zone, from day through nanosecond precision. */",
                            ),
                        },
                    ),
                    (
                        "startOfDay".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Returns the first valid instant on this date in the current time zone. */",
                            ),
                        },
                    ),
                    (
                        "toInstant".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_instant_ref(),
                            predicate: None,
                            doc: doc("/** Drops the time-zone, returns the underlying Instant. */"),
                        },
                    ),
                    (
                        "toPlainDate".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_plain_date_ref(),
                            predicate: None,
                            doc: doc(
                                "/** The wall-clock calendar date in this time zone, as a tz-free `PlainDate`. */",
                            ),
                        },
                    ),
                    (
                        "toPlainTime".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_ref("PlainTime"),
                            predicate: None,
                            doc: doc(
                                "/** The wall-clock time in this time zone, as a tz-free `PlainTime` (with nanosecond precision). */",
                            ),
                        },
                    ),
                    (
                        "toPlainDateTime".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: temporal_ref("PlainDateTime"),
                            predicate: None,
                            doc: doc(
                                "/** The wall-clock date and time in this time zone, as a tz-free `PlainDateTime`. */",
                            ),
                        },
                    ),
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns the ISO 8601 form with bracketed tz (e.g. `\"2024-03-09T12:00:00+00:00[UTC]\"`). */",
                            ),
                        },
                    ),
                    ("toJSON".to_string(), temporal_to_json_sig()),
                    (
                        "equals".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", temporal_zdt_ref())],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** `true` if `this` and `other` have the same instant **and** the same time-zone id. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    (
                        "timeZoneId".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The IANA time-zone id (e.g. `\"America/New_York\"`). */"),
                        },
                    ),
                    (
                        "epochMilliseconds".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Milliseconds since the Unix epoch (timezone-independent). */"),
                        },
                    ),
                    (
                        "epochNanoseconds".to_string(),
                        PropertySig {
                            ty: Type::BigInt,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Nanoseconds since the Unix epoch. */"),
                        },
                    ),
                    (
                        "offset".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** UTC offset at this instant, such as `\"-05:00\"`. */"),
                        },
                    ),
                    (
                        "offsetNanoseconds".to_string(),
                        zdt_field_prop("/** UTC offset at this instant in nanoseconds. */"),
                    ),
                    (
                        "hoursInDay".to_string(),
                        zdt_field_prop(
                            "/** Elapsed hours in this local day, accounting for time-zone transitions. */",
                        ),
                    ),
                    ("year".to_string(), zdt_field_prop("/** Calendar year in this time zone. */")),
                    ("month".to_string(), zdt_field_prop("/** Calendar month (1–12). */")),
                    ("day".to_string(), zdt_field_prop("/** Day of the month (1–31). */")),
                    ("hour".to_string(), zdt_field_prop("/** Hour of the day (0–23). */")),
                    ("minute".to_string(), zdt_field_prop("/** Minute of the hour (0–59). */")),
                    ("second".to_string(), zdt_field_prop("/** Second of the minute (0–59 or 60 on a leap-second). */")),
                    (
                        "dayOfWeek".to_string(),
                        zdt_field_prop("/** Day of the week, 1 (Monday) through 7 (Sunday). */"),
                    ),
                    (
                        "monthCode".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** ISO month code, e.g. `\"M06\"`. */"),
                        },
                    ),
                    ("dayOfYear".to_string(), zdt_field_prop("/** Day of the year (1–365 or 366). */")),
                    ("weekOfYear".to_string(), zdt_field_prop("/** ISO week number. */")),
                    ("yearOfWeek".to_string(), zdt_field_prop("/** ISO week-numbering year. */")),
                    ("daysInWeek".to_string(), zdt_field_prop("/** Days in this ISO week. */")),
                    ("daysInMonth".to_string(), zdt_field_prop("/** Days in this month. */")),
                    ("daysInYear".to_string(), zdt_field_prop("/** Days in this year. */")),
                    ("monthsInYear".to_string(), zdt_field_prop("/** Months in this year. */")),
                    (
                        "inLeapYear".to_string(),
                        PropertySig {
                            ty: Type::Boolean,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** True when the ISO calendar year is a leap year. */"),
                        },
                    ),
                    ("millisecond".to_string(), zdt_field_prop("/** Millisecond within the second (0–999). */")),
                    ("microsecond".to_string(), zdt_field_prop("/** Microsecond within the millisecond (0–999). */")),
                    ("nanosecond".to_string(), zdt_field_prop("/** Nanosecond within the microsecond (0–999). */")),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** An Instant paired with an IANA time zone. Wall-clock fields (`year`/`month`/…/`dayOfWeek`) are computed in the tz. Arithmetic is DST-aware. */",
                ),
            },
        },
    );
    temporal.types.insert(
        "ZonedDateTimeConstructor".to_string(),
        TypeSymbol {
            name: "Temporal.ZonedDateTimeConstructor".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "ZonedDateTimeConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "from".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("iso", Type::String)],
                            ret: temporal_zdt_ref(),
                            predicate: None,
                            doc: doc(
                                "/** Parse an ISO 8601 zoned datetime with bracketed tz (e.g. `\"2026-01-15T10:00:00-05:00[America/New_York]\"`). Throws on malformed input or unknown zone id. */",
                            ),
                        },
                    ),
                    (
                        "compare".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("a", temporal_zdt_ref()),
                                Param::new("b", temporal_zdt_ref()),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/** Orders two ZonedDateTimes by their exact instant: -1, 0, or 1. Usable as an Array#sort comparator. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc("/** Constructor object for `Temporal.ZonedDateTime`. */"),
            },
        },
    );
    temporal.values.insert(
        "ZonedDateTime".to_string(),
        ValueSymbol {
            name: "ZonedDateTime".to_string(),
            mangled_name: crate::mangle::extend(&temporal_prefix, "ZonedDateTime"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: temporal_ref("ZonedDateTimeConstructor"),
                doc: doc("/** The `Temporal.ZonedDateTime` constructor. */"),
            },
        },
    );

    // ---- Plain* family (calendar/clock-only, ISO-8601, no time zone) ----
    let temporal_plain_date_ref = || temporal_ref("PlainDate");
    let temporal_plain_time_ref = || temporal_ref("PlainTime");
    // The four arithmetic instance methods shared by every Plain* type that
    // supports them: `add`/`subtract` take a `Duration | DurationLike` and return
    // the same type; `until`/`since` take another value and return a `Duration`.
    let arith_methods = |self_ty: Type| -> Vec<(String, MethodSig)> {
        vec![
            (
                "add".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new("duration", temporal_duration_like_ref())],
                    ret: self_ty.clone(),
                    predicate: None,
                    doc: doc("/** Returns a new value with `duration` added. */"),
                },
            ),
            (
                "subtract".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new("duration", temporal_duration_like_ref())],
                    ret: self_ty.clone(),
                    predicate: None,
                    doc: doc("/** Returns a new value with `duration` subtracted. */"),
                },
            ),
            (
                "until".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![
                        Param::new("other", self_ty.clone()),
                        Param::with_default(
                            "options",
                            temporal_since_until_options_ref(),
                            crate::DefaultValue::Null,
                        ),
                    ],
                    ret: temporal_duration_ref(),
                    predicate: None,
                    doc: doc(
                        "/** The `Duration` from `this` until `other`. `options` controls unit balancing and rounding. */",
                    ),
                },
            ),
            (
                "since".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![
                        Param::new("other", self_ty.clone()),
                        Param::with_default(
                            "options",
                            temporal_since_until_options_ref(),
                            crate::DefaultValue::Null,
                        ),
                    ],
                    ret: temporal_duration_ref(),
                    predicate: None,
                    doc: doc(
                        "/** The `Duration` from `other` since which `this` occurs. `options` controls unit balancing and rounding. */",
                    ),
                },
            ),
        ]
    };
    let temporal_plain_date_time_ref = || temporal_ref("PlainDateTime");
    let temporal_plain_year_month_ref = || temporal_ref("PlainYearMonth");
    // `with(fields): Self` — `fields_name` is the matching `…Fields` bag type.
    let with_method = |self_ty: Type, fields_name: &'static str| -> (String, MethodSig) {
        (
            "with".to_string(),
            MethodSig {
                generics: Vec::new(),
                params: vec![Param::new("fields", temporal_ref(fields_name))],
                ret: self_ty,
                predicate: None,
                doc: doc(
                    "/** Returns a copy with the listed fields replaced; unlisted fields are kept. Out-of-range values are constrained. */",
                ),
            },
        )
    };
    let temporal_plain_month_day_ref = || temporal_ref("PlainMonthDay");
    // Projections onto the coarser carriers, shared by PlainDate and PlainDateTime.
    let to_year_month_method = || {
        (
            "toPlainYearMonth".to_string(),
            MethodSig {
                generics: Vec::new(),
                params: Vec::new(),
                ret: temporal_plain_year_month_ref(),
                predicate: None,
                doc: doc("/** The year-and-month part as a `PlainYearMonth`. */"),
            },
        )
    };
    let to_month_day_method = || {
        (
            "toPlainMonthDay".to_string(),
            MethodSig {
                generics: Vec::new(),
                params: Vec::new(),
                ret: temporal_plain_month_day_ref(),
                predicate: None,
                doc: doc("/** The month-and-day part as a `PlainMonthDay`. */"),
            },
        )
    };
    insert_temporal_plain_type(
        &mut temporal,
        &temporal_prefix,
        "PlainDate",
        "/** A calendar date (year, month, day) with no time or time zone. ISO-8601 proleptic Gregorian. */",
        &[
            ("year", "/** Calendar year. */"),
            ("month", "/** Calendar month (1–12). */"),
            ("day", "/** Day of the month (1–31). */"),
            (
                "dayOfWeek",
                "/** Day of the week, 1 (Monday) through 7 (Sunday). */",
            ),
            ("monthCode", "/** ISO month code, e.g. `\"M06\"`. */"),
            ("dayOfYear", "/** Day of the year (1–365 or 366). */"),
            ("weekOfYear", "/** ISO week number. */"),
            ("yearOfWeek", "/** ISO week-numbering year. */"),
            ("daysInWeek", "/** Days in this ISO week. */"),
            ("daysInMonth", "/** Days in this month. */"),
            ("daysInYear", "/** Days in this year. */"),
            ("monthsInYear", "/** Months in this year. */"),
            (
                "inLeapYear",
                "/** True when the ISO calendar year is a leap year. */",
            ),
        ],
        {
            let mut m = arith_methods(temporal_plain_date_ref());
            m.push(with_method(temporal_plain_date_ref(), "PlainDateFields"));
            m.push(to_year_month_method());
            m.push(to_month_day_method());
            m.push((
                "toPlainDateTime".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::with_default(
                        "time",
                        temporal_plain_time_ref(),
                        crate::DefaultValue::Null,
                    )],
                    ret: temporal_plain_date_time_ref(),
                    predicate: None,
                    doc: doc(
                        "/** Combines this date with `time` (default midnight) into a `PlainDateTime`. */",
                    ),
                },
            ));
            m.push((
                "toZonedDateTime".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new(
                        "timeZoneOrOptions",
                        Type::Union(vec![
                            Type::String,
                            temporal_ref("PlainDateToZonedOptions"),
                        ]),
                    )],
                    ret: temporal_zdt_ref(),
                    predicate: None,
                    doc: doc(
                        "/** Interprets this date in a time zone — pass a zone id (midnight) or `{ timeZone, plainTime? }`. Resolves DST gaps with `compatible` disambiguation. */",
                    ),
                },
            ));
            m
        },
        true,
    );
    insert_temporal_plain_type(
        &mut temporal,
        &temporal_prefix,
        "PlainTime",
        "/** A wall-clock time (hour, minute, second, nanosecond) with no date or time zone. */",
        &[
            ("hour", "/** Hour of the day (0–23). */"),
            ("minute", "/** Minute of the hour (0–59). */"),
            ("second", "/** Second of the minute (0–59). */"),
            (
                "millisecond",
                "/** Millisecond within the second (0–999). */",
            ),
            (
                "microsecond",
                "/** Microsecond within the millisecond (0–999). */",
            ),
            (
                "nanosecond",
                "/** Nanoseconds within the second (0–999_999_999). */",
            ),
        ],
        {
            let mut m = arith_methods(temporal_plain_time_ref());
            m.push(with_method(temporal_plain_time_ref(), "PlainTimeFields"));
            m
        },
        true,
    );
    insert_temporal_plain_type(
        &mut temporal,
        &temporal_prefix,
        "PlainDateTime",
        "/** A calendar date and wall-clock time with no time zone. */",
        &[
            ("year", "/** Calendar year. */"),
            ("month", "/** Calendar month (1–12). */"),
            ("day", "/** Day of the month (1–31). */"),
            ("hour", "/** Hour of the day (0–23). */"),
            ("minute", "/** Minute of the hour (0–59). */"),
            ("second", "/** Second of the minute (0–59). */"),
            (
                "dayOfWeek",
                "/** Day of the week, 1 (Monday) through 7 (Sunday). */",
            ),
            ("monthCode", "/** ISO month code, e.g. `\"M06\"`. */"),
            ("dayOfYear", "/** Day of the year (1–365 or 366). */"),
            ("weekOfYear", "/** ISO week number. */"),
            ("yearOfWeek", "/** ISO week-numbering year. */"),
            ("daysInWeek", "/** Days in this ISO week. */"),
            ("daysInMonth", "/** Days in this month. */"),
            ("daysInYear", "/** Days in this year. */"),
            ("monthsInYear", "/** Months in this year. */"),
            (
                "inLeapYear",
                "/** True when the ISO calendar year is a leap year. */",
            ),
            (
                "millisecond",
                "/** Millisecond within the second (0–999). */",
            ),
            (
                "microsecond",
                "/** Microsecond within the millisecond (0–999). */",
            ),
            (
                "nanosecond",
                "/** Nanoseconds within the second (0–999_999_999). */",
            ),
        ],
        {
            let mut m = arith_methods(temporal_plain_date_time_ref());
            m.push((
                "toPlainDate".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: Vec::new(),
                    ret: temporal_plain_date_ref(),
                    predicate: None,
                    doc: doc("/** The calendar-date part as a `PlainDate`. */"),
                },
            ));
            m.push((
                "toPlainTime".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: Vec::new(),
                    ret: temporal_plain_time_ref(),
                    predicate: None,
                    doc: doc("/** The wall-clock-time part as a `PlainTime`. */"),
                },
            ));
            m.push(to_year_month_method());
            m.push(to_month_day_method());
            m.push((
                "toZonedDateTime".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new("timeZone", Type::String)],
                    ret: temporal_zdt_ref(),
                    predicate: None,
                    doc: doc(
                        "/** Interprets this wall clock in `timeZone`, resolving DST gaps with `compatible` disambiguation. */",
                    ),
                },
            ));
            m.push(with_method(
                temporal_plain_date_time_ref(),
                "PlainDateTimeFields",
            ));
            m
        },
        true,
    );
    insert_temporal_plain_type(
        &mut temporal,
        &temporal_prefix,
        "PlainYearMonth",
        "/** A calendar year and month with no day or time zone (e.g. `2026-06`). */",
        &[
            ("year", "/** Calendar year. */"),
            ("month", "/** Calendar month (1–12). */"),
            ("monthCode", "/** ISO month code, e.g. `\"M06\"`. */"),
            ("daysInMonth", "/** Days in this month. */"),
            ("daysInYear", "/** Days in this year. */"),
            ("monthsInYear", "/** Months in this year. */"),
            (
                "inLeapYear",
                "/** True when the ISO calendar year is a leap year. */",
            ),
        ],
        {
            let mut m = arith_methods(temporal_plain_year_month_ref());
            m.push(with_method(
                temporal_plain_year_month_ref(),
                "PlainYearMonthFields",
            ));
            m.push((
                "toPlainDate".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new(
                        "fields",
                        temporal_ref("PlainYearMonthToDateFields"),
                    )],
                    ret: temporal_plain_date_ref(),
                    predicate: None,
                    doc: doc(
                        "/** Adds a `day` to make a `PlainDate`; the day constrains to the month length. */",
                    ),
                },
            ));
            m
        },
        true,
    );
    insert_temporal_plain_type(
        &mut temporal,
        &temporal_prefix,
        "PlainMonthDay",
        "/** A calendar month and day with no year or time zone (e.g. `06-07`). */",
        &[
            ("month", "/** Calendar month (1–12). */"),
            ("monthCode", "/** ISO month code, e.g. `\"M06\"`. */"),
            ("day", "/** Day of the month (1–31). */"),
        ],
        vec![
            with_method(temporal_ref("PlainMonthDay"), "PlainMonthDayFields"),
            (
                "toPlainDate".to_string(),
                MethodSig {
                    generics: Vec::new(),
                    params: vec![Param::new(
                        "fields",
                        temporal_ref("PlainMonthDayToDateFields"),
                    )],
                    ret: temporal_plain_date_ref(),
                    predicate: None,
                    doc: doc(
                        "/** Adds a `year` to make a `PlainDate`; a Feb-29 month-day constrains to Feb-28 in a non-leap year. */",
                    ),
                },
            ),
        ],
        false,
    );

    let now_prefix = crate::mangle::extend(&temporal_prefix, "Now");
    let mut now = crate::NamespaceSymbol {
        name: "Now".to_string(),
        mangled_prefix: now_prefix.clone(),
        declaration_span: Span::at(crate::FileId::PRELUDE),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: doc(
            "/** Non-deterministic time / time-zone queries. Calls land in the durable-execution log so replays can reproduce the original wall-clock values. */",
        ),
    };
    now.values.insert(
        "instant".to_string(),
        ValueSymbol {
            name: "instant".to_string(),
            mangled_name: crate::mangle::extend(&now_prefix, "instant"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: Vec::new(),
                ret: temporal_instant_ref(),
                type_predicate: None,
                doc: doc(
                    "/** The current wall-clock instant. Non-deterministic — reads the system clock. */",
                ),
            },
        },
    );
    now.values.insert(
        "timeZoneId".to_string(),
        ValueSymbol {
            name: "timeZoneId".to_string(),
            mangled_name: crate::mangle::extend(&now_prefix, "timeZoneId"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: Vec::new(),
                ret: Type::String,
                type_predicate: None,
                doc: doc(
                    "/** The system IANA time-zone id (or `\"UTC\"` if the system zone can't be detected). Non-deterministic. */",
                ),
            },
        },
    );
    now.values.insert(
        "zonedDateTimeISO".to_string(),
        ValueSymbol {
            name: "zonedDateTimeISO".to_string(),
            mangled_name: crate::mangle::extend(&now_prefix, "zonedDateTimeISO"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::with_default(
                    "tz",
                    Type::Union(vec![Type::String, Type::Null]),
                    crate::DefaultValue::Null,
                )],
                ret: temporal_zdt_ref(),
                type_predicate: None,
                doc: doc(
                    "/** The current ZonedDateTime in `tz`, or the system zone if `tz` is omitted / null. Non-deterministic. */",
                ),
            },
        },
    );
    let mut insert_now_plain = |name: &str, ret: Type, doc_text: &'static str| {
        now.values.insert(
            name.to_string(),
            ValueSymbol {
                name: name.to_string(),
                mangled_name: crate::mangle::extend(&now_prefix, name),
                declaration_span: Span::at(FileId::PRELUDE),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::with_default(
                        "tz",
                        Type::Union(vec![Type::String, Type::Null]),
                        crate::DefaultValue::Null,
                    )],
                    ret,
                    type_predicate: None,
                    doc: doc(doc_text),
                },
            },
        );
    };
    insert_now_plain(
        "plainDateISO",
        temporal_plain_date_ref(),
        "/** Today's date in `tz`, or the system (local) zone if `tz` is omitted / null. Non-deterministic. */",
    );
    insert_now_plain(
        "plainTimeISO",
        temporal_plain_time_ref(),
        "/** The current wall-clock time in `tz`, or the system (local) zone if `tz` is omitted / null. Non-deterministic. */",
    );
    insert_now_plain(
        "plainDateTimeISO",
        temporal_ref("PlainDateTime"),
        "/** The current date and wall-clock time in `tz`, or the system (local) zone if `tz` is omitted / null. Non-deterministic. */",
    );
    now.values.insert(
        "zonedDateTime".to_string(),
        ValueSymbol {
            name: "zonedDateTime".to_string(),
            mangled_name: crate::mangle::extend(&now_prefix, "zonedDateTime"),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::with_default(
                    "tz",
                    Type::Union(vec![Type::String, Type::Null]),
                    crate::DefaultValue::Null,
                )],
                ret: temporal_zdt_ref(),
                type_predicate: None,
                doc: doc(
                    "/** The current ZonedDateTime in `tz`, or the system zone if `tz` is null. Non-deterministic. Alias of `zonedDateTimeISO` (our subset has no calendar argument). */",
                ),
            },
        },
    );
    temporal.namespaces.insert("Now".to_string(), now);

    for (local_name, ty_sym) in &temporal.types {
        let flat_key = format!("Temporal#{local_name}");
        defs.types.insert(flat_key, ty_sym.clone());
    }

    defs.namespaces.insert("Temporal".to_string(), temporal);
}

#[cfg(test)]
mod invariant_tests {
    use super::*;
    #[test]
    fn fractional_digits_are_checked_even_for_date_only_durations() {
        for digits in [10, u8::MAX] {
            assert!(
                rounding_for_fractional_second_digits(digits)
                    .unwrap_err()
                    .is::<wasmtime::Trap>()
            );
            assert!(
                format_duration_with_fractional_digits(&Span::new().days(1), digits)
                    .unwrap_err()
                    .is::<wasmtime::Trap>()
            );
        }
        assert_eq!(
            format_duration_with_fractional_digits(&Span::new().seconds(1).milliseconds(25), 3)
                .unwrap(),
            "PT1.025S"
        );
    }
}
