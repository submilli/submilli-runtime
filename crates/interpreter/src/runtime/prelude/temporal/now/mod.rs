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

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ZonedObservation {
    pub instant: Timestamp,
    pub id: String,
    #[serde(skip)]
    zone: Option<TimeZone>,
}

pub(super) fn zoned_date_time_iso(
    caller: &mut Caller<'_, StoreData>,
    time_zone: Option<&str>,
) -> wasmtime::Result<ZonedObservation> {
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
    Ok(ZonedObservation {
        instant: Timestamp::now(),
        id,
        zone: Some(zone),
    })
}

impl ZonedObservation {
    pub fn zoned(&self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Zoned> {
        let zone = match &self.zone {
            Some(zone) => zone.clone(),
            None => super::zoned_date_time::resolve_time_zone(caller, &self.id, "Temporal.Now")?.0,
        };
        Ok(self.instant.to_zoned(zone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs, fuel};
    use wasmtime::{Func, FuncType};

    #[test]
    fn recorded_zone_reconstruction_charges_only_the_lookup_it_performs() {
        super::super::zoned_date_time::prepare_zones().unwrap();
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let probe = Func::new(
            &mut store,
            FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                for id in ["UTC", "+02:00"] {
                    let observed = zoned_date_time_iso(&mut caller, Some(id))?;
                    let before = caller.data().host_fuel;
                    let live = observed.zoned(&mut caller)?;
                    assert_eq!(caller.data().host_fuel, before);

                    let bytes = rmp_serde::to_vec_named(&observed).unwrap();
                    let restored: ZonedObservation = rmp_serde::from_slice(&bytes).unwrap();
                    assert!(restored.zone.is_none());
                    let replay = restored.zoned(&mut caller)?;
                    assert_eq!(caller.data().host_fuel - before, fuel::TZ);
                    assert_eq!(live.timestamp(), replay.timestamp());
                    assert_eq!(live.offset(), replay.offset());
                }
                Ok(())
            },
        );
        store.set_fuel(1_000_000).unwrap();
        probe.call(&mut store, &[], &mut []).unwrap();
    }
}
