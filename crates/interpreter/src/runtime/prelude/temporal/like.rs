//! ToTemporal* conversions shared by constructors and methods accepting Like arguments.

use jiff::{Timestamp, Zoned, civil};
use wasmtime::{Caller, StructType, Val};

use super::shared::*;
use crate::runtime::host::{range_error, read_string_arg, type_error};
use crate::runtime::prelude::collection::is_a;
use crate::runtime::{StoreData, fuel};

#[derive(Clone, Copy, Default)]
pub(super) enum Overflow {
    #[default]
    Constrain,
    Reject,
}

impl Overflow {
    pub(super) fn read(
        caller: &mut Caller<'_, StoreData>,
        options: Option<Val>,
    ) -> wasmtime::Result<Self> {
        let Some(options) = options else {
            return Ok(Self::Constrain);
        };
        if crate::runtime::prelude::undefined::is_undefined(caller, &options)? {
            return Ok(Self::Constrain);
        }
        require_options_object(caller, &options)?;
        let Some(overflow) = object_field(caller, &options, "overflow")? else {
            return Ok(Self::Constrain);
        };
        let string = crate::runtime::intrinsic_types::intrinsic_types(&mut *caller)?
            .string
            .clone();
        if !is_a(caller, &overflow, &string)? {
            return Err(range_error(
                "Temporal.from: overflow must be constrain or reject",
            ));
        }
        match read_string_arg(caller, &overflow, "Temporal.from overflow")?.as_str() {
            "constrain" => Ok(Self::Constrain),
            "reject" => Ok(Self::Reject),
            _ => Err(range_error(
                "Temporal.from: overflow must be constrain or reject",
            )),
        }
    }

    fn regulate(self, value: f64, min: i32, max: i32, field: &str) -> wasmtime::Result<i32> {
        if matches!(self, Self::Reject) && (value < f64::from(min) || value > f64::from(max)) {
            return Err(range_error(format!(
                "Temporal.from: {field} is outside {min}..={max}"
            )));
        }
        // Bounds are applied in f64 before narrowing, including for finite values beyond i64.
        Ok(value.clamp(f64::from(min), f64::from(max)) as i32)
    }
}

pub(super) type PlainConverter =
    fn(&mut Caller<'_, StoreData>, &Val, Option<Val>) -> wasmtime::Result<Val>;

pub(super) fn plain_date(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<Val> {
    let ty = temporal_abi(caller)?.plain_date.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        return Ok(*value);
    }
    let date = date(caller, value, options)?;
    make_plain_date(
        caller,
        i32::from(date.year()),
        i32::from(date.month()),
        i32::from(date.day()),
    )
}

pub(super) fn plain_time(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<Val> {
    let ty = temporal_abi(caller)?.plain_time.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        return Ok(*value);
    }
    let time = time(caller, value, options)?;
    make_plain_time(
        caller,
        i32::from(time.hour()),
        i32::from(time.minute()),
        i32::from(time.second()),
        time.subsec_nanosecond(),
    )
}

pub(super) fn plain_date_time(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<Val> {
    let ty = temporal_abi(caller)?.plain_date_time.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        return Ok(*value);
    }
    let dt = date_time(caller, value, options)?;
    make_plain_date_time(
        caller,
        i32::from(dt.year()),
        i32::from(dt.month()),
        i32::from(dt.day()),
        i32::from(dt.hour()),
        i32::from(dt.minute()),
        i32::from(dt.second()),
        dt.subsec_nanosecond(),
    )
}

pub(super) fn plain_year_month(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<Val> {
    let ty = temporal_abi(caller)?.plain_year_month.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        return Ok(*value);
    }
    if let Some(date) = date_projection(caller, value)? {
        Overflow::read(caller, options)?;
        return make_plain_year_month(caller, i32::from(date.year()), i32::from(date.month()));
    }
    let (year, month) = if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        super::plain_year_month::parse(&text).map_err(range_error)?
    } else {
        let fields = year_month_fields(caller, value)?;
        // YearMonth validates options before requiring the prepared fields.
        let overflow = Overflow::read(caller, options)?;
        let year = required(fields.year, "year")?;
        (
            Overflow::Reject.regulate(year, -9999, 9999, "year")?,
            fields.month(overflow)?,
        )
    };
    make_plain_year_month(caller, year, month)
}

pub(super) fn plain_month_day(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<Val> {
    let ty = temporal_abi(caller)?.plain_month_day.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        return Ok(*value);
    }
    if let Some(date) = date_projection(caller, value)? {
        Overflow::read(caller, options)?;
        return make_plain_month_day(caller, i32::from(date.month()), i32::from(date.day()));
    }
    let (month, day) = if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        super::plain_month_day::parse(&text).map_err(range_error)?
    } else {
        let fields = date_fields(caller, value)?;
        required(fields.day, "day")?;
        fields.require_month()?;
        let overflow = Overflow::read(caller, options)?;
        // ISO monthCode selects the leap reference year; the supplied year was
        // still converted above, so non-finite years cannot escape validation.
        let year = if fields.month_code.is_some() {
            1972.0
        } else {
            fields.year.unwrap_or(1972.0)
        };
        let date = fields.date_in_year(year, overflow)?;
        (i32::from(date.month()), i32::from(date.day()))
    };
    make_plain_month_day(caller, month, day)
}

pub(super) fn instant(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Timestamp> {
    let abi = temporal_abi(caller)?;
    let instant_ty = abi.instant.clone();
    let zoned_ty = abi.zoned_date_time.clone();
    if is_a(caller, value, &instant_ty)? || is_a(caller, value, &zoned_ty)? {
        return instant_from_val(caller, value, "Instant.from");
    }
    let text = string(caller, value)?.ok_or_else(|| {
        type_error("Temporal.Instant.from: expected an Instant, ZonedDateTime or string")
    })?;
    super::instant::parse(&text).map_err(range_error)
}

pub(super) fn zoned(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<(Zoned, String)> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    if is_a(caller, value, &ty)? {
        Overflow::read(caller, options)?;
        let zoned = zoned_date_time_from_val(caller, value, "ZonedDateTime.from")?;
        let id = zoned_date_time_time_zone_id_from_val(caller, value, "ZonedDateTime.from")?;
        return Ok((zoned, id));
    }
    if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        fuel::charge_host_fuel(&mut *caller, fuel::TZ)?;
        return super::zoned_date_time::parse(&text);
    }
    let fields = date_fields(caller, value)?;
    let time = time_fields(caller, value)?;
    let offset = object_string_field(caller, value, "offset", "ZonedDateTime.from")?;
    let zone = object_field(caller, value, "timeZone")?
        .ok_or_else(|| type_error("Temporal.ZonedDateTime.from: timeZone is required"))?;
    let zone_id = time_zone(caller, &zone, "ZonedDateTime.from")?;
    let (zone, canonical_id) =
        super::zoned_date_time::resolve_time_zone(caller, &zone_id, "ZonedDateTime.from")?;
    fields.require_date()?;
    let overflow = Overflow::read(caller, options)?;
    let date = fields.date(overflow)?;
    let time = regulate_time(time, overflow)?;
    let dt = date.to_datetime(time);
    let ambiguous = match offset {
        None => zone.into_ambiguous_zoned(dt),
        Some(offset) => {
            fuel::charge(&mut *caller, fuel::PARSE, offset.len() as u64)?;
            let offset = numeric_offset(&offset)?;
            jiff::tz::OffsetConflict::Reject
                .resolve(dt, offset, zone)
                .map_err(temporal_err("ZonedDateTime.from"))?
        }
    };
    let zoned = ambiguous
        .compatible()
        .map_err(temporal_err("ZonedDateTime.from"))?;
    Ok((zoned, canonical_id))
}

pub(super) fn date(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<civil::Date> {
    if let Some(date) = date_projection(caller, value)? {
        Overflow::read(caller, options)?;
        return Ok(date);
    }
    if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        return super::plain_date::parse(&text).map_err(range_error);
    }
    let fields = date_fields(caller, value)?;
    fields.require_date()?;
    let overflow = Overflow::read(caller, options)?;
    fields.date(overflow)
}

pub(super) fn time(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<civil::Time> {
    let abi = temporal_abi(caller)?;
    let time_ty = abi.plain_time.clone();
    let datetime_ty = abi.plain_date_time.clone();
    let zoned_ty = abi.zoned_date_time.clone();
    if is_a(caller, value, &time_ty)? || is_a(caller, value, &datetime_ty)? {
        let offset = if is_a(caller, value, &time_ty)? { 0 } else { 3 };
        Overflow::read(caller, options)?;
        let st = as_struct_val(caller, value, "PlainTime.from")?;
        return plain_time_from_struct(caller, st, offset, "PlainTime.from");
    }
    if is_a(caller, value, &zoned_ty)? {
        Overflow::read(caller, options)?;
        return Ok(zoned_date_time_from_val(caller, value, "PlainTime.from")?.time());
    }
    if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        return super::plain_time::parse(&text).map_err(range_error);
    }
    let fields = time_fields(caller, value)?;
    let overflow = Overflow::read(caller, options)?;
    if fields.iter().all(Option::is_none) {
        return Err(type_error(
            "Temporal.PlainTime.from: at least one time field is required",
        ));
    }
    regulate_time(fields, overflow)
}

pub(super) fn date_time(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    options: Option<Val>,
) -> wasmtime::Result<civil::DateTime> {
    let abi = temporal_abi(caller)?;
    let datetime_ty = abi.plain_date_time.clone();
    let date_ty = abi.plain_date.clone();
    let zoned_ty = abi.zoned_date_time.clone();
    if is_a(caller, value, &datetime_ty)? {
        Overflow::read(caller, options)?;
        let st = as_struct_val(caller, value, "PlainDateTime.from")?;
        return Ok(plain_date_from_struct(caller, st, "PlainDateTime.from")?
            .to_datetime(plain_time_from_struct(caller, st, 3, "PlainDateTime.from")?));
    }
    if is_a(caller, value, &date_ty)? {
        Overflow::read(caller, options)?;
        let st = as_struct_val(caller, value, "PlainDateTime.from")?;
        return Ok(plain_date_from_struct(caller, st, "PlainDateTime.from")?
            .to_datetime(civil::Time::midnight()));
    }
    if is_a(caller, value, &zoned_ty)? {
        Overflow::read(caller, options)?;
        return Ok(zoned_date_time_from_val(caller, value, "PlainDateTime.from")?.datetime());
    }
    if let Some(text) = string(caller, value)? {
        Overflow::read(caller, options)?;
        return super::plain_date_time::parse(&text).map_err(range_error);
    }
    let fields = date_fields(caller, value)?;
    let time = time_fields(caller, value)?;
    fields.require_date()?;
    let overflow = Overflow::read(caller, options)?;
    Ok(fields
        .date(overflow)?
        .to_datetime(regulate_time(time, overflow)?))
}

fn string(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<Option<String>> {
    let ty: StructType = caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| crate::runtime::host::fatal_host_error("Temporal: host ABI unset"))?
        .string_type
        .clone();
    if !is_a(caller, value, &ty)? {
        return Ok(None);
    }
    let text = read_string_arg(caller, value, "Temporal.from")?;
    fuel::charge(&mut *caller, fuel::PARSE, text.len() as u64)?;
    Ok(Some(text))
}

fn number(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    field: &str,
) -> wasmtime::Result<Option<f64>> {
    let Some(value) = object_f64_field(caller, value, field, "from")? else {
        return Ok(None);
    };
    if !value.is_finite() {
        return Err(range_error(format!(
            "Temporal.from: {field} must be finite"
        )));
    }
    Ok(Some(value.trunc()))
}

fn required(value: Option<f64>, field: &str) -> wasmtime::Result<f64> {
    value.ok_or_else(|| type_error(format!("Temporal.from: {field} is required")))
}

fn calendar(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<()> {
    if let Some(calendar) = object_string_field(caller, value, "calendar", "from")?
        && !calendar.eq_ignore_ascii_case("iso8601")
    {
        return Err(range_error(
            "Temporal.from: only the iso8601 calendar is supported",
        ));
    }
    Ok(())
}

struct DateFields {
    year: Option<f64>,
    month: Option<f64>,
    month_code: Option<String>,
    day: Option<f64>,
}

impl DateFields {
    fn require_month(&self) -> wasmtime::Result<()> {
        if self.month.is_none() && self.month_code.is_none() {
            return Err(type_error("Temporal.from: month or monthCode is required"));
        }
        Ok(())
    }

    fn require_date(&self) -> wasmtime::Result<()> {
        required(self.year, "year")?;
        required(self.day, "day")?;
        self.require_month()
    }

    fn month(&self, overflow: Overflow) -> wasmtime::Result<i32> {
        if let Some(code) = &self.month_code {
            // The pattern validates length and ASCII before decoding either digit.
            let [b'M', tens @ b'0'..=b'1', ones @ b'0'..=b'9'] = code.as_bytes() else {
                return Err(range_error(
                    "Temporal.from: monthCode must be M01 through M12",
                ));
            };
            let month = i32::from(tens - b'0') * 10 + i32::from(ones - b'0');
            if !(1..=12).contains(&month)
                || self.month.is_some_and(|value| value != f64::from(month))
            {
                return Err(range_error(
                    "Temporal.from: month and monthCode must agree in M01 through M12",
                ));
            }
            return Ok(month);
        }
        overflow.regulate(required(self.month, "month or monthCode")?, 1, 12, "month")
    }

    fn date(&self, overflow: Overflow) -> wasmtime::Result<civil::Date> {
        self.date_in_year(required(self.year, "year")?, overflow)
    }

    fn date_in_year(&self, year: f64, overflow: Overflow) -> wasmtime::Result<civil::Date> {
        let year = Overflow::Reject.regulate(year, -9999, 9999, "year")?;
        let month = self.month(overflow)?;
        let day = required(self.day, "day")?;
        let limit = days_in_month(i64::from(year), i64::from(month), "from")?;
        let day = overflow.regulate(day, 1, limit as i32, "day")?;
        y_m_d_date(i64::from(year), i64::from(month), i64::from(day), "from")
    }
}

fn date_fields(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<DateFields> {
    calendar(caller, value)?;
    let day = positive_field(caller, value, "day")?;
    let month = positive_field(caller, value, "month")?;
    let month_code = object_string_field(caller, value, "monthCode", "from")?;
    let year = number(caller, value, "year")?;
    Ok(DateFields {
        year,
        month,
        month_code,
        day,
    })
}

fn year_month_fields(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<DateFields> {
    calendar(caller, value)?;
    let month = positive_field(caller, value, "month")?;
    let month_code = object_string_field(caller, value, "monthCode", "from")?;
    let year = number(caller, value, "year")?;
    Ok(DateFields {
        year,
        month,
        month_code,
        day: None,
    })
}

fn positive_field(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    field: &str,
) -> wasmtime::Result<Option<f64>> {
    let value = number(caller, value, field)?;
    if let Some(value) = value {
        positive(value, field)?;
    }
    Ok(value)
}

const TIME_FIELDS: [&str; 6] = [
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];

fn time_fields(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<[Option<f64>; 6]> {
    let mut fields = [None; 6];
    for (field, name) in fields.iter_mut().zip(TIME_FIELDS) {
        *field = number(caller, value, name)?;
    }
    Ok(fields)
}

fn regulate_time(fields: [Option<f64>; 6], overflow: Overflow) -> wasmtime::Result<civil::Time> {
    let mut values = [0; 6];
    for ((slot, value), name) in values.iter_mut().zip(fields).zip(TIME_FIELDS) {
        let max = match name {
            "hour" => 23,
            "minute" | "second" => 59,
            _ => 999,
        };
        *slot = overflow.regulate(value.unwrap_or(0.0), 0, max, name)?;
    }
    // Regulation bounds every field; their combined nanoseconds fit in i32.
    let [hour, minute, second, milli, micro, nano] = values;
    h_m_s_ns_time(
        i64::from(hour),
        i64::from(minute),
        i64::from(second),
        i64::from(milli * 1_000_000 + micro * 1_000 + nano),
        "from",
    )
}

fn numeric_offset(input: &str) -> wasmtime::Result<jiff::tz::Offset> {
    let invalid =
        || range_error("Temporal.ZonedDateTime.from: offset must be a matching numeric UTC offset");
    let (sign, rest) = if let Some(rest) = input.strip_prefix('+') {
        (1, rest)
    } else if let Some(rest) = input.strip_prefix('-').or_else(|| input.strip_prefix('−')) {
        (-1, rest)
    } else {
        return Err(invalid());
    };
    let (whole, fraction) = rest
        .split_once(['.', ','])
        .map_or((rest, None), |(whole, fraction)| (whole, Some(fraction)));
    let zero = b'0';
    let (hours, minutes, seconds, has_seconds) = match whole.as_bytes() {
        [a, b] => ([*a, *b], [zero, zero], [zero, zero], false),
        [a, b, c, d] | [a, b, b':', c, d] => ([*a, *b], [*c, *d], [zero, zero], false),
        [a, b, c, d, e, f] | [a, b, b':', c, d, b':', e, f] => ([*a, *b], [*c, *d], [*e, *f], true),
        _ => return Err(invalid()),
    };
    // Native zone offsets have whole-second precision. A nonzero fraction can
    // never match one; zero fractions are valid only after explicit seconds.
    if fraction.is_some_and(|fraction| {
        !has_seconds
            || fraction.is_empty()
            || fraction.len() > 9
            || !fraction.bytes().all(|byte| byte == b'0')
    }) {
        return Err(invalid());
    }
    let pair = |digits: [u8; 2]| -> wasmtime::Result<i32> {
        let [tens, ones] = digits;
        if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
            return Err(invalid());
        }
        Ok(i32::from(tens - b'0') * 10 + i32::from(ones - b'0'))
    };
    let (hours, minutes, seconds) = (pair(hours)?, pair(minutes)?, pair(seconds)?);
    if hours > 23 || minutes > 59 || seconds > 59 {
        return Err(invalid());
    }
    jiff::tz::Offset::from_seconds(sign * (hours * 3600 + minutes * 60 + seconds))
        .map_err(temporal_err("ZonedDateTime.from offset"))
}

/// The supported ISO subset of TypeScript's Temporal `…Like` unions. Bags use
/// object shapes in both declarations so direct host calls retain their data slots.
pub(crate) fn like_type(local: &str) -> crate::Type {
    use crate::{ObjectField, Type};
    if local == "TimeZone" {
        return Type::union(vec![Type::String, temporal_type("ZonedDateTime")]);
    }
    let mut variants = vec![temporal_type(local), Type::String];
    let others: &[&str] = match local {
        "Instant" => &["ZonedDateTime"],
        "PlainDate" => &["ZonedDateTime", "PlainDateTime"],
        "PlainYearMonth" | "PlainMonthDay" => &["PlainDate", "ZonedDateTime", "PlainDateTime"],
        "PlainTime" => &["PlainDateTime", "ZonedDateTime"],
        "PlainDateTime" => &["ZonedDateTime", "PlainDate"],
        _ => &[],
    };
    variants.extend(others.iter().map(|name| temporal_type(name)));
    let numbers: &[&str] = match local {
        "Duration" => &DURATION_FIELD_NAMES,
        "PlainDate" | "PlainMonthDay" => &["year", "month", "day"],
        "PlainYearMonth" => &["year", "month"],
        "PlainTime" => &[
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
        ],
        "PlainDateTime" | "ZonedDateTime" => &[
            "year",
            "month",
            "day",
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
        ],
        _ => return Type::union(variants),
    };
    let mut fields = numbers
        .iter()
        .map(|name| {
            let field = if *name == "day" {
                ObjectField::required(Type::Number)
            } else {
                ObjectField::optional(Type::Number)
            };
            ((*name).to_string(), field)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    if !matches!(local, "Duration" | "PlainTime") {
        for name in ["calendar", "monthCode", "era"] {
            fields.insert(name.to_string(), ObjectField::optional(Type::String));
        }
        fields.insert("eraYear".to_string(), ObjectField::optional(Type::Number));
    }
    if local == "ZonedDateTime" {
        fields.insert(
            "timeZone".to_string(),
            ObjectField::required(Type::union(vec![
                Type::String,
                temporal_type("ZonedDateTime"),
            ])),
        );
        fields.insert("offset".to_string(), ObjectField::optional(Type::String));
    }
    variants.push(Type::Object {
        index: None,
        fields,
    });
    Type::union(variants)
}

pub(crate) fn overflow_param() -> crate::Param {
    use crate::{DefaultValue, ObjectField, Param, Type};
    Param::with_default(
        "options",
        Type::union(vec![
            Type::Object {
                index: None,
                fields: std::collections::BTreeMap::from([(
                    "overflow".to_string(),
                    ObjectField::optional(Type::union(vec![
                        Type::StringLiteral("constrain".to_string()),
                        Type::StringLiteral("reject".to_string()),
                    ])),
                )]),
            },
            Type::Undefined,
        ]),
        DefaultValue::Undefined,
    )
}

fn positive(value: f64, field: &str) -> wasmtime::Result<()> {
    if value <= 0.0 {
        return Err(range_error(format!(
            "Temporal.from: {field} must be positive"
        )));
    }
    Ok(())
}

fn date_projection(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Option<civil::Date>> {
    let abi = temporal_abi(caller)?;
    let date_ty = abi.plain_date.clone();
    let datetime_ty = abi.plain_date_time.clone();
    let zoned_ty = abi.zoned_date_time.clone();
    if is_a(caller, value, &date_ty)? || is_a(caller, value, &datetime_ty)? {
        let st = as_struct_val(caller, value, "from")?;
        return plain_date_from_struct(caller, st, "from").map(Some);
    }
    if is_a(caller, value, &zoned_ty)? {
        return Ok(Some(
            zoned_date_time_from_val(caller, value, "from")?.date(),
        ));
    }
    Ok(None)
}

pub(super) fn time_zone(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    label: &'static str,
) -> wasmtime::Result<String> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    if is_a(caller, value, &ty)? {
        return zoned_date_time_time_zone_id_from_val(caller, value, label);
    }
    read_string_arg(caller, value, label)
}

pub(super) fn relative_to(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Anchor> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    if is_a(caller, value, &ty)? || object_field(caller, value, "timeZone")?.is_some() {
        return zoned(caller, value, None).map(|(zoned, _)| Anchor::Zoned(zoned));
    }
    if let Some(text) = string(caller, value)? {
        let pieces =
            jiff::fmt::temporal::Pieces::parse(&text).map_err(temporal_err("relativeTo"))?;
        if pieces.time_zone_annotation().is_some() {
            fuel::charge_host_fuel(&mut *caller, fuel::TZ)?;
            return super::zoned_date_time::parse(&text).map(|(zoned, _)| Anchor::Zoned(zoned));
        }
        if pieces
            .offset()
            .is_some_and(|offset| matches!(offset, jiff::fmt::temporal::PiecesOffset::Zulu))
        {
            return Err(range_error(
                "Temporal.relativeTo: a UTC designator requires a bracketed time zone",
            ));
        }
        return Ok(Anchor::Date(pieces.date()));
    }
    date(caller, value, None).map(Anchor::Date)
}

/// Ordering needs only the timestamp; do not copy a native carrier's zone ID.
pub(super) fn zoned_timestamp(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Timestamp> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    if is_a(caller, value, &ty)? {
        return zoned_date_time_timestamp_from_val(caller, value, "ZonedDateTime.compare");
    }
    zoned(caller, value, None).map(|(zoned, _)| zoned.timestamp())
}

pub(super) fn zoned_value(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Zoned> {
    let ty = temporal_abi(caller)?.zoned_date_time.clone();
    if is_a(caller, value, &ty)? {
        return zoned_date_time_from_val(caller, value, "ZonedDateTime difference");
    }
    zoned(caller, value, None).map(|(zoned, _)| zoned)
}

fn require_options_object(
    caller: &mut Caller<'_, StoreData>,
    options: &Val,
) -> wasmtime::Result<()> {
    let invalid = || type_error("Temporal.from: options must be an object or undefined");
    let Val::AnyRef(Some(reference)) = options else {
        return Err(invalid());
    };
    if reference.as_struct(&mut *caller)?.is_none() {
        return Err(invalid());
    }
    let primitive_types = {
        let intr = crate::runtime::intrinsic_types::intrinsic_types(&mut *caller)?;
        [
            intr.string.clone(),
            intr.boxed_number.clone(),
            intr.boxed_boolean.clone(),
            intr.bigint.clone(),
        ]
    };
    for ty in primitive_types {
        if is_a(caller, options, &ty)? {
            return Err(invalid());
        }
    }
    Ok(())
}
