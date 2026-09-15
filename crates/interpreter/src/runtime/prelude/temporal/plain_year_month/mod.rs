//! `Temporal.PlainYearMonth` operations.

mod install;

pub(super) use install::{declare, install};

type Result<T> = std::result::Result<T, String>;

pub(super) fn parse(input: &str) -> Result<(i32, i32)> {
    let input = input.trim();
    let bad = || format!("Temporal.PlainYearMonth.from: expected `YYYY-MM`, got {input:?}");
    let (year, month) = input.split_once('-').ok_or_else(bad)?;
    let year: i32 = year.parse().map_err(|_| bad())?;
    let month: i32 = month.parse().map_err(|_| bad())?;
    super::shared::y_m_d_date(i64::from(year), i64::from(month), 1, "PlainYearMonth.from")
        .map_err(|err| err.to_string())?;
    Ok((year, month))
}

pub(super) fn to_string(year: i32, month: i32) -> String {
    format!("{year:04}-{month:02}")
}

pub(super) fn equals(a: (i32, i32), b: (i32, i32)) -> bool {
    a == b
}
