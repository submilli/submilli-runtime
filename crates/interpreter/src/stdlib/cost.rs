//! `submilli:cost` — packages report what their own calls cost.
//!
//! A package that calls a paid API reports the amount, so the run's cost
//! includes it beside the model calls the host priced itself. The host fills in
//! which package reported, from the running frame; a package cannot report
//! under another name. Packages only: the main module is refused ahead of
//! policy.
//!
//! A report is not a durable effect. A resumed attempt re-runs the program, and
//! the program reports again, so each attempt's store lists the whole run.

use std::collections::BTreeMap;

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::host::{
    abi_arg, read_boxed_number, read_string_arg_units, register_host_fn_async, type_error,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::undefined::is_undefined;
use crate::runtime::prelude::vtable::read_object_entries;
use crate::runtime::{MAX_PACKAGE_COST_UNITS, PackageReport, StoreData, fuel};
use crate::stdlib::shared::{check_and_approve_call, running_package};
use crate::{ObjectField, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:cost";

/// The capability `report` requires.
pub const CAPABILITY: &str = "cost.report";

/// The longest label a report keeps, in UTF-16 units.
const MAX_LABEL: usize = 256;

/// The longest usage unit name, in UTF-16 units.
const MAX_UNIT: usize = 64;

/// Pico-dollars in one dollar.
const PICO_PER_USD: f64 = 1e12;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    let usage = Type::Object {
        fields: BTreeMap::new(),
        index: Some(crate::IndexSignature {
            string_keyof_includes_number: false,
            key: Box::new(Type::String),
            value: Box::new(Type::Number),
            readonly: false,
        }),
        more_indexes: Vec::new(),
    };
    let entry = Type::Object {
        fields: BTreeMap::from([
            ("usd".to_string(), ObjectField::optional(Type::Number)),
            ("usage".to_string(), ObjectField::optional(usage)),
            ("label".to_string(), ObjectField::optional(Type::String)),
        ]),
        index: None,
        more_indexes: Vec::new(),
    };
    defs.values.insert(
        "report".to_string(),
        ValueSymbol {
            name: "report".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "report"),
            declaration_span: Span::at(crate::FileId::COST),
            kind: ValueKind::Function {
                generics: Default::default(),
                params: vec![Param::new("entry", entry)],
                ret: Type::Void,
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::COST,
                    "/**\n * Report what one of this package's calls cost, so the run's cost includes it. Packages only; the main module is always refused. The host records which package reported.\n * @param entry `usd` is the amount in US dollars. `usage` holds the counts behind it, keyed by unit, such as `input_tokens`. `label` names what was called, such as a model id. Give `usd`, `usage`, or both; a report without `usd` is listed as unpriced.\n * @capability cost.report\n */",
                ),
            },
        },
    );
    defs
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    // A required object parameter lowers to a non-null shaped object.
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape),
    ));
    let ty = FuncType::new(&engine, [object], []);
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "report"),
        ty,
        |caller, params, _results| {
            Box::pin(async move {
                let entry = *abi_arg(params, 0)?;
                check_and_approve_call(&mut *caller, CAPABILITY, serde_json::json!({})).await?;
                let report = read_report(caller, &entry)?;
                // The gate already refused an unknown principal; this cannot fail
                // differently now, but it must fail closed if it does.
                let package =
                    running_package(caller).map_err(|error| error.into_denial(CAPABILITY))?;
                caller.data_mut().note_package_cost(&package, report);
                Ok(())
            })
        },
    )
}

/// Read and check the guest's entry. Every refusal is a catchable `TypeError`.
fn read_report(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    entry: &Val,
) -> wasmtime::Result<PackageReport> {
    let mut report = PackageReport {
        label: None,
        pico_usd: None,
        usage: Vec::new(),
    };
    for (name, value) in read_object_entries(caller, entry, "cost.report entry")? {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        if is_undefined(caller, &value)? {
            continue;
        }
        match String::from_utf16_lossy(&name).as_str() {
            "usd" => {
                let usd = read_boxed_number(caller, &value, "cost.report usd")?;
                report.pico_usd = Some(pico_usd(usd)?);
            }
            "label" => {
                let units = read_string_arg_units(caller, &value, "cost.report")?;
                fuel::charge(&mut *caller, fuel::SCAN, units.len() as u64)?;
                if units.len() > MAX_LABEL {
                    return Err(type_error(format!(
                        "cost.report: label is longer than {MAX_LABEL} characters"
                    )));
                }
                report.label = Some(String::from_utf16_lossy(&units));
            }
            "usage" => report.usage = read_usage(caller, &value)?,
            _ => {}
        }
    }
    if report.pico_usd.is_none() && report.usage.is_empty() {
        return Err(type_error(
            "cost.report: give usd, usage, or both; an empty report says nothing",
        ));
    }
    Ok(report)
}

fn read_usage(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Vec<(String, f64)>> {
    let entries = read_object_entries(caller, value, "cost.report usage")?;
    if entries.len() > MAX_PACKAGE_COST_UNITS {
        return Err(type_error(format!(
            "cost.report: usage has more than {MAX_PACKAGE_COST_UNITS} units"
        )));
    }
    let mut usage = Vec::with_capacity(entries.len());
    for (unit, count) in entries {
        fuel::charge(&mut *caller, fuel::SCAN, unit.len() as u64)?;
        if unit.is_empty() || unit.len() > MAX_UNIT {
            return Err(type_error(format!(
                "cost.report: a usage unit must be 1 to {MAX_UNIT} characters"
            )));
        }
        // Lossy decoding would fold two distinct lone surrogates into one unit.
        let Ok(unit) = String::from_utf16(&unit) else {
            return Err(type_error(
                "cost.report: a usage unit must be valid Unicode text",
            ));
        };
        let count = read_boxed_number(caller, &count, "cost.report usage")?;
        if !count.is_finite() || count < 0.0 {
            return Err(type_error(format!(
                "cost.report: usage '{unit}' must be a finite count of at least 0, not {count}"
            )));
        }
        usage.push((unit, count));
    }
    Ok(usage)
}

/// Dollars to whole pico-dollars, rounded to the nearest.
fn pico_usd(usd: f64) -> wasmtime::Result<u128> {
    if !usd.is_finite() || usd < 0.0 {
        return Err(type_error(format!(
            "cost.report: usd must be a finite amount of at least 0, not {usd}"
        )));
    }
    // `as` saturates, and f64 holds every amount a report could plausibly carry.
    Ok((usd * PICO_PER_USD).round() as u128)
}

#[cfg(test)]
mod tests;
