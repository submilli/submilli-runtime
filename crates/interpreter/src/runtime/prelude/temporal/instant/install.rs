use jiff::{RoundMode, TimestampRound};
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use super::super::shared::{
    instant_from_val, make_duration, make_instant, make_zoned_date_time, object_diff_options,
    object_i64_field, object_string_field, prelude_key, read_duration_like, read_f64,
    unit_from_str,
};
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn, write_submilli_string_struct};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    install_direct(
        linker,
        &types.engine,
        &types.intr,
        &types.object,
        &types.string,
    )
}

fn install_direct(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    obj: &ValType,
    string: &ValType,
) -> wasmtime::Result<()> {
    let host = crate::runtime::prelude::MODULE_NAME;
    let obj_result = [obj.clone()];
    let bigint = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.bigint.clone()),
    ));

    register_host_fn(
        linker,
        host,
        prelude_key("InstantConstructor", "from"),
        FuncType::new(engine, [string.clone()], obj_result.clone()),
        true,
        |caller, params, results| {
            let s = read_string_arg(caller, &params[0], "Temporal.Instant.from")?;
            let ts = super::parse(&s).map_err(crate::runtime::host::range_error)?;
            results[0] = make_instant(caller, ts)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        host,
        prelude_key("InstantConstructor", "fromEpochMilliseconds"),
        FuncType::new(engine, [ValType::F64], obj_result.clone()),
        true,
        |caller, params, results| {
            let ms = read_f64(&params[0], "Instant.fromEpochMilliseconds")?;
            let ts =
                super::from_epoch_milliseconds(ms).map_err(crate::runtime::host::range_error)?;
            results[0] = make_instant(caller, ts)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        host,
        prelude_key("InstantConstructor", "fromEpochNanoseconds"),
        FuncType::new(engine, [bigint.clone()], obj_result.clone()),
        true,
        |caller, params, results| {
            let (sign, limbs) = crate::runtime::prelude::bigint::ops::read_bigint_struct(
                caller,
                &params[0],
                "Temporal.Instant.fromEpochNanoseconds",
            )?;
            let big = crate::runtime::prelude::bigint::ops::limbs_to_bigint(sign, &limbs);
            let ts =
                super::from_epoch_nanoseconds(&big).map_err(crate::runtime::host::range_error)?;
            results[0] = make_instant(caller, ts)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        host,
        prelude_key("InstantConstructor", "compare"),
        FuncType::new(engine, [obj.clone(), obj.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            let a = instant_from_val(caller, &params[0], "Instant.compare")?;
            let b = instant_from_val(caller, &params[1], "Instant.compare")?;
            results[0] = Val::F64(super::compare(a, b).to_bits());
            Ok(())
        },
    )?;

    for method in ["toString", "toJSON"] {
        register_host_fn(
            linker,
            host,
            prelude_key("Instant", method),
            FuncType::new(engine, [obj.clone()], [string.clone()]),
            true,
            move |caller, params, results| {
                let ts = instant_from_val(caller, &params[0], "Instant.toString")?;
                let text = super::to_string(ts);
                let value = write_submilli_string_struct(caller, &text)?;
                results[0] = Val::AnyRef(Some(value.to_anyref()));
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        host,
        prelude_key("Instant", "equals"),
        FuncType::new(engine, [obj.clone(), obj.clone()], [ValType::I32]),
        true,
        |caller, params, results| {
            let a = instant_from_val(caller, &params[0], "Instant.equals")?;
            let b = instant_from_val(caller, &params[1], "Instant.equals")?;
            results[0] = Val::I32(super::equals(a, b) as i32);
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        host,
        prelude_key("Instant", "epochMilliseconds"),
        FuncType::new(engine, [obj.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            let ts = instant_from_val(caller, &params[0], "Instant.epochMilliseconds")?;
            results[0] = Val::F64(super::epoch_milliseconds(ts).to_bits());
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        host,
        prelude_key("Instant", "epochNanoseconds"),
        FuncType::new(engine, [obj.clone()], [bigint]),
        true,
        |caller, params, results| {
            let ts = instant_from_val(caller, &params[0], "Instant.epochNanoseconds")?;
            results[0] = crate::runtime::prelude::bigint::ops::make_bigint_struct(
                caller,
                super::epoch_nanoseconds(ts),
            )?;
            Ok(())
        },
    )?;

    let intr_for_duration = intr.clone();
    for (method, add) in [("add", true), ("subtract", false)] {
        let intr = intr_for_duration.clone();
        register_host_fn(
            linker,
            host,
            prelude_key("Instant", method),
            FuncType::new(engine, [obj.clone(), obj.clone()], obj_result.clone()),
            true,
            move |caller, params, results| {
                let ts = instant_from_val(caller, &params[0], "Instant arithmetic")?;
                let span = read_duration_like(caller, &params[1], &intr, "Instant arithmetic")?;
                let out = if add {
                    super::add(ts, span)
                } else {
                    super::subtract(ts, span)
                }
                .map_err(crate::runtime::host::range_error)?;
                results[0] = make_instant(caller, out)?;
                Ok(())
            },
        )?;
    }

    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    for (method, until) in [("until", true), ("since", false)] {
        register_host_fn(
            linker,
            host,
            prelude_key("Instant", method),
            FuncType::new(
                engine,
                [obj.clone(), obj.clone(), options.clone()],
                obj_result.clone(),
            ),
            true,
            move |caller, params, results| {
                let a = instant_from_val(caller, &params[0], "Instant difference")?;
                let b = instant_from_val(caller, &params[1], "Instant difference")?;
                let label = if until {
                    "Instant.until"
                } else {
                    "Instant.since"
                };
                let opts = object_diff_options(caller, &params[2], label)?;
                let description = opts.description();
                let span = if until {
                    super::until(a, opts.timestamp(b), &description)
                } else {
                    super::since(a, opts.timestamp(b), &description)
                }
                .map_err(crate::runtime::host::range_error)?;
                results[0] = make_duration(caller, &span)?;
                Ok(())
            },
        )?;
    }

    let round_ty = FuncType::new(engine, [obj.clone(), obj.clone()], obj_result.clone());
    register_host_fn(
        linker,
        host,
        prelude_key("Instant", "round"),
        round_ty,
        true,
        instant_round_direct,
    )?;
    register_host_fn(
        linker,
        host,
        prelude_key("Instant", "toZonedDateTimeISO"),
        FuncType::new(engine, [obj.clone(), string.clone()], obj_result.clone()),
        true,
        |caller, params, results| {
            let ts = instant_from_val(caller, &params[0], "Instant.toZonedDateTimeISO")?;
            let tz_id = read_string_arg(caller, &params[1], "Temporal.Instant.toZonedDateTimeISO")?;
            let (zoned, canonical_id) = super::to_zoned_date_time_iso(caller, ts, &tz_id)?;
            results[0] = make_zoned_date_time(caller, &zoned, &canonical_id)?;
            Ok(())
        },
    )?;

    Ok(())
}

fn instant_round_direct(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let ts = instant_from_val(caller, &params[0], "Instant.round")?;
    let string_type = caller
        .data()
        .host_abi
        .as_ref()
        .map(|abi| abi.string_type.clone())
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
    let (unit, mode, increment) =
        if crate::runtime::prelude::collection::is_a(caller, &params[1], &string_type)? {
            (
                read_string_arg(caller, &params[1], "Temporal.Instant.round")?,
                None,
                None,
            )
        } else {
            let unit = object_string_field(caller, &params[1], "smallestUnit", "Instant.round")?
                .ok_or_else(|| {
                    wasmtime::Error::msg("Temporal.Instant.round: smallestUnit is required")
                })?;
            let mode = object_string_field(caller, &params[1], "roundingMode", "Instant.round")?;
            let increment =
                object_i64_field(caller, &params[1], "roundingIncrement", "Instant.round")?;
            (unit, mode, increment)
        };
    let description = format!(
        "smallestUnit=\"{unit}\", roundingMode={}, roundingIncrement={}",
        shared::string_option_description(mode.as_deref()),
        shared::i64_option_description(increment),
    );
    let unit = unit_from_str(&unit, "Instant.round")?;
    let mut round = TimestampRound::new()
        .smallest(unit)
        .mode(RoundMode::HalfCeil);
    if let Some(mode) = mode {
        round = round.mode(super::round_mode(&mode).map_err(crate::runtime::host::range_error)?);
    }
    if let Some(increment) = increment {
        round = round.increment(increment);
    }
    let out = super::round(ts, round, &description).map_err(crate::runtime::host::range_error)?;
    results[0] = make_instant(caller, out)?;
    Ok(())
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let instant = || shared::temporal_type("Instant");
    let receiver = || Param::new("receiver", instant());
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
        shared::prelude_key("InstantConstructor", "from"),
        vec![Param::new("iso", Type::String)],
        instant(),
    );
    shared::declare_direct_fn(
        defs,
        "fromEpochMilliseconds",
        shared::prelude_key("InstantConstructor", "fromEpochMilliseconds"),
        vec![Param::new("ms", Type::Number)],
        instant(),
    );
    shared::declare_direct_fn(
        defs,
        "fromEpochNanoseconds",
        shared::prelude_key("InstantConstructor", "fromEpochNanoseconds"),
        vec![Param::new("ns", Type::BigInt)],
        instant(),
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("InstantConstructor", "compare"),
        vec![Param::new("a", instant()), Param::new("b", instant())],
        Type::Number,
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Instant", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("Instant", "equals"),
        vec![receiver(), Param::new("other", instant())],
        Type::Boolean,
    );
    for (getter, ret) in [
        ("epochMilliseconds", Type::Number),
        ("epochNanoseconds", Type::BigInt),
    ] {
        shared::declare_direct_fn(
            defs,
            getter,
            shared::prelude_key("Instant", getter),
            vec![receiver()],
            ret,
        );
    }
    for method in ["add", "subtract"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Instant", method),
            vec![receiver(), duration_like()],
            instant(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Instant", method),
            vec![
                receiver(),
                Param::new("other", instant()),
                Param::with_default("options", nullable_options(), DefaultValue::Null),
            ],
            shared::temporal_type("Duration"),
        );
    }
    shared::declare_direct_fn(
        defs,
        "round",
        shared::prelude_key("Instant", "round"),
        vec![
            receiver(),
            Param::new(
                "roundTo",
                Type::Union(vec![
                    Type::String,
                    shared::temporal_type("InstantRoundOptions"),
                ]),
            ),
        ],
        instant(),
    );
    shared::declare_direct_fn(
        defs,
        "toZonedDateTimeISO",
        shared::prelude_key("Instant", "toZonedDateTimeISO"),
        vec![receiver(), Param::new("tz", Type::String)],
        shared::temporal_type("ZonedDateTime"),
    );
}
