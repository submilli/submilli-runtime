//! `Temporal.ZonedDateTime` operations.

mod install;

pub(super) use install::{declare, install};

use std::cmp::Ordering;
use std::str::FromStr;

use jiff::{
    RoundMode, Span, Timestamp, Unit, Zoned, ZonedRound, civil,
    fmt::temporal::DateTimeParser,
    tz::{Offset, TimeZone},
};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<(Zoned, String)> {
    let input = input.trim();
    let zoned = Zoned::from_str(input).map_err(|_| {
        format!(
            "Temporal.ZonedDateTime.from: {input:?} is not a valid ISO 8601 zoned date-time (expected e.g. \"2024-03-09T15:30:45-05:00[America/New_York]\")"
        )
    })?;
    let tz_id = zoned
        .time_zone()
        .iana_name()
        .map_or_else(|| format_offset(zoned.offset().seconds()), str::to_string);
    Ok((zoned, tz_id))
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

pub(super) fn resolve_time_zone(time_zone: &str, operation: &str) -> Result<(TimeZone, String)> {
    if is_offset_identifier(time_zone) {
        let offset =
            parse_fixed_offset(time_zone).map_err(|_| unknown_zone(operation, time_zone))?;
        return Ok((TimeZone::fixed(offset), format_offset(offset.seconds())));
    }
    let tz = TimeZone::get(time_zone).map_err(|_| unknown_zone(operation, time_zone))?;
    let id = tz
        .iana_name()
        .ok_or_else(|| unknown_zone(operation, time_zone))?
        .to_string();
    Ok((tz, id))
}

pub(super) fn time_zone_ids_equal(a: &str, b: &str) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    let a_is_offset = is_offset_identifier(a);
    let b_is_offset = is_offset_identifier(b);
    if a_is_offset || b_is_offset {
        if a_is_offset != b_is_offset {
            return false;
        }
        let (Ok(a), Ok(b)) = (parse_fixed_offset(a), parse_fixed_offset(b)) else {
            return false;
        };
        return a == b;
    }
    let (Ok(a), Ok(b)) = (TimeZone::get(a), TimeZone::get(b)) else {
        return false;
    };
    time_zones_have_same_rules(&a, &b)
}

fn parse_fixed_offset(input: &str) -> std::result::Result<Offset, jiff::Error> {
    DateTimeParser::new()
        .parse_time_zone(input)
        .and_then(|time_zone| time_zone.to_fixed_offset())
}

fn time_zones_have_same_rules(a: &TimeZone, b: &TimeZone) -> bool {
    if a == b {
        return true;
    }
    let a_initial = a.to_offset_info(Timestamp::MIN);
    let b_initial = b.to_offset_info(Timestamp::MIN);
    if a_initial.offset() != b_initial.offset()
        || a_initial.abbreviation() != b_initial.abbreviation()
        || a_initial.dst() != b_initial.dst()
    {
        return false;
    }

    let mut a_transitions = a.following(Timestamp::MIN);
    let mut b_transitions = b.following(Timestamp::MIN);
    loop {
        match (a_transitions.next(), b_transitions.next()) {
            (None, None) => return true,
            (Some(a), Some(b))
                if a.timestamp() == b.timestamp()
                    && a.offset() == b.offset()
                    && a.abbreviation() == b.abbreviation()
                    && a.dst() == b.dst() => {}
            _ => return false,
        }
    }
}

fn is_offset_identifier(time_zone: &str) -> bool {
    matches!(time_zone.as_bytes().first(), Some(b'+' | b'-'))
}

pub(super) fn with_time_zone(zoned: &Zoned, time_zone: &str) -> Result<(Zoned, String)> {
    let (tz, id) = resolve_time_zone(time_zone, "ZonedDateTime.withTimeZone")?;
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
