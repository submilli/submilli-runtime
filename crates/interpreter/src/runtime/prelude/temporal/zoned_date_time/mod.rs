//! `Temporal.ZonedDateTime` operations.

mod install;
mod zone_cache;
mod zone_ids;

pub(super) fn prepare_zones() -> wasmtime::Result<()> {
    zone_cache::prepare()
}

pub(super) use install::{declare, install};

use std::cmp::Ordering;

use jiff::{
    RoundMode, Span, Timestamp, Unit, Zoned, ZonedRound, civil,
    fmt::temporal::{DateTimeParser, Pieces, PiecesOffset, TimeZoneAnnotationKind},
    tz::{Offset, OffsetConflict, TimeZone},
};
use wasmtime::Caller;

use crate::runtime::StoreData;
use crate::runtime::fuel;

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> wasmtime::Result<(Zoned, String)> {
    let input = input.trim();
    let invalid = || {
        crate::runtime::host::range_error(format!(
            "Temporal.ZonedDateTime.from: {input:?} is not a valid ISO 8601 zoned date-time (expected e.g. \"2024-03-09T15:30:45-05:00[America/New_York]\")"
        ))
    };
    let pieces = Pieces::parse(input).map_err(|_| invalid())?;
    let annotation = pieces.time_zone_annotation().ok_or_else(invalid)?;
    let zone = match annotation.kind() {
        TimeZoneAnnotationKind::Named(name) => {
            zone_cache::named(name.as_str())?.ok_or_else(invalid)?
        }
        TimeZoneAnnotationKind::Offset(offset) => TimeZone::fixed(*offset),
        _ => return Err(invalid()),
    };
    let datetime = civil::DateTime::from_parts(
        pieces.date(),
        pieces.time().unwrap_or(civil::Time::midnight()),
    );
    let ambiguous = match pieces.offset() {
        None => zone.into_ambiguous_zoned(datetime),
        Some(PiecesOffset::Zulu) => OffsetConflict::AlwaysOffset
            .resolve(datetime, Offset::UTC, zone)
            .map_err(|_| invalid())?,
        Some(PiecesOffset::Numeric(offset)) => {
            let exact = has_offset_seconds(input);
            OffsetConflict::Reject
                .resolve_with(datetime, offset.offset(), zone, |parsed, candidate| {
                    parsed == candidate
                        || (!exact
                            && candidate.seconds() % 60 != 0
                            && candidate
                                .round(Unit::Minute)
                                .is_ok_and(|rounded| parsed == rounded))
                })
                .map_err(|_| invalid())?
        }
        Some(_) => return Err(invalid()),
    };
    let zoned = ambiguous.compatible().map_err(|_| invalid())?;
    let tz_id = zoned
        .time_zone()
        .iana_name()
        .map_or_else(|| format_offset(zoned.offset().seconds()), str::to_string);
    Ok((zoned, tz_id))
}

fn has_offset_seconds(input: &str) -> bool {
    let head = input.split('[').next().unwrap_or(input);
    let time = head
        .split_once(['T', 't', ' '])
        .map_or(head, |(_, time)| time);
    let offset = time
        .rfind(['+', '-'])
        .and_then(|index| time.get(index + 1..))
        .unwrap_or("");
    offset.bytes().filter(|byte| *byte == b':').count() >= 2
        || (!offset.contains(':') && offset.len() >= 6)
}

pub(super) fn compare_timestamps(a: Timestamp, b: Timestamp) -> f64 {
    match a.cmp(&b) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

pub(super) fn offset(zoned: &Zoned, _: &str) -> String {
    format_offset(zoned.offset().seconds())
}

fn format_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let absolute = seconds.unsigned_abs();
    let hours = absolute / 3_600;
    let minutes = absolute % 3_600 / 60;
    let seconds = absolute % 60;
    if seconds == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

pub(super) fn month_code(zoned: &Zoned, _: &str) -> String {
    format!("M{:02}", zoned.month())
}

pub(super) fn add(zoned: &Zoned, span: Span) -> Result<Zoned> {
    zoned.checked_add(span).map_err(|_| {
        super::shared::temporal_error(
            "ZonedDateTime.add",
            format!(
                "adding {span} to {zoned} produces a date outside the supported range; use a smaller duration"
            ),
        )
    })
}

pub(super) fn subtract(zoned: &Zoned, span: Span) -> Result<Zoned> {
    zoned.checked_sub(span).map_err(|_| {
        super::shared::temporal_error(
            "ZonedDateTime.subtract",
            format!(
                "subtracting {span} from {zoned} produces a date outside the supported range; use a smaller duration"
            ),
        )
    })
}

/// Looks a zone up in the finite pre-parsed database. Charged as one `TZ`.
pub(super) fn resolve_time_zone(
    caller: &mut Caller<'_, StoreData>,
    time_zone: &str,
    operation: &str,
) -> wasmtime::Result<(TimeZone, String)> {
    // A fuel trap passes through as it is; an unknown zone is the program's
    // `RangeError`.
    fuel::charge_host_fuel(&mut *caller, fuel::TZ)?;
    let unknown = || crate::runtime::host::range_error(unknown_zone(operation, time_zone));
    if is_offset_identifier(time_zone) {
        let offset = parse_fixed_offset(time_zone).map_err(|_| unknown())?;
        return Ok((TimeZone::fixed(offset), format_offset(offset.seconds())));
    }
    let tz = zone_cache::named(time_zone)?.ok_or_else(unknown)?;
    let id = tz.iana_name().ok_or_else(unknown)?.to_string();
    Ok((tz, id))
}

pub(super) fn system_zone(id: &str) -> wasmtime::Result<TimeZone> {
    Ok(zone_cache::named(id)?.unwrap_or(TimeZone::UTC))
}

pub(super) fn time_zone_ids_equal(
    caller: &mut Caller<'_, StoreData>,
    a: &str,
    b: &str,
) -> wasmtime::Result<bool> {
    if a.eq_ignore_ascii_case(b) {
        return Ok(true);
    }
    let a_is_offset = is_offset_identifier(a);
    let b_is_offset = is_offset_identifier(b);
    if a_is_offset || b_is_offset {
        if a_is_offset != b_is_offset {
            return Ok(false);
        }
        let (Ok(a), Ok(b)) = (parse_fixed_offset(a), parse_fixed_offset(b)) else {
            return Ok(false);
        };
        return Ok(a == b);
    }
    fuel::charge_host_fuel(&mut *caller, 2 * fuel::TZ)?;
    Ok(zone_ids::primary(a).eq_ignore_ascii_case(zone_ids::primary(b)))
}

fn parse_fixed_offset(input: &str) -> std::result::Result<Offset, jiff::Error> {
    DateTimeParser::new()
        .parse_time_zone(input)
        .and_then(|time_zone| time_zone.to_fixed_offset())
}

fn is_offset_identifier(time_zone: &str) -> bool {
    matches!(time_zone.as_bytes().first(), Some(b'+' | b'-'))
}

pub(super) fn with_time_zone(
    caller: &mut Caller<'_, StoreData>,
    zoned: &Zoned,
    time_zone: &str,
) -> wasmtime::Result<(Zoned, String)> {
    let (tz, id) = resolve_time_zone(caller, time_zone, "ZonedDateTime.withTimeZone")?;
    Ok((zoned.timestamp().to_zoned(tz), id))
}

pub(super) fn with_fields(zoned: &Zoned, date: civil::Date, time: civil::Time) -> Result<Zoned> {
    zoned
        .with()
        .date(date)
        .time(time)
        .build()
        .map_err(|_| {
            super::shared::temporal_error(
                "ZonedDateTime.with",
                format!(
                    "{date} at {time} cannot be represented in time zone {}; use fields within the supported date range",
                    zoned.time_zone().iana_name().unwrap_or("the current zone")
                ),
            )
        })
}

pub(super) fn round(
    zoned: &Zoned,
    smallest: Unit,
    mode: Option<RoundMode>,
    increment: Option<i64>,
) -> Result<Zoned> {
    if matches!(smallest, Unit::Week | Unit::Month | Unit::Year) {
        return Err(super::shared::temporal_error(
            "ZonedDateTime.round",
            format!(
                "smallestUnit='{}' is a calendar unit; choose a smallestUnit of day or smaller",
                super::shared::temporal_unit_name(smallest)
            ),
        ));
    }
    let mut options = ZonedRound::new().smallest(smallest);
    if let Some(mode) = mode {
        options = options.mode(mode);
    }
    if let Some(increment) = increment {
        options = options.increment(increment);
    }
    zoned
        .round(options)
        .map_err(|_| {
            let description = super::shared::round_options_description(
                Some(smallest),
                None,
                mode,
                increment,
            );
            super::shared::temporal_error(
                "ZonedDateTime.round",
                format!(
                    "cannot round {zoned} with {description}; use a positive roundingIncrement that divides evenly into the next larger unit"
                ),
            )
        })
}

pub(super) fn start_of_day(zoned: &Zoned) -> Result<Zoned> {
    zoned.start_of_day().map_err(|_| {
        super::shared::temporal_error(
            "ZonedDateTime.startOfDay",
            format!(
                "the start of {} is outside the supported range",
                zoned.date()
            ),
        )
    })
}

pub(super) fn hours_in_day(zoned: &Zoned) -> Result<f64> {
    let start = start_of_day(zoned)?;
    let tomorrow = start
        .tomorrow()
        .and_then(|tomorrow| tomorrow.start_of_day())
        .map_err(|_| {
            super::shared::temporal_error(
                "ZonedDateTime.hoursInDay",
                format!(
                    "the day after {} is outside the supported range",
                    zoned.date()
                ),
            )
        })?;
    let elapsed = tomorrow.timestamp().as_nanosecond() - start.timestamp().as_nanosecond();
    Ok(elapsed as f64 / 3_600_000_000_000.0)
}

pub(super) fn unknown_zone(operation: &str, time_zone: &str) -> String {
    format!(
        "Temporal.{operation}: time zone {time_zone:?} not found (expected an IANA name like \"America/New_York\", \"UTC\", or a fixed offset like \"-05:00\")"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_zone_parser_preserves_annotation_and_offset_rules() {
        for input in [
            "2024-01-01T12:00Z[UTC]",
            "2024-01-01T12:00+00:00[UTC]",
            "2024-01-01T12:00[UTC]",
            "2024-01-01[UTC]",
            "2024-03-10T02:30[America/New_York]",
            "2024-11-03T01:30[America/New_York]",
            "2024-01-01T12:00+00:00[+00:00]",
            "2024-01-01T12:00Z[UTC][u-ca=iso8601]",
            "2024-01-01T12:00Z[UTC][u-ca=hebrew]",
            "2024-01-01T12:00Z[UTC][!u-ca=hebrew]",
            "2024-01-01T12:00Z[UTC][!unknown=value]",
            "2024-01-01T12:00-05:00[UTC]",
        ] {
            let previous = input.parse::<Zoned>();
            let current = parse(input);
            assert_eq!(
                previous.is_ok(),
                current.is_ok(),
                "{input}: {previous:?} / {current:?}"
            );
            if let (Ok(previous), Ok((current, _))) = (previous, current) {
                assert_eq!(previous.timestamp(), current.timestamp(), "{input}");
                assert_eq!(previous.offset(), current.offset(), "{input}");
            }
        }
    }
}
