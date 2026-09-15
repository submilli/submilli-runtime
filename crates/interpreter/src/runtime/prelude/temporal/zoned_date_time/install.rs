use jiff::Zoned;
use num_bigint::BigInt;
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn, write_submilli_string_struct};
use crate::runtime::prelude::MODULE_NAME;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

type NumericGetter = fn(&Zoned) -> f64;

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    install_construction(linker, types)?;
    install_getters(linker, types)?;
    install_arithmetic(linker, types)?;
    install_updates(linker, types)?;
    install_conversions(linker, types)?;
    install_strings(linker, types)
}

fn install_construction(
    linker: &mut Linker<StoreData>,
    types: &DirectTypes,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTimeConstructor", "from"),
        FuncType::new(
            &types.engine,
            [types.string.clone()],
            [types.object.clone()],
        ),
        true,
        |caller, params, results| {
            let input = read_string_arg(caller, &params[0], "Temporal.ZonedDateTime.from")?;
            let (zoned, tz_id) = super::parse(&input).map_err(crate::runtime::host::range_error)?;
            results[0] = shared::make_zoned_date_time(caller, &zoned, &tz_id)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTimeConstructor", "compare"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [ValType::F64],
        ),
        true,
        |caller, params, results| {
            let a = shared::zoned_date_time_timestamp_from_val(
                caller,
                &params[0],
                "ZonedDateTime.compare",
            )?;
            let b = shared::zoned_date_time_timestamp_from_val(
                caller,
                &params[1],
                "ZonedDateTime.compare",
            )?;
            results[0] = Val::F64(super::compare_timestamps(a, b).to_bits());
            Ok(())
        },
    )
}

fn install_getters(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "epochMilliseconds"),
        FuncType::new(&types.engine, [types.object.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            let timestamp = shared::zoned_date_time_timestamp_from_val(
                caller,
                &params[0],
                "ZonedDateTime.epochMilliseconds",
            )?;
            results[0] = Val::F64(shared::epoch_milliseconds(timestamp).to_bits());
            Ok(())
        },
    )?;

    let numeric: [(&str, NumericGetter); 18] = [
        ("year", |z| f64::from(z.year())),
        ("month", |z| f64::from(z.month())),
        ("day", |z| f64::from(z.day())),
        ("hour", |z| f64::from(z.hour())),
        ("minute", |z| f64::from(z.minute())),
        ("second", |z| f64::from(z.second())),
        ("dayOfWeek", |z| {
            f64::from(z.weekday().to_monday_one_offset())
        }),
        ("dayOfYear", |z| f64::from(z.day_of_year())),
        ("weekOfYear", |z| f64::from(z.date().iso_week_date().week())),
        ("yearOfWeek", |z| f64::from(z.date().iso_week_date().year())),
        ("daysInWeek", |_| 7.0),
        ("daysInMonth", |z| f64::from(z.days_in_month())),
        ("daysInYear", |z| f64::from(z.days_in_year())),
        ("monthsInYear", |_| 12.0),
        ("millisecond", |z| f64::from(z.millisecond())),
        ("microsecond", |z| f64::from(z.microsecond())),
        ("nanosecond", |z| f64::from(z.nanosecond())),
        ("offsetNanoseconds", |z| {
            f64::from(z.offset().seconds()) * 1_000_000_000.0
        }),
    ];
    for (name, getter) in numeric {
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", name),
            FuncType::new(&types.engine, [types.object.clone()], [ValType::F64]),
            true,
            move |caller, params, results| {
                let zoned = shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime")?;
                results[0] = Val::F64(getter(&zoned).to_bits());
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "timeZoneId"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.string.clone()],
        ),
        true,
        |caller, params, results| {
            let text = shared::zoned_date_time_time_zone_id_from_val(
                caller,
                &params[0],
                "ZonedDateTime.timeZoneId",
            )?;
            results[0] = Val::AnyRef(Some(
                write_submilli_string_struct(caller, &text)?.to_anyref(),
            ));
            Ok(())
        },
    )?;

    for (name, getter) in [
        ("offset", super::offset as fn(&Zoned, &str) -> String),
        ("monthCode", super::month_code as fn(&Zoned, &str) -> String),
    ] {
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", name),
            FuncType::new(
                &types.engine,
                [types.object.clone()],
                [types.string.clone()],
            ),
            true,
            move |caller, params, results| {
                let (zoned, tz_id) =
                    shared::zoned_date_time_parts_from_val(caller, &params[0], "ZonedDateTime")?;
                let text = getter(&zoned, &tz_id);
                results[0] = Val::AnyRef(Some(
                    write_submilli_string_struct(caller, &text)?.to_anyref(),
                ));
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "inLeapYear"),
        FuncType::new(&types.engine, [types.object.clone()], [ValType::I32]),
        true,
        |caller, params, results| {
            let zoned = shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime")?;
            results[0] = Val::I32(zoned.in_leap_year() as i32);
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "hoursInDay"),
        FuncType::new(&types.engine, [types.object.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            let zoned = shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime")?;
            results[0] = Val::F64(
                super::hours_in_day(&zoned)
                    .map_err(crate::runtime::host::range_error)?
                    .to_bits(),
            );
            Ok(())
        },
    )?;

    let bigint = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(types.intr.bigint.clone()),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "epochNanoseconds"),
        FuncType::new(&types.engine, [types.object.clone()], [bigint]),
        true,
        |caller, params, results| {
            let timestamp = shared::zoned_date_time_timestamp_from_val(
                caller,
                &params[0],
                "ZonedDateTime.epochNanoseconds",
            )?;
            results[0] = crate::runtime::prelude::bigint::ops::make_bigint_struct(
                caller,
                BigInt::from(timestamp.as_nanosecond()),
            )?;
            Ok(())
        },
    )
}

fn install_arithmetic(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let intr = types.intr.clone();
    for (method, add) in [("add", true), ("subtract", false)] {
        let intr = intr.clone();
        let label = if add {
            "ZonedDateTime.add"
        } else {
            "ZonedDateTime.subtract"
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", method),
            FuncType::new(
                &types.engine,
                [types.object.clone(), types.object.clone()],
                [types.object.clone()],
            ),
            true,
            move |caller, params, results| {
                let (zoned, tz_id) =
                    shared::zoned_date_time_parts_from_val(caller, &params[0], label)?;
                let span = shared::read_duration_like(caller, &params[1], &intr, label)?;
                let out = if add {
                    super::add(&zoned, span)
                } else {
                    super::subtract(&zoned, span)
                }
                .map_err(crate::runtime::host::range_error)?;
                results[0] = shared::make_zoned_date_time(caller, &out, &tz_id)?;
                Ok(())
            },
        )?;
    }

    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(types.intr.object_shape.clone()),
    ));
    for (method, until) in [("until", true), ("since", false)] {
        let label = if until {
            "ZonedDateTime.until"
        } else {
            "ZonedDateTime.since"
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", method),
            FuncType::new(
                &types.engine,
                [types.object.clone(), types.object.clone(), options.clone()],
                [types.object.clone()],
            ),
            true,
            move |caller, params, results| {
                let a = shared::zoned_date_time_from_val(caller, &params[0], label)?;
                let b = shared::zoned_date_time_from_val(caller, &params[1], label)?;
                let options = shared::object_diff_options(caller, &params[2], label)?;
                let span = if until {
                    a.until(options.zoned(&b))
                } else {
                    a.since(options.zoned(&b))
                }
                .map_err(|_| {
                    crate::runtime::host::range_error(shared::temporal_error(
                        label,
                        format!(
                            "cannot compute the difference with {}; use compatible largestUnit and smallestUnit values and a roundingIncrement valid for smallestUnit",
                            options.description()
                        ),
                    ))
                })?;
                results[0] = shared::make_duration(caller, &span)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

fn install_updates(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let string_type = types.intr.string.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "withTimeZone"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.string.clone()],
            [types.object.clone()],
        ),
        true,
        move |caller, params, results| {
            let zoned =
                shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime.withTimeZone")?;
            let tz_id = read_string_arg(caller, &params[1], "Temporal.ZonedDateTime.withTimeZone")?;
            let (out, canonical_id) =
                super::with_time_zone(&zoned, &tz_id).map_err(crate::runtime::host::range_error)?;
            results[0] = shared::make_zoned_date_time(caller, &out, &canonical_id)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "with"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [types.object.clone()],
        ),
        true,
        |caller, params, results| {
            let (zoned, tz_id) =
                shared::zoned_date_time_parts_from_val(caller, &params[0], "ZonedDateTime.with")?;
            let dt = zoned.datetime();
            let (year, month, day) = shared::merge_date(
                (
                    i32::from(dt.year()),
                    i32::from(dt.month()),
                    i32::from(dt.day()),
                ),
                shared::object_i64_field(caller, &params[1], "year", "ZonedDateTime.with")?,
                shared::object_i64_field(caller, &params[1], "month", "ZonedDateTime.with")?,
                shared::object_i64_field(caller, &params[1], "day", "ZonedDateTime.with")?,
                "ZonedDateTime.with",
            )?;
            let time = shared::merge_time(
                (
                    i32::from(dt.hour()),
                    i32::from(dt.minute()),
                    i32::from(dt.second()),
                    dt.subsec_nanosecond(),
                ),
                shared::object_time_bag(caller, &params[1], "ZonedDateTime.with")?,
                "ZonedDateTime.with",
            )?;
            let date = shared::y_m_d_date(year, month, day, "ZonedDateTime.with")?;
            let out = super::with_fields(&zoned, date, time)
                .map_err(crate::runtime::host::range_error)?;
            results[0] = shared::make_zoned_date_time(caller, &out, &tz_id)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "round"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [types.object.clone()],
        ),
        true,
        move |caller, params, results| {
            let (zoned, tz_id) =
                shared::zoned_date_time_parts_from_val(caller, &params[0], "ZonedDateTime.round")?;
            let (unit, mode, increment) =
                if crate::runtime::prelude::collection::is_a(caller, &params[1], &string_type)? {
                    (
                        read_string_arg(caller, &params[1], "Temporal.ZonedDateTime.round")?,
                        None,
                        None,
                    )
                } else {
                    let unit = shared::object_string_field(
                        caller,
                        &params[1],
                        "smallestUnit",
                        "ZonedDateTime.round",
                    )?
                    .ok_or_else(|| {
                        wasmtime::Error::msg(
                            "Temporal.ZonedDateTime.round: smallestUnit is required",
                        )
                    })?;
                    let mode = shared::object_string_field(
                        caller,
                        &params[1],
                        "roundingMode",
                        "ZonedDateTime.round",
                    )?
                    .map(|mode| shared::round_mode_from_str(&mode, "ZonedDateTime.round"))
                    .transpose()?;
                    let increment = shared::object_i64_field(
                        caller,
                        &params[1],
                        "roundingIncrement",
                        "ZonedDateTime.round",
                    )?;
                    (unit, mode, increment)
                };
            let unit = shared::unit_from_str(&unit, "ZonedDateTime.round")?;
            let out = super::round(&zoned, unit, mode, increment)
                .map_err(crate::runtime::host::range_error)?;
            results[0] = shared::make_zoned_date_time(caller, &out, &tz_id)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "startOfDay"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        true,
        move |caller, params, results| {
            let (zoned, tz_id) = shared::zoned_date_time_parts_from_val(
                caller,
                &params[0],
                "ZonedDateTime.startOfDay",
            )?;
            let out = super::start_of_day(&zoned).map_err(crate::runtime::host::range_error)?;
            results[0] = shared::make_zoned_date_time(caller, &out, &tz_id)?;
            Ok(())
        },
    )?;
    Ok(())
}

fn install_conversions(
    linker: &mut Linker<StoreData>,
    types: &DirectTypes,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "toInstant"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        true,
        |caller, params, results| {
            let timestamp = shared::zoned_date_time_timestamp_from_val(
                caller,
                &params[0],
                "ZonedDateTime.toInstant",
            )?;
            results[0] = shared::make_instant(caller, timestamp)?;
            Ok(())
        },
    )?;
    for method in ["toPlainDate", "toPlainTime", "toPlainDateTime"] {
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", method),
            FuncType::new(
                &types.engine,
                [types.object.clone()],
                [types.object.clone()],
            ),
            true,
            move |caller, params, results| {
                let zoned = shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime")?;
                results[0] = match method {
                    "toPlainDate" => shared::make_plain_date(
                        caller,
                        i32::from(zoned.year()),
                        i32::from(zoned.month()),
                        i32::from(zoned.day()),
                    )?,
                    "toPlainTime" => shared::make_plain_time(
                        caller,
                        i32::from(zoned.hour()),
                        i32::from(zoned.minute()),
                        i32::from(zoned.second()),
                        zoned.datetime().subsec_nanosecond(),
                    )?,
                    "toPlainDateTime" => shared::make_plain_date_time(
                        caller,
                        i32::from(zoned.year()),
                        i32::from(zoned.month()),
                        i32::from(zoned.day()),
                        i32::from(zoned.hour()),
                        i32::from(zoned.minute()),
                        i32::from(zoned.second()),
                        zoned.datetime().subsec_nanosecond(),
                    )?,
                    _ => unreachable!(),
                };
                Ok(())
            },
        )?;
    }
    Ok(())
}

fn install_strings(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    for method in ["toString", "toJSON"] {
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("ZonedDateTime", method),
            FuncType::new(
                &types.engine,
                [types.object.clone()],
                [types.string.clone()],
            ),
            true,
            move |caller, params, results| {
                let zoned = shared::zoned_date_time_from_val(caller, &params[0], "ZonedDateTime")?;
                results[0] = Val::AnyRef(Some(
                    write_submilli_string_struct(caller, &zoned.to_string())?.to_anyref(),
                ));
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("ZonedDateTime", "equals"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [ValType::I32],
        ),
        true,
        |caller, params, results| {
            let (a, a_tz) = shared::zoned_date_time_identity_from_val(
                caller,
                &params[0],
                "ZonedDateTime.equals",
            )?;
            let (b, b_tz) = shared::zoned_date_time_identity_from_val(
                caller,
                &params[1],
                "ZonedDateTime.equals",
            )?;
            results[0] = Val::I32((a == b && super::time_zone_ids_equal(&a_tz, &b_tz)) as i32);
            Ok(())
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let zoned = || shared::temporal_type("ZonedDateTime");
    let receiver = || Param::new("receiver", zoned());
    let duration_like = || {
        Param::new(
            "duration",
            Type::Union(vec![
                shared::temporal_type("Duration"),
                shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
            ]),
        )
    };
    let nullable_options = || Type::Union(vec![shared::object_shape_type(&[]), Type::Null]);

    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("ZonedDateTimeConstructor", "from"),
        vec![Param::new("iso", Type::String)],
        zoned(),
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("ZonedDateTimeConstructor", "compare"),
        vec![Param::new("a", zoned()), Param::new("b", zoned())],
        Type::Number,
    );
    for method in ["add", "subtract"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("ZonedDateTime", method),
            vec![receiver(), duration_like()],
            zoned(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("ZonedDateTime", method),
            vec![
                receiver(),
                Param::new("other", zoned()),
                Param::with_default("options", nullable_options(), DefaultValue::Null),
            ],
            shared::temporal_type("Duration"),
        );
    }
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("ZonedDateTime", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::temporal_type("ZonedDateTimeFields")),
        ],
        zoned(),
    );
    shared::declare_direct_fn(
        defs,
        "round",
        shared::prelude_key("ZonedDateTime", "round"),
        vec![
            receiver(),
            Param::new(
                "roundTo",
                Type::Union(vec![
                    Type::String,
                    shared::temporal_type("ZonedDateTimeRoundOptions"),
                ]),
            ),
        ],
        zoned(),
    );
    shared::declare_direct_fn(
        defs,
        "withTimeZone",
        shared::prelude_key("ZonedDateTime", "withTimeZone"),
        vec![receiver(), Param::new("timeZone", Type::String)],
        zoned(),
    );
    shared::declare_direct_fn(
        defs,
        "startOfDay",
        shared::prelude_key("ZonedDateTime", "startOfDay"),
        vec![receiver()],
        zoned(),
    );
    for method in ["toInstant", "toPlainDate", "toPlainTime", "toPlainDateTime"] {
        let ret = match method {
            "toInstant" => shared::temporal_type("Instant"),
            "toPlainDate" => shared::temporal_type("PlainDate"),
            "toPlainTime" => shared::temporal_type("PlainTime"),
            "toPlainDateTime" => shared::temporal_type("PlainDateTime"),
            _ => unreachable!(),
        };
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("ZonedDateTime", method),
            vec![receiver()],
            ret,
        );
    }
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("ZonedDateTime", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("ZonedDateTime", "equals"),
        vec![receiver(), Param::new("other", zoned())],
        Type::Boolean,
    );
    for field in [
        "epochMilliseconds",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "dayOfWeek",
        "dayOfYear",
        "weekOfYear",
        "yearOfWeek",
        "daysInWeek",
        "daysInMonth",
        "daysInYear",
        "monthsInYear",
        "millisecond",
        "microsecond",
        "nanosecond",
        "offsetNanoseconds",
        "hoursInDay",
    ] {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("ZonedDateTime", field),
            vec![receiver()],
            Type::Number,
        );
    }
    for field in ["timeZoneId", "offset", "monthCode"] {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("ZonedDateTime", field),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "inLeapYear",
        shared::prelude_key("ZonedDateTime", "inLeapYear"),
        vec![receiver()],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "epochNanoseconds",
        shared::prelude_key("ZonedDateTime", "epochNanoseconds"),
        vec![receiver()],
        Type::BigInt,
    );
}
