//! `Temporal.PlainDateTime` operations.

mod install;

pub(super) use install::{declare, install};

use std::str::FromStr;

use jiff::{Span, civil};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<civil::DateTime> {
    civil::DateTime::from_str(input.trim()).map_err(|_| {
        format!(
            "Temporal.PlainDateTime.from: {input:?} is not a valid ISO 8601 date-time (expected e.g. \"2024-03-09T15:30:45\")"
        )
    })
}

pub(super) fn to_string(date_time: civil::DateTime) -> String {
    date_time.to_string()
}

pub(super) fn equals(a: civil::DateTime, b: civil::DateTime) -> bool {
    a == b
}

pub(super) fn add(date_time: civil::DateTime, span: Span) -> Result<civil::DateTime> {
    date_time
        .checked_add(span)
        .map_err(|_| {
            super::shared::temporal_error(
                "PlainDateTime.add",
                format!(
                    "cannot add {span} to {date_time}; use a duration whose result stays within the supported date-time range"
                ),
            )
        })
}

pub(super) fn subtract(date_time: civil::DateTime, span: Span) -> Result<civil::DateTime> {
    date_time
        .checked_sub(span)
        .map_err(|_| {
            super::shared::temporal_error(
                "PlainDateTime.subtract",
                format!(
                    "cannot subtract {span} from {date_time}; use a duration whose result stays within the supported date-time range"
                ),
            )
        })
}
