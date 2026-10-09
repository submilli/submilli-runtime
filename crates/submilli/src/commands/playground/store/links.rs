//! The rerun log: `links.jsonl`, one line for each run the playground ran again live,
//! naming the run it came from. A test run carries its source in its own record
//! (`test_of`); a rerun is an ordinary run, so its link is kept here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FORMAT, Result, Store, append_line, json_line, now_micros, read_lines};

/// One line of the rerun log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LinkLine {
    pub(crate) format: u32,
    /// Microseconds since the Unix epoch when the line was written.
    pub(crate) at_micros: u64,
    /// The new run.
    pub(crate) run: u64,
    /// The run whose program it ran again.
    pub(crate) rerun_of: u64,
}

impl Store {
    pub(crate) fn links_path(&self) -> std::path::PathBuf {
        self.root.join("links.jsonl")
    }

    /// Notes that `run` ran `rerun_of`'s program again.
    pub(crate) fn append_link(&self, run: u64, rerun_of: u64) -> Result<()> {
        self.writer()?;
        let _inner = self.lock();
        append_line(
            &self.links_path(),
            &json_line(&LinkLine {
                format: FORMAT,
                at_micros: now_micros(),
                run,
                rerun_of,
            }),
        )
    }

    /// Each rerun's source, by the rerun's id, without lines that do not parse.
    pub(crate) fn reruns(&self) -> Result<BTreeMap<u64, u64>> {
        Ok(read_lines::<LinkLine>(&self.links_path())?
            .records
            .into_iter()
            .map(|line| (line.run, line.rerun_of))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rerun_reads_back_with_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        assert!(store.reruns().unwrap().is_empty());
        store.append_link(7, 3).unwrap();
        let read = Store::open_read_only(store.root()).unwrap();
        assert_eq!(read.reruns().unwrap(), BTreeMap::from([(7, 3)]));
        assert!(read.append_link(8, 3).is_err());
    }
}
