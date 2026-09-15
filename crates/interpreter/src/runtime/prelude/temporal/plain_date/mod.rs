//! `Temporal.PlainDate` operations.

mod install;

pub(super) use install::{declare, install};

use std::str::FromStr;

use jiff::{Span, civil};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<civil::Date> {
    civil::Date::from_str(input.trim()).map_err(|_| {
        format!(
            "Temporal.PlainDate.from: {input:?} is not a valid ISO 8601 date (expected e.g. \"2024-03-09\")"
        )
    })
}

pub(super) fn to_string(date: civil::Date) -> String {
    date.to_string()
}

pub(super) fn equals(a: civil::Date, b: civil::Date) -> bool {
    a == b
}

pub(super) fn add(date: civil::Date, span: Span) -> Result<civil::Date> {
    date.checked_add(span).map_err(|_| {
        super::shared::temporal_error(
            "PlainDate.add",
            format!(
                "cannot add {span} to {date}; use a duration whose result stays within the supported date range"
            ),
        )
    })
}

pub(super) fn subtract(date: civil::Date, span: Span) -> Result<civil::Date> {
    date.checked_sub(span).map_err(|_| {
        super::shared::temporal_error(
            "PlainDate.subtract",
            format!(
                "cannot subtract {span} from {date}; use a duration whose result stays within the supported date range"
            ),
        )
    })
}
