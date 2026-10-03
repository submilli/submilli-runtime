use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let obj = &types.object;
    shared::reg_plain_from(
        linker,
        engine,
        &types.string,
        obj,
        "PlainDateTimeConstructor",
        "PlainDateTime#from",
        |s, caller| {
            let dt = super::parse(s).map_err(crate::runtime::host::range_error)?;
            shared::make_plain_date_time(
                caller,
                i32::from(dt.date().year()),
                i32::from(dt.date().month()),
                i32::from(dt.date().day()),
                i32::from(dt.time().hour()),
                i32::from(dt.time().minute()),
                i32::from(dt.time().second()),
                dt.time().subsec_nanosecond(),
            )
        },
    )?;
    for method in ["toString", "toJSON"] {
        shared::reg_plain_string(
            linker,
            engine,
            obj,
            &types.string,
            "PlainDateTime",
            method,
            shared::plain_to_string_date_time,
        )?;
    }
    shared::reg_plain_equals(
        linker,
        engine,
        obj,
        "PlainDateTime",
        shared::plain_equals_date_time,
    )?;
    shared::reg_plain_compare(
        linker,
        engine,
        obj,
        "PlainDateTimeConstructor",
        &[1, 2, 3, 4, 5, 6, 7],
    )?;
    for (name, field) in [
        ("year", 1),
        ("month", 2),
        ("day", 3),
        ("hour", 4),
        ("minute", 5),
        ("second", 6),
    ] {
        shared::reg_plain_i32_getter(linker, engine, obj, "PlainDateTime", name, field)?;
    }
    shared::reg_plain_time_getters(linker, engine, obj, "PlainDateTime")?;
    shared::reg_plain_date_derived_getters(linker, engine, obj, "PlainDateTime", 0)?;

    let pair = FuncType::new(engine, [obj.clone(), obj.clone()], [obj.clone()]);
    for method in ["add", "subtract"] {
        reg_plain_date_time_duration_op(linker, types.intr.clone(), method, pair.clone())?;
    }
    reg_plain_date_time_with(
        linker,
        engine,
        FuncType::new(
            engine,
            [obj.clone(), types.object_shape.clone()],
            [obj.clone()],
        ),
    )?;
    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(types.intr.object_shape.clone()),
    ));
    let pair_with_options =
        FuncType::new(engine, [obj.clone(), obj.clone(), options], [obj.clone()]);
    for method in ["until", "since"] {
        reg_plain_date_time_pair_op(linker, method, pair_with_options.clone())?;
    }
    let receiver_ty = FuncType::new(engine, [obj.clone()], [obj.clone()]);
    for method in [
        "toPlainDate",
        "toPlainTime",
        "toPlainYearMonth",
        "toPlainMonthDay",
    ] {
        if matches!(method, "toPlainDate" | "toPlainTime") {
            reg_plain_date_time_projection(linker, method, receiver_ty.clone())?;
        } else if method == "toPlainYearMonth" {
            shared::reg_plain_date_to_year_month(linker, "PlainDateTime", receiver_ty.clone())?;
        } else {
            shared::reg_plain_date_to_month_day(linker, "PlainDateTime", receiver_ty.clone())?;
        }
    }
    reg_plain_date_time_to_zoned(
        linker,
        FuncType::new(engine, [obj.clone(), types.string.clone()], [obj.clone()]),
    )
}

fn reg_plain_date_time_duration_op(
    linker: &mut Linker<StoreData>,
    intr: IntrinsicTypes,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainDateTime.{method}");
    let add = method == "add";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDateTime", method),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let dt = plain_date_from_struct(caller, st, &label)?
                .to_datetime(plain_time_from_struct(caller, st, 3, &label)?);
            let span = read_duration_like(caller, abi_arg(params, 1)?, &intr, &label)?;
            let out = if add {
                super::add(dt, span)
            } else {
                super::subtract(dt, span)
            }
            .map_err(crate::runtime::host::range_error)?;
            *abi_result(results, 0)? = make_plain_date_time(
                caller,
                i32::from(out.date().year()),
                i32::from(out.date().month()),
                i32::from(out.date().day()),
                i32::from(out.time().hour()),
                i32::from(out.time().minute()),
                i32::from(out.time().second()),
                out.time().subsec_nanosecond(),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_date_time_with(
    linker: &mut Linker<StoreData>,
    _engine: &wasmtime::Engine,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDateTime", "with"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainDateTime.with")?;
            let (y, m, d) = merge_date(
                (
                    st_i32(caller, st, 1, "PlainDateTime.with")?,
                    st_i32(caller, st, 2, "PlainDateTime.with")?,
                    st_i32(caller, st, 3, "PlainDateTime.with")?,
                ),
                object_i64_field(caller, abi_arg(params, 1)?, "year", "PlainDateTime.with")?,
                object_i64_field(caller, abi_arg(params, 1)?, "month", "PlainDateTime.with")?,
                object_i64_field(caller, abi_arg(params, 1)?, "day", "PlainDateTime.with")?,
                "PlainDateTime.with",
            )?;
            let time = merge_time(
                (
                    st_i32(caller, st, 4, "PlainDateTime.with")?,
                    st_i32(caller, st, 5, "PlainDateTime.with")?,
                    st_i32(caller, st, 6, "PlainDateTime.with")?,
                    st_i32(caller, st, 7, "PlainDateTime.with")?,
                ),
                object_time_bag(caller, abi_arg(params, 1)?, "PlainDateTime.with")?,
                "PlainDateTime.with",
            )?;
            let date = super::super::shared::y_m_d_date(y, m, d, "PlainDateTime.with")?;
            *abi_result(results, 0)? = make_plain_date_time(
                caller,
                i32::from(date.year()),
                i32::from(date.month()),
                i32::from(date.day()),
                i32::from(time.hour()),
                i32::from(time.minute()),
                i32::from(time.second()),
                time.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_date_time_pair_op(
    linker: &mut Linker<StoreData>,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainDateTime.{method}");
    let until = method == "until";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDateTime", method),
        ty,
        true,
        move |caller, params, results| {
            let a_st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let b_st = as_struct_val(caller, abi_arg(params, 1)?, &label)?;
            let a = plain_date_from_struct(caller, a_st, &label)?
                .to_datetime(plain_time_from_struct(caller, a_st, 3, &label)?);
            let b = plain_date_from_struct(caller, b_st, &label)?
                .to_datetime(plain_time_from_struct(caller, b_st, 3, &label)?);
            let o = object_diff_options(
                caller,
                abi_arg(params, 2)?,
                if until {
                    "PlainDateTime.until"
                } else {
                    "PlainDateTime.since"
                },
            )?;
            let span = if until {
                a.until(o.datetime(b))
                    .map_err(temporal_err("PlainDateTime.until"))?
            } else {
                a.since(o.datetime(b))
                    .map_err(temporal_err("PlainDateTime.since"))?
            };
            *abi_result(results, 0)? = make_duration(caller, &span)?;
            Ok(())
        },
    )
}

fn reg_plain_date_time_projection(
    linker: &mut Linker<StoreData>,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDateTime", method),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, method)?;
            *abi_result(results, 0)? = if method == "toPlainDate" {
                let y = st_i32(caller, st, 1, method)?;
                let m = st_i32(caller, st, 2, method)?;
                let d = st_i32(caller, st, 3, method)?;
                make_plain_date(caller, y, m, d)?
            } else {
                let h = st_i32(caller, st, 4, method)?;
                let m = st_i32(caller, st, 5, method)?;
                let s = st_i32(caller, st, 6, method)?;
                let ns = st_i32(caller, st, 7, method)?;
                make_plain_time(caller, h, m, s, ns)?
            };
            Ok(())
        },
    )
}

fn reg_plain_date_time_to_zoned(
    linker: &mut Linker<StoreData>,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainDateTime", "toZonedDateTime"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainDateTime.toZonedDateTime")?;
            let tz_id =
                read_string_arg(caller, abi_arg(params, 1)?, "PlainDateTime.toZonedDateTime")?;
            let date = plain_date_from_struct(caller, st, "PlainDateTime.toZonedDateTime")?;
            let time = plain_time_from_struct(caller, st, 3, "PlainDateTime.toZonedDateTime")?;
            let (tz, canonical_id) = super::super::zoned_date_time::resolve_time_zone(
                caller,
                &tz_id,
                "PlainDateTime.toZonedDateTime",
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

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let ty = || shared::temporal_type("PlainDateTime");
    let receiver = || Param::new("receiver", ty());
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("PlainDateTimeConstructor", "from"),
        vec![Param::new("s", Type::String)],
        ty(),
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDateTime", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("PlainDateTime", "equals"),
        vec![receiver(), Param::new("other", ty())],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("PlainDateTimeConstructor", "compare"),
        vec![Param::new("a", ty()), Param::new("b", ty())],
        Type::Number,
    );
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
            shared::prelude_key("PlainDateTime", method),
            vec![receiver(), duration_like()],
            ty(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDateTime", method),
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
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("PlainDateTime", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        ty(),
    );
    for (method, ret) in [
        ("toPlainDate", "PlainDate"),
        ("toPlainTime", "PlainTime"),
        ("toPlainYearMonth", "PlainYearMonth"),
        ("toPlainMonthDay", "PlainMonthDay"),
    ] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainDateTime", method),
            vec![receiver()],
            shared::temporal_type(ret),
        );
    }
    shared::declare_direct_fn(
        defs,
        "toZonedDateTime",
        shared::prelude_key("PlainDateTime", "toZonedDateTime"),
        vec![receiver(), Param::new("timeZone", Type::String)],
        shared::temporal_type("ZonedDateTime"),
    );
    for (field, ret) in fields() {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("PlainDateTime", field),
            vec![receiver()],
            ret,
        );
    }
}

fn fields() -> Vec<(&'static str, Type)> {
    let mut out = vec![
        ("year", Type::Number),
        ("month", Type::Number),
        ("day", Type::Number),
        ("hour", Type::Number),
        ("minute", Type::Number),
        ("second", Type::Number),
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
    ];
    out.extend([
        ("millisecond", Type::Number),
        ("microsecond", Type::Number),
        ("nanosecond", Type::Number),
    ]);
    out
}
