//! `Temporal.PlainTime` operations.

mod install;

pub(super) use install::{declare, install};

use std::str::FromStr;

use jiff::{Span, civil};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<civil::Time> {
    civil::Time::from_str(input.trim()).map_err(|_| {
        format!(
            "Temporal.PlainTime.from: {input:?} is not a valid ISO 8601 time (expected e.g. \"15:30:45\" or \"15:30:45.123\")"
        )
    })
}

pub(super) fn to_string(time: civil::Time) -> String {
    time.to_string()
}

pub(super) fn equals(a: civil::Time, b: civil::Time) -> bool {
    a == b
}

pub(super) fn add(time: civil::Time, span: Span) -> civil::Time {
    time.wrapping_add(span)
}

pub(super) fn subtract(time: civil::Time, span: Span) -> civil::Time {
    time.wrapping_sub(span)
}
