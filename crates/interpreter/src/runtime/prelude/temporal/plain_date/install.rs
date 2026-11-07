use crate::runtime::host::{abi_arg, abi_result};
use jiff::civil;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::register_host_fn;
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::collection::object_field;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let obj = &types.object;
    shared::reg_plain_from(
        linker,
        engine,
        obj,
        "PlainDateConstructor",
        super::super::like::plain_date,
    )?;
    for method in ["toString", "toJSON"] {
        shared::reg_plain_string(
            linker,
            engine,
            obj,
            &types.string,
            "PlainDate",
            method,
            shared::plain_to_string_date,
        )?;
    }
    shared::reg_plain_equals(
        linker,
        engine,
        obj,
        "PlainDate",
        shared::plain_equals_date,
        super::super::like::plain_date,
    )?;
    shared::reg_plain_compare(
        linker,
        engine,
        obj,
        "PlainDateConstructor",
        &[1, 2, 3],
        super::super::like::plain_date,
    )?;
    for (name, field) in [("year", 1), ("month", 2), ("day", 3)] {
        shared::reg_plain_i32_getter(linker, engine, obj, "PlainDate", name, field)?;
    }
    shared::reg_plain_date_derived_getters(linker, engine, obj, "PlainDate", 0)?;

    let obj_pair = FuncType::new(engine, [obj.clone(), obj.clone()], [obj.clone()]);
    for method in ["add", "subtract"] {
        reg_plain_date_duration_op(linker, types.intr.clone(), method, obj_pair.clone())?;
    }
    let with_ty = FuncType::new(
        engine,
        [obj.clone(), types.object_shape.clone()],
        [obj.clone()],
    );
    reg_plain_date_with(linker, engine, with_ty)?;
    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(types.intr.object.clone()),
    ));
    let pair_ty = FuncType::new(engine, [obj.clone(), obj.clone(), options], [obj.clone()]);
    for method in ["until", "since"] {
        reg_plain_date_pair_op(linker, method, pair_ty.clone())?;
    }

    let receiver_ty = FuncType::new(engine, [obj.clone()], [obj.clone()]);
    shared::reg_plain_date_to_year_month(linker, "PlainDate", receiver_ty.clone())?;
    shared::reg_plain_date_to_month_day(linker, "PlainDate", receiver_ty)?;
    reg_plain_date_to_plain_date_time(linker, obj_pair.clone())?;
    reg_plain_date_to_zoned(linker, obj_pair)
}

fn reg_plain_date_duration_op(
    linker: &mut Linker<StoreData>,
    intr: IntrinsicTypes,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainDate.{method}");
    let add = method == "add";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDate", method),
        ty,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let date = plain_date_from_struct(caller, st, &label)?;
            let span = read_duration_like(caller, abi_arg(params, 1)?, &intr, &label)?;
            let out = if add {
                super::add(date, span)
            } else {
                super::subtract(date, span)
            }
            .map_err(crate::runtime::host::range_error)?;
            *abi_result(results, 0)? = make_plain_date(
                caller,
                i32::from(out.year()),
                i32::from(out.month()),
                i32::from(out.day()),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_date_with(
    linker: &mut Linker<StoreData>,
    _engine: &wasmtime::Engine,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDate", "with"),
        ty,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainDate.with")?;
            let (y, m, d) = merge_date(
                (
                    st_i32(caller, st, 1, "PlainDate.with")?,
                    st_i32(caller, st, 2, "PlainDate.with")?,
                    st_i32(caller, st, 3, "PlainDate.with")?,
                ),
                object_i64_field(caller, abi_arg(params, 1)?, "year", "PlainDate.with")?,
                object_i64_field(caller, abi_arg(params, 1)?, "month", "PlainDate.with")?,
                object_i64_field(caller, abi_arg(params, 1)?, "day", "PlainDate.with")?,
                "PlainDate.with",
            )?;
            let date = super::super::shared::y_m_d_date(y, m, d, "PlainDate.with")?;
            *abi_result(results, 0)? = make_plain_date(
                caller,
                i32::from(date.year()),
                i32::from(date.month()),
                i32::from(date.day()),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_date_pair_op(
    linker: &mut Linker<StoreData>,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainDate.{method}");
    let until = method == "until";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDate", method),
        ty,
        move |caller, params, results| {
            let a_st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let a = plain_date_from_struct(caller, a_st, &label)?;
            let b = super::super::like::date(caller, abi_arg(params, 1)?, None)?;
            let o = object_diff_options(
                caller,
                abi_arg(params, 2)?,
                if until {
                    "PlainDate.until"
                } else {
                    "PlainDate.since"
                },
            )?;
            let span = if until {
                a.until(o.date(b))
                    .map_err(temporal_err("PlainDate.until"))?
            } else {
                a.since(o.date(b))
                    .map_err(temporal_err("PlainDate.since"))?
            };
            *abi_result(results, 0)? = make_duration(caller, &span)?;
            Ok(())
        },
    )
}

fn reg_plain_date_to_plain_date_time(
    linker: &mut Linker<StoreData>,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDate", "toPlainDateTime"),
        ty,
        |caller, params, results| {
            let d = as_struct_val(caller, abi_arg(params, 0)?, "PlainDate.toPlainDateTime")?;
            let maybe_t = *abi_arg(params, 1)?;
            let time = if crate::runtime::prelude::undefined::is_undefined(caller, &maybe_t)? {
                civil::Time::midnight()
            } else {
                super::super::like::time(caller, &maybe_t, None)?
            };
            let y = st_i32(caller, d, 1, "PlainDate.toPlainDateTime")?;
            let m = st_i32(caller, d, 2, "PlainDate.toPlainDateTime")?;
            let day = st_i32(caller, d, 3, "PlainDate.toPlainDateTime")?;
            *abi_result(results, 0)? = make_plain_date_time(
                caller,
                y,
                m,
                day,
                i32::from(time.hour()),
                i32::from(time.minute()),
                i32::from(time.second()),
                time.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_date_to_zoned(linker: &mut Linker<StoreData>, ty: FuncType) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDate", "toZonedDateTime"),
        ty,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainDate.toZonedDateTime")?;
            let date = plain_date_from_struct(caller, st, "PlainDate.toZonedDateTime")?;
            let (tz_id, time) = plain_date_to_zoned_arg(caller, abi_arg(params, 1)?)?;
            let (tz, canonical_id) = super::super::zoned_date_time::resolve_time_zone(
                caller,
                &tz_id,
                "PlainDate.toZonedDateTime",
            )?;
            let z = date
                .to_datetime(time)
                .to_zoned(tz)
                .map_err(temporal_err("toZonedDateTime"))?;
            *abi_result(results, 0)? = make_zoned_date_time(caller, &z, &canonical_id)?;
            Ok(())
        },
    )
}

fn plain_date_to_zoned_arg(
    caller: &mut Caller<'_, StoreData>,
    arg: &Val,
) -> wasmtime::Result<(String, civil::Time)> {
    let zoned_ty = temporal_abi(caller)?.zoned_date_time.clone();
    let string_ty = caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| crate::runtime::host::fatal_host_error("Temporal: host ABI unset"))?
        .string_type
        .clone();
    if crate::runtime::prelude::collection::is_a(caller, arg, &zoned_ty)?
        || crate::runtime::prelude::collection::is_a(caller, arg, &string_ty)?
    {
        return Ok((
            super::super::like::time_zone(caller, arg, "PlainDate.toZonedDateTime")?,
            civil::Time::midnight(),
        ));
    }
    let zone = object_field(caller, arg, "timeZone")?.ok_or_else(|| {
        crate::runtime::host::type_error("Temporal.PlainDate.toZonedDateTime: timeZone is required")
    })?;
    let tz_id = super::super::like::time_zone(caller, &zone, "PlainDate.toZonedDateTime")?;
    let time = match object_field(caller, arg, "plainTime")? {
        Some(t) if !crate::runtime::prelude::undefined::is_undefined(caller, &t)? => {
            super::super::like::time(caller, &t, None)?
        }
        _ => civil::Time::midnight(),
    };
    Ok((tz_id, time))
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let ty = || shared::temporal_type("PlainDate");
    let receiver = || Param::new("receiver", ty());
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("PlainDateConstructor", "from"),
        vec![
            Param::new("item", super::super::like::like_type("PlainDate")),
            super::super::like::overflow_param(),
        ],
        ty(),
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDate", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("PlainDate", "equals"),
        vec![
            receiver(),
            Param::new("other", super::super::like::like_type("PlainDate")),
        ],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("PlainDateConstructor", "compare"),
        vec![
            Param::new("a", super::super::like::like_type("PlainDate")),
            Param::new("b", super::super::like::like_type("PlainDate")),
        ],
        Type::Number,
    );
    declare_arithmetic(defs, &receiver, &ty);
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("PlainDate", "with"),
        vec![receiver(), Param::new("fields", Type::any_object())],
        ty(),
    );
    for (method, ret) in [
        ("toPlainYearMonth", "PlainYearMonth"),
        ("toPlainMonthDay", "PlainMonthDay"),
    ] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDate", method),
            vec![receiver()],
            shared::temporal_type(ret),
        );
    }
    shared::declare_direct_fn(
        defs,
        "toPlainDateTime",
        shared::prelude_key("PlainDate", "toPlainDateTime"),
        vec![
            receiver(),
            Param::with_default(
                "time",
                Type::Union(vec![
                    super::super::like::like_type("PlainTime"),
                    Type::Undefined,
                ]),
                DefaultValue::Undefined,
            ),
        ],
        shared::temporal_type("PlainDateTime"),
    );
    shared::declare_direct_fn(
        defs,
        "toZonedDateTime",
        shared::prelude_key("PlainDate", "toZonedDateTime"),
        vec![
            receiver(),
            Param::new(
                "timeZoneOrOptions",
                Type::Union(vec![
                    super::super::like::like_type("TimeZone"),
                    shared::temporal_type("PlainDateToZonedOptions"),
                ]),
            ),
        ],
        shared::temporal_type("ZonedDateTime"),
    );
    for (field, ret) in date_fields() {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("PlainDate", field),
            vec![receiver()],
            ret,
        );
    }
}

fn declare_arithmetic(
    defs: &mut PackageDeclaration,
    receiver: &impl Fn() -> Param,
    ty: &impl Fn() -> Type,
) {
    let duration_like = || {
        Param::new(
            "duration",
            Type::Union(vec![
                Type::String,
                shared::temporal_type("Duration"),
                shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
            ]),
        )
    };
    for method in ["add", "subtract"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDate", method),
            vec![receiver(), duration_like()],
            ty(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDate", method),
            vec![
                receiver(),
                Param::new("other", super::super::like::like_type("PlainDate")),
                Param::with_default(
                    "options",
                    Type::Union(vec![Type::any_object(), Type::Undefined]),
                    DefaultValue::Undefined,
                ),
            ],
            shared::temporal_type("Duration"),
        );
    }
}

fn date_fields() -> Vec<(&'static str, Type)> {
    vec![
        ("year", Type::Number),
        ("month", Type::Number),
        ("day", Type::Number),
        ("dayOfWeek", Type::Number),
        ("monthCode", Type::String),
        ("dayOfYear", Type::Number),
        ("weekOfYear", Type::Number),
        ("yearOfWeek", Type::Number),
        ("daysInWeek", Type::Number),
        ("daysInMonth", Type::Number),
        ("daysInYear", Type::Number),
        ("monthsInYear", Type::Number),
        ("inLeapYear", Type::Boolean),
    ]
}
