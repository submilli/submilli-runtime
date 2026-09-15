use jiff::civil;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::collection::object_field;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let obj = &types.object;
    shared::reg_plain_from(
        linker,
        engine,
        &types.string,
        obj,
        "PlainDateConstructor",
        "PlainDate#from",
        |s, caller| {
            let date = super::parse(s).map_err(crate::runtime::host::range_error)?;
            shared::make_plain_date(
                caller,
                i32::from(date.year()),
                i32::from(date.month()),
                i32::from(date.day()),
            )
        },
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
    shared::reg_plain_equals(linker, engine, obj, "PlainDate", shared::plain_equals_date)?;
    shared::reg_plain_compare(linker, engine, obj, "PlainDateConstructor", &[1, 2, 3])?;
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
        HeapType::ConcreteStruct(types.intr.object_shape.clone()),
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
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, &params[0], &label)?;
            let date = plain_date_from_struct(caller, st, &label)?;
            let span = read_duration_like(caller, &params[1], &intr, &label)?;
            let out = if add {
                super::add(date, span)
            } else {
                super::subtract(date, span)
            }
            .map_err(crate::runtime::host::range_error)?;
            results[0] = make_plain_date(
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
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, &params[0], "PlainDate.with")?;
            let (y, m, d) = merge_date(
                (
                    st_i32(caller, st, 1, "PlainDate.with")?,
                    st_i32(caller, st, 2, "PlainDate.with")?,
                    st_i32(caller, st, 3, "PlainDate.with")?,
                ),
                object_i64_field(caller, &params[1], "year", "PlainDate.with")?,
                object_i64_field(caller, &params[1], "month", "PlainDate.with")?,
                object_i64_field(caller, &params[1], "day", "PlainDate.with")?,
                "PlainDate.with",
            )?;
            let date = super::super::shared::y_m_d_date(y, m, d, "PlainDate.with")?;
            results[0] = make_plain_date(
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
        true,
        move |caller, params, results| {
            let a_st = as_struct_val(caller, &params[0], &label)?;
            let b_st = as_struct_val(caller, &params[1], &label)?;
            let a = plain_date_from_struct(caller, a_st, &label)?;
            let b = plain_date_from_struct(caller, b_st, &label)?;
            let o = object_diff_options(
                caller,
                &params[2],
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
            results[0] = make_duration(caller, &span)?;
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
        true,
        |caller, params, results| {
            let d = as_struct_val(caller, &params[0], "PlainDate.toPlainDateTime")?;
            let maybe_t = params[1];
            let (h, mi, s, ns) = if let Val::AnyRef(Some(_)) = maybe_t {
                let t = as_struct_val(caller, &maybe_t, "PlainDate.toPlainDateTime")?;
                (
                    st_i32(caller, t, 1, "PlainDate.toPlainDateTime")?,
                    st_i32(caller, t, 2, "PlainDate.toPlainDateTime")?,
                    st_i32(caller, t, 3, "PlainDate.toPlainDateTime")?,
                    st_i32(caller, t, 4, "PlainDate.toPlainDateTime")?,
                )
            } else {
                (0, 0, 0, 0)
            };
            let y = st_i32(caller, d, 1, "PlainDate.toPlainDateTime")?;
            let m = st_i32(caller, d, 2, "PlainDate.toPlainDateTime")?;
            let day = st_i32(caller, d, 3, "PlainDate.toPlainDateTime")?;
            results[0] = make_plain_date_time(caller, y, m, day, h, mi, s, ns)?;
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
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, &params[0], "PlainDate.toZonedDateTime")?;
            let date = plain_date_from_struct(caller, st, "PlainDate.toZonedDateTime")?;
            let (tz_id, time) = plain_date_to_zoned_arg(caller, &params[1])?;
            let (tz, canonical_id) = super::super::zoned_date_time::resolve_time_zone(
                &tz_id,
                "PlainDate.toZonedDateTime",
            )
            .map_err(crate::runtime::host::range_error)?;
            let z = date
                .to_datetime(time)
                .to_zoned(tz)
                .map_err(temporal_err("toZonedDateTime"))?;
            results[0] = make_zoned_date_time(caller, &z, &canonical_id)?;
            Ok(())
        },
    )
}

fn plain_date_to_zoned_arg(
    caller: &mut Caller<'_, StoreData>,
    arg: &Val,
) -> wasmtime::Result<(String, civil::Time)> {
    if read_string_arg(caller, arg, "PlainDate.toZonedDateTime").is_ok() {
        return Ok((
            read_string_arg(caller, arg, "PlainDate.toZonedDateTime")?,
            civil::Time::midnight(),
        ));
    }
    let tz_id = object_string_field(caller, arg, "timeZone", "PlainDate.toZonedDateTime")?
        .ok_or_else(|| {
            wasmtime::Error::msg("Temporal.PlainDate.toZonedDateTime: `timeZone` is required")
        })?;
    let time = match object_field(caller, arg, "plainTime")? {
        Some(Val::AnyRef(Some(_))) => {
            let t = object_field(caller, arg, "plainTime")?.expect("plainTime reread");
            let st = as_struct_val(caller, &t, "PlainDate.toZonedDateTime")?;
            plain_time_from_struct(caller, st, 0, "PlainDate.toZonedDateTime")?
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
        vec![Param::new("s", Type::String)],
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
        vec![receiver(), Param::new("other", ty())],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("PlainDateConstructor", "compare"),
        vec![Param::new("a", ty()), Param::new("b", ty())],
        Type::Number,
    );
    declare_arithmetic(defs, &receiver, &ty);
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("PlainDate", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
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
                Type::Union(vec![shared::temporal_type("PlainTime"), Type::Null]),
                DefaultValue::Null,
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
                    Type::String,
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
                Param::new("other", ty()),
                Param::with_default(
                    "options",
                    Type::Union(vec![shared::object_shape_type(&[]), Type::Null]),
                    DefaultValue::Null,
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
