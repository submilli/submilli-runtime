//! `Temporal.Now` operations.

mod install;

pub(super) use install::{declare, install};

use jiff::{Timestamp, Zoned, tz::TimeZone};
use std::sync::OnceLock;
use wasmtime::Caller;

use crate::runtime::StoreData;

pub(super) fn instant() -> Timestamp {
    Timestamp::now()
}

static SYSTEM: OnceLock<TimeZone> = OnceLock::new();

pub(super) fn prepare_system_zone() -> wasmtime::Result<()> {
    if SYSTEM.get().is_some() {
        return Ok(());
    }
    let system = TimeZone::try_system().unwrap_or(TimeZone::UTC);
    let id = system.iana_name().unwrap_or("UTC");
    let _ = SYSTEM.set(super::zoned_date_time::system_zone(id)?);
    Ok(())
}

pub(super) fn time_zone_id() -> &'static str {
    SYSTEM.get().and_then(TimeZone::iana_name).unwrap_or("UTC")
}

pub(super) fn zoned_date_time_iso(
    caller: &mut Caller<'_, StoreData>,
    time_zone: Option<&str>,
) -> wasmtime::Result<(Zoned, String)> {
    let (zone, id) = if let Some(requested) = time_zone {
        super::zoned_date_time::resolve_time_zone(caller, requested, "Now.zonedDateTimeISO")?
    } else {
        let zone = SYSTEM
            .get()
            .ok_or_else(|| {
                crate::runtime::host::fatal_host_error("system time zone was not initialized")
            })?
            .clone();
        (zone, time_zone_id().to_string())
    };
    Ok((Timestamp::now().to_zoned(zone), id))
}
