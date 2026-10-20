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

/// Records the native result while restoring zones from our pinned database.
/// Jiff's default `Zoned` deserializer uses the machine's time-zone database.
pub(super) struct RecordedZoned(pub (Zoned, String));

impl serde::Serialize for RecordedZoned {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let (zoned, id) = &self.0;
        let mut state = serializer.serialize_struct("RecordedZoned", 2)?;
        state.serialize_field("instant", &zoned.timestamp())?;
        state.serialize_field("id", id)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for RecordedZoned {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct SavedZoned {
            instant: Timestamp,
            id: String,
        }

        let saved = SavedZoned::deserialize(deserializer)?;
        let (zone, _) = super::zoned_date_time::lookup_time_zone(&saved.id, "Temporal.Now")
            .map_err(serde::de::Error::custom)?;
        Ok(Self((saved.instant.to_zoned(zone), saved.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_zones_round_trip_through_the_pinned_database() {
        for id in ["UTC", "+02:00", "-05:30", "America/New_York", "US/Eastern"] {
            for instant in ["2024-03-10T06:59:59Z", "2024-03-10T07:00:00Z"] {
                let instant: Timestamp = instant.parse().unwrap();
                let (zone, id) =
                    super::super::zoned_date_time::lookup_time_zone(id, "test").unwrap();
                let original = RecordedZoned((instant.to_zoned(zone), id));
                let bytes = rmp_serde::to_vec_named(&original).unwrap();
                let RecordedZoned((restored, restored_id)) = rmp_serde::from_slice(&bytes).unwrap();
                let (zoned, id) = original.0;
                assert_eq!(restored.timestamp(), zoned.timestamp());
                assert_eq!(restored.datetime(), zoned.datetime());
                assert_eq!(restored.offset(), zoned.offset());
                assert_eq!(restored_id, id);
                assert_eq!(
                    restored.time_zone().iana_name(),
                    zoned.time_zone().iana_name()
                );
            }
        }
    }

    #[test]
    fn previous_recorded_outcomes_keep_the_same_encoding() {
        #[derive(serde::Serialize)]
        struct PreviousObservation {
            instant: Timestamp,
            id: String,
        }

        let previous = PreviousObservation {
            instant: "2024-07-01T12:00:00Z".parse().unwrap(),
            id: "America/New_York".into(),
        };
        let bytes = rmp_serde::to_vec_named(&previous).unwrap();
        let restored: RecordedZoned = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(rmp_serde::to_vec_named(&restored).unwrap(), bytes);
        let RecordedZoned((zoned, id)) = restored;
        assert_eq!(zoned.timestamp(), previous.instant);
        assert_eq!(id, previous.id);
    }

    #[test]
    fn invalid_recorded_zones_are_rejected() {
        for id in ["Not/A_Zone", "+99:00", ""] {
            let saved = serde_json::json!({"instant": "2024-07-01T12:00:00Z", "id": id});
            assert!(serde_json::from_value::<RecordedZoned>(saved).is_err());
        }
    }
}
