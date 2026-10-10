//! A finite, pre-parsed IANA database, independent of filesystem cache expiry.
use std::sync::OnceLock;

use jiff::tz::TimeZone;

use crate::runtime::host::fatal_host_error;

type Zones = Vec<(&'static str, TimeZone)>;
static ZONES: OnceLock<std::result::Result<Zones, &'static str>> = OnceLock::new();

pub(super) fn prepare() -> wasmtime::Result<()> {
    zones().map(|_| ())
}

pub(super) fn named(name: &str) -> wasmtime::Result<Option<TimeZone>> {
    if name.eq_ignore_ascii_case("UTC") {
        return Ok(Some(TimeZone::UTC));
    }
    if name.eq_ignore_ascii_case("Etc/Unknown") {
        return Ok(Some(TimeZone::unknown()));
    }
    let zones = zones()?;
    let found = zones.binary_search_by(|(candidate, _)| compare_names(candidate, name));
    Ok(found
        .ok()
        .and_then(|index| zones.get(index))
        .map(|(_, zone)| zone.clone()))
}

fn zones() -> wasmtime::Result<&'static Zones> {
    ZONES
        .get_or_init(load)
        .as_ref()
        .map_err(|message| fatal_host_error(*message))
}

fn load() -> std::result::Result<Zones, &'static str> {
    let mut zones = Vec::new();
    zones
        .try_reserve_exact(1024)
        .map_err(|_| "cannot reserve built-in time zone database")?;
    for name in jiff_tzdb::available() {
        if zones.len() == 1024 {
            return Err("built-in time zone database exceeds 1024 zones");
        }
        let (canonical, bytes) = jiff_tzdb::get(name).ok_or("missing built-in time zone data")?;
        let zone =
            TimeZone::tzif(canonical, bytes).map_err(|_| "invalid built-in time zone data")?;
        zones.push((canonical, zone));
    }
    zones.sort_unstable_by(|(left, _), (right, _)| compare_names(left, right));
    Ok(zones)
}

fn compare_names(left: &str, right: &str) -> std::cmp::Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}
