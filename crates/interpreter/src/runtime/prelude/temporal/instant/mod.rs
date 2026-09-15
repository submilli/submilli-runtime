//! `Temporal.Instant` operations.

mod install;

pub(super) use install::{declare, install};

use std::cmp::Ordering;
use std::str::FromStr;

use jiff::{RoundMode, Span, Timestamp, TimestampDifference, TimestampRound, Zoned};
use num_bigint::BigInt;

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<Timestamp> {
    Timestamp::from_str(input.trim()).map_err(|_| {
        format!(
            "Temporal.Instant.from: {input:?} is not a valid ISO 8601 instant (expected e.g. \"2024-03-09T15:30:45Z\")"
        )
    })
}

pub(super) fn from_epoch_milliseconds(ms: f64) -> Result<Timestamp> {
    if !ms.is_finite() {
        return Err("Temporal.Instant.fromEpochMilliseconds: expected a finite value".to_string());
    }
    let secs_f = (ms / 1_000.0).trunc();
    if secs_f < i64::MIN as f64 || secs_f > i64::MAX as f64 {
        return Err(range_error("fromEpochMilliseconds"));
    }
    let mut secs = secs_f as i64;
    let mut nanos = ((ms - secs as f64 * 1_000.0) * 1_000_000.0) as i32;
    if nanos < 0 {
        secs = secs
            .checked_sub(1)
            .ok_or_else(|| range_error("fromEpochMilliseconds"))?;
        nanos += 1_000_000_000;
    }
    Timestamp::new(secs, nanos).map_err(|_| range_error("fromEpochMilliseconds"))
}

pub(super) fn from_epoch_nanoseconds(nanos: &BigInt) -> Result<Timestamp> {
    let nanos: i128 = nanos
        .try_into()
        .map_err(|_| range_error("fromEpochNanoseconds"))?;
    Timestamp::from_nanosecond(nanos).map_err(|_| range_error("fromEpochNanoseconds"))
}

pub(super) fn compare(a: Timestamp, b: Timestamp) -> f64 {
    match a.cmp(&b) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

pub(super) fn to_string(ts: Timestamp) -> String {
    ts.to_string()
}

pub(super) fn equals(a: Timestamp, b: Timestamp) -> bool {
    a == b
}

pub(super) fn epoch_milliseconds(ts: Timestamp) -> f64 {
    super::shared::epoch_milliseconds(ts)
}

pub(super) fn epoch_nanoseconds(ts: Timestamp) -> BigInt {
    BigInt::from(i128::from(ts.as_second()) * 1_000_000_000 + i128::from(ts.subsec_nanosecond()))
}

pub(super) fn add(ts: Timestamp, span: Span) -> Result<Timestamp> {
    reject_calendar_units(&span, "add")?;
    ts.checked_add(span).map_err(|_| result_range_error("add"))
}

pub(super) fn subtract(ts: Timestamp, span: Span) -> Result<Timestamp> {
    reject_calendar_units(&span, "subtract")?;
    ts.checked_sub(span)
        .map_err(|_| result_range_error("subtract"))
}

pub(super) fn until(
    from: Timestamp,
    difference: TimestampDifference,
    options: &str,
) -> Result<Span> {
    from.until(difference).map_err(|_| {
        super::shared::temporal_error(
            "Instant.until",
            format!(
                "cannot compute the difference from {from} with {options}; use compatible largestUnit and smallestUnit values and a roundingIncrement valid for smallestUnit"
            ),
        )
    })
}

pub(super) fn since(
    from: Timestamp,
    difference: TimestampDifference,
    options: &str,
) -> Result<Span> {
    from.since(difference).map_err(|_| {
        super::shared::temporal_error(
            "Instant.since",
            format!(
                "cannot compute the difference from {from} with {options}; use compatible largestUnit and smallestUnit values and a roundingIncrement valid for smallestUnit"
            ),
        )
    })
}

pub(super) fn round(
    ts: Timestamp,
    options: TimestampRound,
    description: &str,
) -> Result<Timestamp> {
    ts.round(options).map_err(|_| {
        super::shared::temporal_error(
            "Instant.round",
            format!(
                "cannot round {ts} with {description}; choose a positive roundingIncrement that divides evenly into the next larger unit"
            ),
        )
    })
}

pub(super) fn round_mode(mode: &str) -> Result<RoundMode> {
    Ok(match mode {
        // Temporal rounds along its representable timeline, whose origin is
        // the minimum Instant rather than the Unix epoch.
        "trunc" => RoundMode::Floor,
        "expand" => RoundMode::Ceil,
        "halfTrunc" => RoundMode::HalfFloor,
        "halfExpand" => RoundMode::HalfCeil,
        "ceil" => RoundMode::Ceil,
        "floor" => RoundMode::Floor,
        "halfCeil" => RoundMode::HalfCeil,
        "halfFloor" => RoundMode::HalfFloor,
        "halfEven" => RoundMode::HalfEven,
        other => {
            return Err(format!(
                "Temporal.Instant.round: unknown roundingMode {other:?}; use ceil, floor, expand, trunc, halfCeil, halfFloor, halfExpand, halfTrunc, or halfEven"
            ));
        }
    })
}

pub(super) fn to_zoned_date_time_iso(ts: Timestamp, time_zone: &str) -> Result<(Zoned, String)> {
    let (tz, id) =
        super::zoned_date_time::resolve_time_zone(time_zone, "Instant.toZonedDateTimeISO")?;
    Ok((ts.to_zoned(tz), id))
}

fn reject_calendar_units(span: &Span, op: &str) -> Result<()> {
    if span.get_years() != 0
        || span.get_months() != 0
        || span.get_weeks() != 0
        || span.get_days() != 0
    {
        return Err(format!(
            "Temporal.Instant.{op}: a Duration with calendar units (years/months/weeks/days) \
             can't be applied to an Instant — an Instant has no time zone, so calendar lengths \
             are undefined. Use only time units (hours and smaller), or project onto a time zone \
             with toZonedDateTimeISO(timeZone) for calendar-aware arithmetic."
        ));
    }
    Ok(())
}

fn range_error(op: &str) -> String {
    format!("Temporal.Instant.{op}: the instant is outside the representable range")
}

fn result_range_error(op: &str) -> String {
    format!("Temporal.Instant.{op}: the result is outside the representable instant range")
}
