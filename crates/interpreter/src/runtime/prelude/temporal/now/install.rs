use std::collections::BTreeMap;

use wasmtime::{FuncType, Linker, Val};

use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn, write_submilli_string_struct};
use crate::runtime::prelude::MODULE_NAME;
use crate::{
    DefaultValue, NamespaceSymbol, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol,
};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "instant"),
        FuncType::new(&types.engine, [], [types.object.clone()]),
        false,
        |caller, _params, results| {
            results[0] = shared::make_instant(caller, super::instant())?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "timeZoneId"),
        FuncType::new(&types.engine, [], [types.string.clone()]),
        false,
        |caller, _params, results| {
            let id = super::time_zone_id();
            let value = write_submilli_string_struct(caller, &id)?;
            results[0] = Val::AnyRef(Some(value.to_anyref()));
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "zonedDateTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        false,
        |caller, params, results| {
            let time_zone = match &params[0] {
                Val::AnyRef(None) => None,
                _ => Some(read_string_arg(
                    caller,
                    &params[0],
                    "Temporal.Now.zonedDateTimeISO",
                )?),
            };
            let (zoned, id) = super::zoned_date_time_iso(caller, time_zone.as_deref())?;
            results[0] = shared::make_zoned_date_time(caller, &zoned, &id)?;
            Ok(())
        },
    )?;

    // `Temporal.Now.zonedDateTime` — declared alias of `zonedDateTimeISO` (our
    // subset has no calendar argument), same body under its own dispatch key.
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "zonedDateTime"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        false,
        |caller, params, results| {
            let time_zone = match &params[0] {
                Val::AnyRef(None) => None,
                _ => Some(read_string_arg(
                    caller,
                    &params[0],
                    "Temporal.Now.zonedDateTime",
                )?),
            };
            let (zoned, id) = super::zoned_date_time_iso(caller, time_zone.as_deref())?;
            results[0] = shared::make_zoned_date_time(caller, &zoned, &id)?;
            Ok(())
        },
    )?;

    // `Temporal.Now.plain{Date,Time,DateTime}ISO(tz?)` — the current zoned
    // moment projected onto the calendar-naive Plain types.
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainDateISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        false,
        |caller, params, results| {
            let z = now_zoned(caller, &params[0], "Temporal.Now.plainDateISO")?;
            let d = z.date();
            results[0] = shared::make_plain_date(
                caller,
                i32::from(d.year()),
                i32::from(d.month()),
                i32::from(d.day()),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        false,
        |caller, params, results| {
            let z = now_zoned(caller, &params[0], "Temporal.Now.plainTimeISO")?;
            let t = z.time();
            results[0] = shared::make_plain_time(
                caller,
                i32::from(t.hour()),
                i32::from(t.minute()),
                i32::from(t.second()),
                t.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainDateTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        false,
        |caller, params, results| {
            let z = now_zoned(caller, &params[0], "Temporal.Now.plainDateTimeISO")?;
            let dt = z.datetime();
            results[0] = shared::make_plain_date_time(
                caller,
                i32::from(dt.year()),
                i32::from(dt.month()),
                i32::from(dt.day()),
                i32::from(dt.hour()),
                i32::from(dt.minute()),
                i32::from(dt.second()),
                dt.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// The current moment in the `tz` argument's zone (system zone when null).
fn now_zoned(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    tz: &Val,
    label: &'static str,
) -> wasmtime::Result<jiff::Zoned> {
    let tz = match tz {
        Val::AnyRef(None) => None,
        _ => Some(read_string_arg(caller, tz, label)?),
    };
    let (zoned, _id) = super::zoned_date_time_iso(caller, tz.as_deref())?;
    Ok(zoned)
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let temporal_prefix = crate::mangle::prelude("Temporal");
    let now_prefix = crate::mangle::extend(&temporal_prefix, "Now");
    let function = |name: &str, ret: Type| ValueSymbol {
        name: name.to_string(),
        mangled_name: shared::prelude_key("Now", name),
        declaration_span: Span::at(crate::FileId::TEMPORAL),
        kind: ValueKind::Function {
            generics: Vec::new(),
            params: Vec::new(),
            ret,
            type_predicate: None,
            doc: None,
        },
    };
    let now = NamespaceSymbol {
        name: "Now".to_string(),
        mangled_prefix: now_prefix,
        declaration_span: Span::at(crate::FileId::TEMPORAL),
        values: BTreeMap::from([
            (
                "instant".to_string(),
                function("instant", shared::temporal_type("Instant")),
            ),
            (
                "timeZoneId".to_string(),
                function("timeZoneId", Type::String),
            ),
            (
                "zonedDateTimeISO".to_string(),
                ValueSymbol {
                    name: "zonedDateTimeISO".to_string(),
                    mangled_name: shared::prelude_key("Now", "zonedDateTimeISO"),
                    declaration_span: Span::at(crate::FileId::TEMPORAL),
                    kind: ValueKind::Function {
                        generics: Vec::new(),
                        params: vec![Param::with_default(
                            "timeZone",
                            Type::Union(vec![Type::String, Type::Null]),
                            DefaultValue::Null,
                        )],
                        ret: shared::temporal_type("ZonedDateTime"),
                        type_predicate: None,
                        doc: None,
                    },
                },
            ),
        ]),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    };
    defs.namespaces
        .entry("Temporal".to_string())
        .or_insert_with(|| NamespaceSymbol {
            name: "Temporal".to_string(),
            mangled_prefix: temporal_prefix,
            declaration_span: Span::at(crate::FileId::TEMPORAL),
            values: BTreeMap::new(),
            types: BTreeMap::new(),
            namespaces: BTreeMap::new(),
            doc: None,
        })
        .namespaces
        .insert("Now".to_string(), now);
}
