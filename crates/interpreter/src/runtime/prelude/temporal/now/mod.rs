//! `Temporal.Now` operations.

mod install;

pub(super) use install::{declare, install};

use jiff::{Timestamp, Zoned, tz::TimeZone};
use wasmtime::Caller;

use crate::runtime::StoreData;

pub(super) fn instant() -> Timestamp {
    Timestamp::now()
}

pub(super) fn time_zone_id() -> String {
    TimeZone::system().iana_name().unwrap_or("UTC").to_string()
}

pub(super) fn zoned_date_time_iso(
    caller: &mut Caller<'_, StoreData>,
    time_zone: Option<&str>,
) -> wasmtime::Result<(Zoned, String)> {
    let requested = time_zone.map_or_else(time_zone_id, str::to_string);
    let (zone, id) =
        super::zoned_date_time::resolve_time_zone(caller, &requested, "Now.zonedDateTimeISO")?;
    Ok((Timestamp::now().to_zoned(zone), id))
}
