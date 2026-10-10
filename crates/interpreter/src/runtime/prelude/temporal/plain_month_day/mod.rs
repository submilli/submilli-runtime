//! `Temporal.PlainMonthDay` operations.

mod install;

pub(super) use install::{declare, install};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<(i32, i32)> {
    let input = input.trim();
    let bad = || format!("Temporal.PlainMonthDay.from: expected `MM-DD`, got {input:?}");
    let body = input.strip_prefix("--").unwrap_or(input);
    let (month, day) = body.split_once('-').ok_or_else(bad)?;
    let month: i32 = month.parse().map_err(|_| bad())?;
    let day: i32 = day.parse().map_err(|_| bad())?;
    super::shared::y_m_d_date(2000, i64::from(month), i64::from(day), "PlainMonthDay.from")
        .map_err(|err| err.to_string())?;
    Ok((month, day))
}

pub(super) fn to_string(month: i32, day: i32) -> String {
    format!("{month:02}-{day:02}")
}

pub(super) fn equals(a: (i32, i32), b: (i32, i32)) -> bool {
    a == b
}
