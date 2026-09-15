//! `Temporal.Duration` operations.

mod install;

pub(super) use install::{declare, install};

use std::cmp::Ordering;
use std::str::FromStr;

use jiff::{Span, SpanArithmetic, SpanCompare, SpanRelativeTo, SpanRound, Unit};

use super::shared::Anchor;

type Result<T> = std::result::Result<T, String>;

const FIELD_LIMITS: [i64; 10] = [
    19_998,
    239_976,
    1_043_497,
    7_304_484,
    175_307_616,
    10_518_456_960,
    631_107_417_600,
    631_107_417_600_000,
    631_107_417_600_000_000,
    i64::MAX,
];

const MAX_TIME_NANOSECONDS_EXCLUSIVE: f64 = 9_007_199_254_740_992_000_000_000.0;
const I64_BOUND_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

pub(super) fn parse(input: &str) -> Result<Span> {
    Span::from_str(input.trim()).map_err(|_| {
        format!(
            "Temporal.Duration.from: {input:?} is not a valid ISO 8601 duration (expected e.g. \"PT1H30M\" or \"P1Y2M3DT4H5M6S\")"
        )
    })
}

pub(super) fn from_fields(fields: [i64; 10], label: &str) -> Result<Span> {
    let mut first_nonzero: Option<(usize, i64)> = None;
    for (index, value) in fields.iter().copied().enumerate() {
        let limit = FIELD_LIMITS[index];
        if value < -limit || value > limit {
            let field = super::shared::DURATION_FIELD_NAMES[index];
            let minimum = -limit;
            return Err(super::shared::temporal_error(
                label,
                format!(
                    "duration field {field}={value} is outside the supported range {minimum}..={limit}; use a value within that range"
                ),
            ));
        }
        if value == 0 {
            continue;
        }
        if let Some((first_index, first_value)) = first_nonzero {
            if value.signum() != first_value.signum() {
                let first_field = super::shared::DURATION_FIELD_NAMES[first_index];
                let field = super::shared::DURATION_FIELD_NAMES[index];
                return Err(super::shared::temporal_error(
                    label,
                    format!(
                        "duration fields must have the same sign; {first_field}={first_value} conflicts with {field}={value}. Use all non-negative or all non-positive fields"
                    ),
                ));
            }
        } else {
            first_nonzero = Some((index, value));
        }
    }

    Span::new()
        .try_years(fields[0])
        .and_then(|s| s.try_months(fields[1]))
        .and_then(|s| s.try_weeks(fields[2]))
        .and_then(|s| s.try_days(fields[3]))
        .and_then(|s| s.try_hours(fields[4]))
        .and_then(|s| s.try_minutes(fields[5]))
        .and_then(|s| s.try_seconds(fields[6]))
        .and_then(|s| s.try_milliseconds(fields[7]))
        .and_then(|s| s.try_microseconds(fields[8]))
        .and_then(|s| s.try_nanoseconds(fields[9]))
        .map_err(|_| {
            super::shared::temporal_error(
                label,
                "duration fields exceed the supported range when combined; use smaller values",
            )
        })
}

pub(super) fn field_from_f64(value: f64, index: usize, label: &str) -> Result<i64> {
    let field = super::shared::DURATION_FIELD_NAMES[index];
    if !value.is_finite() {
        return Err(super::shared::temporal_error(
            label,
            format!("duration field {field} must be finite"),
        ));
    }
    if value.fract() != 0.0 {
        return Err(super::shared::temporal_error(
            label,
            format!("duration field {field}={value} must be an integer"),
        ));
    }
    if index == 9 && value.abs() >= MAX_TIME_NANOSECONDS_EXCLUSIVE {
        return Err(super::shared::temporal_error(
            label,
            format!(
                "duration field nanoseconds={value} reaches Temporal's exclusive 2^53-second limit; use an absolute value below 9007199254740992000000000"
            ),
        ));
    }

    if value <= -I64_BOUND_EXCLUSIVE || value >= I64_BOUND_EXCLUSIVE {
        return Err(super::shared::temporal_error(
            label,
            format!(
                "duration field {field}={value} is outside this runtime's supported range; use a smaller absolute value"
            ),
        ));
    }
    Ok(value as i64)
}

pub(super) fn has_calendar_units(span: &Span) -> bool {
    span.get_years() != 0 || span.get_months() != 0 || span.get_weeks() != 0
}

pub(super) fn is_calendar_unit(unit: Unit) -> bool {
    matches!(unit, Unit::Year | Unit::Month | Unit::Week)
}

pub(super) fn calendar_anchor_error(label: &str, action: &str) -> String {
    super::shared::temporal_error(
        label,
        format!(
            "cannot {action} without a relativeTo anchor because years, months, and weeks are calendar units; balance those units relative to a PlainDate or ZonedDateTime first"
        ),
    )
}

pub(super) fn add(a: Span, b: Span) -> Result<Span> {
    if has_calendar_units(&a) || has_calendar_units(&b) {
        return Err(calendar_anchor_error("Duration.add", "add these durations"));
    }
    a.checked_add(SpanArithmetic::from(b).days_are_24_hours())
        .map_err(|_| {
            super::shared::temporal_error(
                "Duration.add",
                format!("the result of adding {b} to {a} is outside the supported range; use smaller duration values"),
            )
        })
}

pub(super) fn subtract(a: Span, b: Span) -> Result<Span> {
    if has_calendar_units(&a) || has_calendar_units(&b) {
        return Err(calendar_anchor_error(
            "Duration.subtract",
            "subtract these durations",
        ));
    }
    a.checked_sub(SpanArithmetic::from(b).days_are_24_hours())
        .map_err(|_| {
            super::shared::temporal_error(
                "Duration.subtract",
                format!("the result of subtracting {b} from {a} is outside the supported range; use smaller duration values"),
            )
        })
}

pub(super) fn negated(span: Span) -> Span {
    -span
}

pub(super) fn abs(span: Span) -> Span {
    span.abs()
}

pub(super) fn round(
    span: Span,
    smallest: Option<Unit>,
    largest: Option<Unit>,
    mode: Option<jiff::RoundMode>,
    increment: Option<i64>,
    anchor: Option<Anchor>,
) -> Result<Span> {
    if smallest.is_none() && largest.is_none() {
        return Err(super::shared::temporal_error(
            "Duration.round",
            "one of smallestUnit or largestUnit is required",
        ));
    }
    if anchor.is_none()
        && (has_calendar_units(&span)
            || smallest.is_some_and(is_calendar_unit)
            || largest.is_some_and(is_calendar_unit))
    {
        return Err(calendar_anchor_error(
            "Duration.round",
            "round this duration",
        ));
    }
    let mut options = SpanRound::new();
    if let Some(unit) = smallest {
        options = options.smallest(unit);
    }
    if let Some(unit) = largest {
        options = options.largest(unit);
    }
    if let Some(mode) = mode {
        options = options.mode(mode);
    }
    if let Some(increment) = increment {
        options = options.increment(increment);
    }
    let anchored = anchor.is_some();
    match anchor {
        Some(Anchor::Date(date)) => span.round(options.relative(date)),
        Some(Anchor::Zoned(zoned)) => span.round(options.relative(&zoned)),
        None => span.round(options.days_are_24_hours()),
    }
    .map_err(|_| {
        let description =
            super::shared::round_options_description(smallest, largest, mode, increment);
        let fix = if anchored {
            "use smaller duration values or an anchor closer to the duration's range"
        } else {
            "use compatible units and a supported roundingIncrement"
        };
        super::shared::temporal_error(
            "Duration.round",
            format!("cannot round {span} with {description}; {fix}"),
        )
    })
}

pub(super) fn compare(a: Span, b: Span, anchor: Option<Anchor>) -> Result<f64> {
    if anchor.is_none() && (has_calendar_units(&a) || has_calendar_units(&b)) {
        return Err(calendar_anchor_error(
            "Duration.compare",
            "compare these durations",
        ));
    }
    let anchored = anchor.is_some();
    let ordering = match anchor {
        Some(Anchor::Date(date)) => a.compare((b, SpanRelativeTo::from(date))),
        Some(Anchor::Zoned(zoned)) => a.compare((b, &zoned)),
        None => a.compare(SpanCompare::from(b).days_are_24_hours()),
    }
    .map_err(|_| {
        let detail = if anchored {
            format!(
                "cannot compare {a} and {b} at the supplied relativeTo anchor because the calendar arithmetic is outside the supported range; use smaller durations or an anchor closer to their range"
            )
        } else {
            format!(
                "cannot compare {a} and {b} because the time arithmetic is outside the supported range; use smaller durations"
            )
        };
        super::shared::temporal_error("Duration.compare", detail)
    })?;
    Ok(match ordering {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    })
}
