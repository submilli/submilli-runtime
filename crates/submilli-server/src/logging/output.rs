use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// Clones share a descriptor, so SIGHUP affects every subsequent record.
#[derive(Clone)]
pub struct LogOutput {
    inner: Arc<Mutex<Option<File>>>,
    path: Option<PathBuf>,
}

impl LogOutput {
    pub fn open(path: Option<PathBuf>) -> io::Result<Self> {
        let file = path.as_deref().map(open_file).transpose()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(file)),
            path,
        })
    }

    /// Audit collection is best-effort even when its destination cannot open.
    pub fn best_effort_open(path: Option<PathBuf>) -> Self {
        if let Ok(output) = Self::open(path.clone()) {
            output
        } else {
            report("cannot open audit output; subsequent records will retry");
            Self {
                inner: Arc::new(Mutex::new(None)),
                path,
            }
        }
    }

    pub fn reopen(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let replacement = open_file(path)?;
        *self
            .inner
            .lock()
            .map_err(|_| io::Error::other("log output lock poisoned"))? = Some(replacement);
        Ok(())
    }

    /// Writes an already encoded record, also available to future audit callers.
    pub fn write_record(&self, record: &str) -> io::Result<()> {
        self.write_bytes(record.as_bytes())
    }

    fn write_bytes(&self, bytes: &[u8]) -> io::Result<()> {
        let mut output = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("log output lock poisoned"))?;
        if output.is_none()
            && let Some(path) = &self.path
        {
            *output = Some(open_file(path)?);
        }
        let result = match output.as_mut() {
            Some(file) => file.write_all(bytes),
            None => io::stdout().lock().write_all(bytes),
        };
        if let Err(error) = &result {
            report(&format!(
                "cannot write server log to {}: {error}",
                self.path
                    .as_deref()
                    .map_or_else(|| "stdout".into(), |p| p.display().to_string())
            ));
        }
        result
    }
}

fn open_file(path: &Path) -> io::Result<File> {
    let open = || {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // A rotated path might become a FIFO. Opening nonblocking lets us
            // reject it rather than hanging this thread and shutdown's join.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log destination must be a regular file",
            ));
        }
        Ok(file)
    };
    open().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot open server log {}: {error}", path.display()),
        )
    })
}

/// tracing-subscriber submits each formatted event with one `write_all` call.
/// Complete writes occur under the output lock, including partial OS writes.
pub struct EventWriter(LogOutput);

impl<'a> MakeWriter<'a> for LogOutput {
    type Writer = EventWriter;
    fn make_writer(&'a self) -> Self::Writer {
        EventWriter(self.clone())
    }
}

impl Write for EventWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write_bytes(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Owns a fallibly spawned signal thread, independent of Tokio's reactor.
/// Reopening may touch disk, so it must not block an async runtime worker.
pub struct ReopenTask {
    #[cfg(unix)]
    stop: signal_hook::iterator::Handle,
    #[cfg(unix)]
    task: Option<std::thread::JoinHandle<io::Result<()>>>,
}

impl ReopenTask {
    #[cfg(unix)]
    pub fn start(output: LogOutput) -> io::Result<Self> {
        use signal_hook::iterator::{backend::SignalDelivery, exfiltrator::SignalOnly};
        let (read, write) = std::os::unix::net::UnixStream::pair()?;
        let mut signals =
            SignalDelivery::with_pipe(read, write, SignalOnly, [signal_hook::consts::SIGHUP])?;
        let stop = signals.handle();
        let task = std::thread::Builder::new()
            .name("server-log-reopen".into())
            .spawn(move || {
                let result = reopen_loop(&mut signals, &output);
                if let Err(error) = &result {
                    report(&format!("server log signal listener failed: {error}"));
                }
                result
            })?;
        Ok(Self {
            stop,
            task: Some(task),
        })
    }

    #[cfg(not(unix))]
    pub fn start(_output: LogOutput) -> io::Result<Self> {
        Ok(Self {})
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.finish()
    }

    fn finish(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.stop.close();
            if let Some(task) = self.task.take() {
                task.join()
                    .map_err(|_| io::Error::other("server log reopen thread failed"))??;
            }
        }
        Ok(())
    }
}

impl Drop for ReopenTask {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            report(&format!("cannot stop server log signal listener: {error}"));
        }
    }
}

#[cfg(unix)]
fn reopen_loop(
    signals: &mut signal_hook::iterator::backend::SignalDelivery<
        std::os::unix::net::UnixStream,
        signal_hook::iterator::exfiltrator::SignalOnly,
    >,
    output: &LogOutput,
) -> io::Result<()> {
    // The convenience blocking iterator panics on IO failure; use the fallible
    // backend so a broken self-pipe reaches the caller as a typed error.
    while !signals.handle().is_closed() {
        if let Some(pending) = signals.poll_pending(&mut read_signal)? {
            for _ in pending {
                if let Err(error) = output.reopen() {
                    report(&format!("cannot reopen server log: {error}"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn read_signal(read: &mut std::os::unix::net::UnixStream) -> io::Result<bool> {
    use std::io::Read;
    loop {
        match read.read(&mut [0]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "server log signal pipe closed",
                ));
            }
            result => return result.map(|_| true),
        }
    }
}

pub(super) fn report(message: &str) {
    // Reporting through tracing would recurse into the broken sink. stderr
    // errors cannot be reported further, and must never panic during cleanup.
    let _ = writeln!(io::stderr().lock(), "{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_failures_are_reported_and_returned() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.log");
        std::fs::write(&path, "").unwrap();
        let output = LogOutput {
            inner: Arc::new(Mutex::new(Some(File::open(&path).unwrap()))),
            path: Some(path),
        };
        assert!(output.write_record("record\n").is_err());
    }
}
