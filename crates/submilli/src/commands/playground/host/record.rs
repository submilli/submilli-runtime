//! What the playground does with the host's runs beyond running them: their
//! events, re-checking a run under a newer blueprint, and running it again as a
//! test against its recording and a throwaway copy of today's local state.
//!
//! Copied from `submilli-server`'s `record/` module. The recorder hook itself is
//! the host's [`RunRecorder`](super::run::RunRecorder): the run path calls it at a
//! run's start, for each event, and at its finish.

pub(crate) mod events;
pub(crate) mod recheck;
pub(crate) mod replay;
pub(crate) mod test_run;
pub(crate) mod throwaway;

pub(crate) use recheck::RecordedRun;

#[cfg(test)]
mod tests;
