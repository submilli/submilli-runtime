use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::register_host_fn;
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
        "PlainYearMonthConstructor",
        "PlainYearMonth#from",
        |s, caller| {
            let (year, month) = super::parse(s).map_err(crate::runtime::host::range_error)?;
            shared::make_plain_year_month(caller, year, month)
        },
    )?;
    for method in ["toString", "toJSON"] {
        shared::reg_plain_string(
            linker,
            engine,
            obj,
            &types.string,
            "PlainYearMonth",
            method,
            shared::plain_to_string_year_month,
        )?;
    }
    shared::reg_plain_equals(
        linker,
        engine,
        obj,
        "PlainYearMonth",
        shared::plain_equals_year_month,
    )?;
    shared::reg_plain_compare(linker, engine, obj, "PlainYearMonthConstructor", &[1, 2])?;
    for (name, field) in [("year", 1), ("month", 2)] {
        shared::reg_plain_i32_getter(linker, engine, obj, "PlainYearMonth", name, field)?;
    }
    shared::reg_plain_date_derived_getters(linker, engine, obj, "PlainYearMonth", 1)?;
    let pair = FuncType::new(engine, [obj.clone(), obj.clone()], [obj.clone()]);
    for method in ["add", "subtract"] {
        reg_plain_year_month_duration_op(linker, types.intr.clone(), method, pair.clone())?;
    }
    reg_plain_year_month_with(
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
        reg_plain_year_month_pair_op(linker, method, pair_with_options.clone())?;
    }
    reg_plain_year_month_to_plain_date(
        linker,
        FuncType::new(
            engine,
            [obj.clone(), types.object_shape.clone()],
            [obj.clone()],
        ),
    )
}

fn reg_plain_year_month_duration_op(
    linker: &mut Linker<StoreData>,
    intr: IntrinsicTypes,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainYearMonth.{method}");
    let add = method == "add";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainYearMonth", method),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, &params[0], &label)?;
            let y = st_i32(caller, st, 1, &label)?;
            let m = st_i32(caller, st, 2, &label)?;
            let span = read_duration_like(caller, &params[1], &intr, &label)?;
            let reference = year_month_reference(
                y,
                m,
                if add {
                    span.signum() < 0
                } else {
                    span.signum() > 0
                },
                if add {
                    "PlainYearMonth.add"
                } else {
                    "PlainYearMonth.subtract"
                },
            )?;
            let out = if add {
                reference.checked_add(span)
            } else {
                reference.checked_sub(span)
            }
            .map_err(temporal_err(if add {
                "PlainYearMonth.add"
            } else {
                "PlainYearMonth.subtract"
            }))?;
            results[0] =
                make_plain_year_month(caller, i32::from(out.year()), i32::from(out.month()))?;
            Ok(())
        },
    )
}

fn reg_plain_year_month_with(
    linker: &mut Linker<StoreData>,
    _engine: &wasmtime::Engine,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainYearMonth", "with"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, &params[0], "PlainYearMonth.with")?;
            let y = object_i64_field(caller, &params[1], "year", "PlainYearMonth.with")?
                .unwrap_or_else(|| i64::from(st_i32_or(caller, st, 1)));
            let m = clamp_month(
                object_i64_field(caller, &params[1], "month", "PlainYearMonth.with")?
                    .unwrap_or_else(|| i64::from(st_i32_or(caller, st, 2))),
            );
            let date = y_m_d_date(y, m, 1, "PlainYearMonth.with")?;
            results[0] =
                make_plain_year_month(caller, i32::from(date.year()), i32::from(date.month()))?;
            Ok(())
        },
    )
}

fn reg_plain_year_month_pair_op(
    linker: &mut Linker<StoreData>,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainYearMonth.{method}");
    let until = method == "until";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainYearMonth", method),
        ty,
        true,
        move |caller, params, results| {
            let a = as_struct_val(caller, &params[0], &label)?;
            let b = as_struct_val(caller, &params[1], &label)?;
            let ay = st_i32(caller, a, 1, &label)?;
            let am = st_i32(caller, a, 2, &label)?;
            let by = st_i32(caller, b, 1, &label)?;
            let bm = st_i32(caller, b, 2, &label)?;
            let o = object_diff_options(
                caller,
                &params[2],
                if until {
                    "PlainYearMonth.until"
                } else {
                    "PlainYearMonth.since"
                },
            )?;
            let span = if until {
                year_month_span(ay, am, by, bm, &o, "PlainYearMonth.until")?
            } else {
                year_month_span(by, bm, ay, am, &o, "PlainYearMonth.since")?
            };
            results[0] = make_duration(caller, &span)?;
            Ok(())
        },
    )
}

fn reg_plain_year_month_to_plain_date(
    linker: &mut Linker<StoreData>,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainYearMonth", "toPlainDate"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, &params[0], "PlainYearMonth.toPlainDate")?;
            let year = i64::from(st_i32(caller, st, 1, "PlainYearMonth.toPlainDate")?);
            let month = clamp_month(i64::from(st_i32(
                caller,
                st,
                2,
                "PlainYearMonth.toPlainDate",
            )?));
            let dim = days_in_month(year, month, "PlainYearMonth.toPlainDate")?;
            let day = object_i64_field(caller, &params[1], "day", "PlainYearMonth.toPlainDate")?
                .unwrap_or(1)
                .clamp(1, dim);
            let d = y_m_d_date(year, month, day, "PlainYearMonth.toPlainDate")?;
            results[0] = make_plain_date(
                caller,
                i32::from(d.year()),
                i32::from(d.month()),
                i32::from(d.day()),
            )?;
            Ok(())
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let ty = || shared::temporal_type("PlainYearMonth");
    let receiver = || Param::new("receiver", ty());
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("PlainYearMonthConstructor", "from"),
        vec![Param::new("s", Type::String)],
        ty(),
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainYearMonth", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("PlainYearMonth", "equals"),
        vec![receiver(), Param::new("other", ty())],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("PlainYearMonthConstructor", "compare"),
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
            shared::prelude_key("PlainYearMonth", method),
            vec![receiver(), duration_like()],
            ty(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainYearMonth", method),
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
        shared::prelude_key("PlainYearMonth", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        ty(),
    );
    shared::declare_direct_fn(
        defs,
        "toPlainDate",
        shared::prelude_key("PlainYearMonth", "toPlainDate"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        shared::temporal_type("PlainDate"),
    );
    for (field, ret) in [
        ("year", Type::Number),
        ("month", Type::Number),
        ("monthCode", Type::String),
        ("daysInMonth", Type::Number),
        ("daysInYear", Type::Number),
        ("monthsInYear", Type::Number),
        ("inLeapYear", Type::Boolean),
    ] {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("PlainYearMonth", field),
            vec![receiver()],
            ret,
        );
    }
}
