//! Runs in flight: `running/<id>.json`, written when a run starts and removed when it
//! finishes, so `runs` can list a run (and `cancel` can be given its id) before it is
//! stored. Only the running playground writes them, and its open removes any a crash
//! left; a reader trusts them only while the playground that wrote them is alive.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{FORMAT, Result, Store, io_error, parse_record, write_private};

/// One run in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RunningRun {
    pub(crate) format: u32,
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) session_id: Option<String>,
    /// Microseconds since the Unix epoch when it started.
    pub(crate) started_at_micros: u64,
}

impl Store {
    pub(crate) fn running_dir(&self) -> PathBuf {
        self.root.join("running")
    }

    fn running_path(&self, id: u64) -> PathBuf {
        self.running_dir().join(format!("{id}.json"))
    }

    /// Notes that run `id` started.
    pub(crate) fn mark_running(
        &self,
        id: u64,
        label: &str,
        session_id: Option<&str>,
        started_at_micros: u64,
    ) -> Result<()> {
        self.writer()?;
        let dir = self.running_dir();
        super::private_dir_all(&dir)?;
        write_private(
            &self.running_path(id),
            &super::json_bytes(&RunningRun {
                format: FORMAT,
                id,
                label: label.to_owned(),
                session_id: session_id.map(str::to_owned),
                started_at_micros,
            }),
        )
    }

    /// Notes that run `id` is no longer in flight. Removing a note that is not there
    /// is not an error.
    pub(crate) fn unmark_running(&self, id: u64) -> Result<()> {
        self.writer()?;
        let path = self.running_path(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(&path)(error)),
        }
    }

    /// The runs noted in flight, by id. A note that does not parse is left out.
    pub(crate) fn running(&self) -> Result<Vec<RunningRun>> {
        let dir = self.running_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(&dir)(error)),
        };
        let mut runs = Vec::new();
        for entry in entries {
            let path = entry.map_err(io_error(&dir))?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                // Finished between the listing and the read.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io_error(&path)(error)),
            };
            if let Ok(run) = parse_record::<RunningRun>(&path, &bytes) {
                runs.push(run);
            }
        }
        runs.sort_by_key(|run| run.id);
        Ok(runs)
    }

    /// Forgets every note: on opening, no run of an earlier process is in flight.
    pub(super) fn clear_running(&self) -> Result<()> {
        let dir = self.running_dir();
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(&dir)(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_listed_in_flight_from_start_to_finish_and_not_after_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let store = Store::open(&root).unwrap();
        store.mark_running(3, "assistant", Some("s-1"), 10).unwrap();
        store.mark_running(4, "example", None, 11).unwrap();
        let read = Store::open_read_only(&root).unwrap();
        let ids: Vec<u64> = read.running().unwrap().iter().map(|run| run.id).collect();
        assert_eq!(ids, [3, 4]);
        assert!(read.mark_running(5, "x", None, 0).is_err());
        store.unmark_running(3).unwrap();
        store.unmark_running(3).unwrap();
        assert_eq!(read.running().unwrap().len(), 1);
        drop(store);
        // A crash left run 4 noted; the next playground's open forgets it.
        let _reopened = Store::open(&root).unwrap();
        assert!(read.running().unwrap().is_empty());
    }
}
