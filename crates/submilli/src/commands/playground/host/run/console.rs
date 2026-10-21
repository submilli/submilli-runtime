//! A run's console: captured in full up to 1 MiB, and reported line by line to a
//! recorded run's log as it is written.
//!
//! Copied from `submilli-server`'s `runner.rs` (`Sink`, `ConsoleLines`,
//! `captured_console`) and `handlers/execute.rs` (`split_console`).

use std::io::Write;
use std::sync::{Arc, Mutex};

use interpreter::runtime::DecisionLog;

/// The most console output one run keeps.
pub(super) const MAX_CONSOLE_BYTES: usize = 1024 * 1024;

/// The longest console line a recorded event carries; the run's console keeps the rest.
const MAX_CONSOLE_LINE_BYTES: usize = 4 * 1024;
/// The most console lines one run reports as events; the run's console keeps the rest.
const MAX_CONSOLE_LINES: usize = 1000;

// A panic during writing may leave partial console output. AGENTS.md permits
// panicking on poisoned access instead of recovering it; the panic that caused
// poisoning is still subject to the no-panic policy.
pub(super) struct Sink {
    pub(super) output: Arc<Mutex<Vec<u8>>>,
    /// Reports each line as it is written, for a recorded run.
    pub(super) lines: Option<ConsoleLines>,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.append(buf)?;
        if let Some(lines) = &mut self.lines {
            lines.feed(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn append(&self, buf: &[u8]) -> std::io::Result<()> {
        let mut output = self.output.lock().expect("console buffer lock poisoned");
        if buf.len() > MAX_CONSOLE_BYTES.saturating_sub(output.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "console output exceeds 1 MiB; print less output",
            ));
        }
        let needed = output.len() + buf.len();
        if needed > output.capacity() {
            let capacity = needed
                .max(output.capacity().saturating_mul(2))
                .min(MAX_CONSOLE_BYTES);
            let additional = capacity - output.len();
            output
                .try_reserve_exact(additional)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::OutOfMemory, error))?;
        }
        output.extend_from_slice(buf);
        Ok(())
    }
}

// Poison means a panic may have interrupted a console write. AGENTS.md permits
// poisoned-lock panics instead of treating potentially partial output as intact;
// it does not permit the panic that caused poisoning. This also applies on readback.
pub(super) fn captured_console(buf: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&buf.lock().expect("console buffer lock poisoned")).into_owned()
}

/// Splits what the console accepted into lines and reports each complete one to the run's
/// log, up to [`MAX_CONSOLE_LINES`]. `console.log` ends every line, so otherwise only
/// output cut at the console's cap leaves a line unreported.
pub(super) struct ConsoleLines {
    log: Arc<DecisionLog>,
    /// The line so far, at most [`MAX_CONSOLE_LINE_BYTES`] of it.
    pending: Vec<u8>,
    truncated: bool,
    reported: usize,
}

impl ConsoleLines {
    pub(super) fn new(log: Arc<DecisionLog>) -> Self {
        Self {
            log,
            pending: Vec::new(),
            truncated: false,
            reported: 0,
        }
    }

    fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() && self.reported < MAX_CONSOLE_LINES {
            let (part, rest, ended) = match bytes.iter().position(|byte| *byte == b'\n') {
                Some(end) => (&bytes[..end], &bytes[end + 1..], true),
                None => (bytes, &[][..], false),
            };
            let room = MAX_CONSOLE_LINE_BYTES.saturating_sub(self.pending.len());
            self.pending
                .extend_from_slice(&part[..part.len().min(room)]);
            self.truncated |= part.len() > room;
            if ended {
                self.report();
            }
            bytes = rest;
        }
    }

    fn report(&mut self) {
        // The cut may split a character: leave its start off rather than show it replaced.
        if self.truncated
            && let Err(error) = std::str::from_utf8(&self.pending)
            && error.error_len().is_none()
        {
            self.pending.truncate(error.valid_up_to());
        }
        let line = String::from_utf8_lossy(&self.pending);
        let line = line.strip_suffix('\r').unwrap_or(&line);
        self.log.console_line(line, self.truncated);
        self.pending.clear();
        self.truncated = false;
        self.reported += 1;
    }
}

/// A run's console text as the lines a response carries.
pub(super) fn split_console(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = raw
        .split('\n')
        .map(std::string::ToString::to_string)
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests;
