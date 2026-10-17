//! Externref payloads for `submilli:fs` open resources.
//!
//! Each payload owns a backend resource plus a clone of
//! `TenantLimits.host_attached_bytes` (via [`ByteCharge`]). It charges its buffer
//! footprint at creation and refunds it when the handle closes or is reclaimed.
//!
//! Readers close eagerly. Writers publish only through explicit backend close;
//! dropping a guest handle releases resources without publishing output.

use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime::OpenFileGuard;
use crate::runtime::filesystem::{FilesystemError, Invocation};
use crate::runtime::limits::{MemoryCapExceeded, TenantLimits};

pub use crate::runtime::default_filesystem::kind_of;

/// Footprint charged per reader/writer, matching the default `BufReader`/`BufWriter`
/// capacity. Byte readers add their `chunk_size` working buffer on top.
const HANDLE_BUF_BYTES: u64 = 8 * 1024;

/// Bytes charged against the store's host-attached counter, refunded on `release`/`drop`.
pub(super) struct ByteCharge {
    bytes: u64,
    counter: Arc<AtomicU64>,
}

impl ByteCharge {
    pub(super) fn new(limits: &TenantLimits, bytes: u64) -> Result<Self, MemoryCapExceeded> {
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }

    pub(super) fn grow(
        &mut self,
        limits: &TenantLimits,
        bytes: u64,
    ) -> Result<(), MemoryCapExceeded> {
        limits.charge_host_bytes(bytes)?;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    /// Refunds the charged bytes once; subsequent calls are no-ops.
    fn release(&mut self) {
        let n = std::mem::take(&mut self.bytes);
        if n == 0 {
            return;
        }
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(n))
            });
    }
}

impl Drop for ByteCharge {
    fn drop(&mut self) {
        self.release();
    }
}

/// An iterator handle with an idempotent eager close — what the shared
/// `for…of` close path calls on any of the three reader kinds.
pub trait Closable: Send + Sync {
    fn close(&mut self);
}

/// Line iterator over a file. `close()` and `Drop` both free the fd and refund bytes.
pub struct ChargedLineReader {
    reader: Option<BufReader<Box<dyn crate::runtime::filesystem::FileReader>>>,
    buf: Vec<u8>,
    bom_handled: bool,
    /// OS-read bytes (including buffered read-ahead) and scanned line bytes,
    /// retained for settlement on errors too.
    pub(super) bytes_read: u64,
    pub(super) bytes_scanned: u64,
    charge: ByteCharge,
    /// Keeps the file's bytes counted against the size limit while it is open,
    /// even after its name is removed.
    held: Option<OpenFileGuard>,
}

impl ChargedLineReader {
    pub fn new(
        reader: BufReader<Box<dyn crate::runtime::filesystem::FileReader>>,
        limits: &TenantLimits,
        held: Option<OpenFileGuard>,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            reader: Some(reader),
            buf: Vec::new(),
            bom_handled: false,
            bytes_read: 0,
            bytes_scanned: 0,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
            held,
        })
    }

    /// Read one bounded line, accounting for the retained buffer and returned
    /// text before allocation. The byte limit includes BOM and line delimiters.
    pub fn read_next(
        &mut self,
        limits: &TenantLimits,
        max_bytes: u64,
    ) -> wasmtime::Result<Option<ChargedLine>> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(None);
        };
        self.buf.clear();
        loop {
            let needs_read = reader.buffer().is_empty();
            let available = reader.fill_buf()?;
            if needs_read {
                self.bytes_read = self.bytes_read.saturating_add(available.len() as u64);
            }
            if available.is_empty() {
                break;
            }
            let newline = available.iter().position(|&byte| byte == b'\n');
            let count = newline.map_or(available.len(), |index| index + 1);
            self.bytes_scanned = self.bytes_scanned.saturating_add(count as u64);
            let length = self.buf.len().checked_add(count).ok_or_else(|| {
                crate::runtime::host::range_error("fs.lines: line length exceeds host capacity")
            })?;
            if length as u64 > max_bytes {
                return Err(crate::runtime::host::range_error(format!(
                    "fs.lines: line exceeds maxReadSize of {max_bytes} bytes; read smaller chunks with fs.bytes"
                )));
            }
            if length > self.buf.capacity() {
                let maximum = usize::try_from(max_bytes).unwrap_or(usize::MAX);
                let capacity = length
                    .max(self.buf.capacity().saturating_mul(2))
                    .min(maximum);
                self.charge
                    .grow(limits, (capacity - self.buf.capacity()) as u64)?;
                self.buf
                    .try_reserve_exact(capacity - self.buf.len())
                    .map_err(crate::runtime::host::fatal_host_error)?;
            }
            self.buf.extend_from_slice(&available[..count]);
            reader.consume(count);
            if newline.is_some() {
                break;
            }
        }
        if self.buf.is_empty() {
            return Ok(None);
        }
        if self.buf.last() == Some(&b'\n') {
            self.buf.pop();
            if self.buf.last() == Some(&b'\r') {
                self.buf.pop();
            }
        }
        // Every invalid UTF-8 byte can become a three-byte replacement character.
        let text_charge = ByteCharge::new(limits, (self.buf.len() as u64).saturating_mul(3))?;
        let mut text = String::new();
        text.try_reserve_exact(self.buf.len().saturating_mul(3))
            .map_err(crate::runtime::host::fatal_host_error)?;
        for part in self.buf.utf8_chunks() {
            text.push_str(part.valid());
            if !part.invalid().is_empty() {
                text.push('\u{FFFD}');
            }
        }
        if !self.bom_handled {
            self.bom_handled = true;
            if text.starts_with('\u{FEFF}') {
                text.drain(..'\u{FEFF}'.len_utf8());
            }
        }
        Ok(Some(ChargedLine {
            text,
            _charge: text_charge,
        }))
    }

    pub fn close(&mut self) {
        self.reader = None;
        self.buf = Vec::new();
        self.charge.release();
        self.held = None;
    }
}

/// Keeps returned native text charged until the caller finishes marshalling it.
pub struct ChargedLine {
    pub text: String,
    _charge: ByteCharge,
}

impl Closable for Option<ChargedLineReader> {
    fn close(&mut self) {
        if let Some(mut reader) = self.take() {
            reader.close();
        }
    }
}

/// Fixed-size chunk iterator over a file.
pub struct ChargedByteReader {
    file: Option<Box<dyn crate::runtime::filesystem::FileReader>>,
    chunk_size: usize,
    charge: ByteCharge,
    /// Keeps the file's bytes counted against the size limit while it is open,
    /// even after its name is removed.
    held: Option<OpenFileGuard>,
}

impl ChargedByteReader {
    pub fn new(
        file: Box<dyn crate::runtime::filesystem::FileReader>,
        chunk_size: usize,
        limits: &TenantLimits,
        held: Option<OpenFileGuard>,
    ) -> Result<Self, MemoryCapExceeded> {
        let bytes = HANDLE_BUF_BYTES.saturating_add(chunk_size as u64);
        Ok(Self {
            file: Some(file),
            chunk_size,
            charge: ByteCharge::new(limits, bytes)?,
            held,
        })
    }

    /// Next chunk of up to `chunk_size` bytes; the final chunk may be short.
    /// `Ok(None)` at EOF or once closed.
    pub fn read_next(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let Some(file) = self.file.as_mut() else {
            return Ok(None);
        };
        let mut buf = vec![0u8; self.chunk_size];
        let mut filled = 0;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            Ok(None)
        } else {
            buf.truncate(filled);
            Ok(Some(buf))
        }
    }

    pub fn close(&mut self) {
        self.file = None;
        self.charge.release();
        self.held = None;
    }
}

/// Directory walker (flat or recursive).
pub struct ChargedDirIter {
    walk: Option<Box<dyn crate::runtime::filesystem::DirectoryReader>>,
    charge: ByteCharge,
}

impl ChargedDirIter {
    pub fn new(
        walk: Box<dyn crate::runtime::filesystem::DirectoryReader>,
        limits: &TenantLimits,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES.max(walk.retained_bytes()))?,
            walk: Some(walk),
        })
    }

    pub fn next_entry(&mut self) -> Option<crate::runtime::filesystem::DirectoryEntry> {
        self.walk.as_mut()?.next_entry()
    }

    pub fn close(&mut self) {
        self.walk = None;
        self.charge.release();
    }
}

/// A guest writer holds only the selected backend and its bounded host buffer.
pub struct BackendFileWriter {
    writer: Box<dyn crate::runtime::filesystem::FileWriter>,
    charge: ByteCharge,
}
impl BackendFileWriter {
    pub fn new(
        writer: Box<dyn crate::runtime::filesystem::FileWriter>,
        limits: &TenantLimits,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            writer,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
        })
    }
    pub async fn write(
        &mut self,
        invocation: &Invocation,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        self.writer.write(invocation, bytes).await
    }
    pub async fn close(&mut self, invocation: &Invocation) -> Result<(), FilesystemError> {
        let result = self.writer.close(invocation).await;
        if self.writer.is_closed() {
            self.charge.release();
        }
        result
    }
}

impl Closable for ChargedLineReader {
    fn close(&mut self) {
        ChargedLineReader::close(self);
    }
}

impl Closable for ChargedByteReader {
    fn close(&mut self) {
        ChargedByteReader::close(self);
    }
}

impl Closable for ChargedDirIter {
    fn close(&mut self) {
        ChargedDirIter::close(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[tokio::test]
    async fn failed_native_backend_close_refunds_a_discarded_buffer() {
        use crate::runtime::default_filesystem::DefaultFilesystem;
        use crate::runtime::filesystem::{Filesystem, Invocation};
        let root = tempfile::tempdir().unwrap();
        let filesystem = DefaultFilesystem::new(
            crate::runtime::Vfs::external(root.path().to_path_buf()).unwrap(),
        );
        let invocation = Invocation::default();
        let limits = TenantLimits::new(64 * 1024);
        let writer = filesystem
            .open_writer(&invocation, "/target")
            .await
            .unwrap();
        let mut handle = BackendFileWriter::new(writer, &limits).unwrap();
        handle.write(&invocation, b"data".to_vec()).await.unwrap();
        std::fs::create_dir(root.path().join("target")).unwrap();
        assert!(handle.close(&invocation).await.is_err());
        assert_eq!(limits.host_attached_bytes(), 0);
        assert!(handle.close(&invocation).await.is_ok());
    }

    fn temp_file() -> File {
        tempfile::tempfile().expect("anonymous temp file")
    }

    fn line_reader(contents: &[u8], limits: &TenantLimits) -> ChargedLineReader {
        use std::io::{Seek, SeekFrom};
        let mut file = temp_file();
        file.write_all(contents).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        ChargedLineReader::new(BufReader::new(Box::new(file)), limits, None).unwrap()
    }

    #[test]
    fn line_limit_includes_delimiters_and_bounds_retained_memory() {
        let limits = TenantLimits::new(1024 * 1024);
        let mut reader = line_reader(b"abc\nx", &limits);
        let line = reader.read_next(&limits, 4).unwrap().unwrap();
        assert_eq!(line.text, "abc");
        assert_eq!(reader.bytes_read, 5);
        assert!(limits.host_attached_bytes() >= HANDLE_BUF_BYTES + 4 + 9);
        drop(line);
        assert_eq!(reader.read_next(&limits, 4).unwrap().unwrap().text, "x");
        reader.close();
        assert_eq!(limits.host_attached_bytes(), 0);

        let mut too_long = line_reader(b"abcde", &limits);
        let error = too_long.read_next(&limits, 4).err().unwrap();
        assert!(error.to_string().contains("maxReadSize"));
        assert_eq!(too_long.buf.capacity(), 0);
        assert_eq!(too_long.bytes_read, 5);
    }

    #[test]
    fn line_buffer_admission_uses_the_current_tenant_memory() {
        let limits = TenantLimits::new(HANDLE_BUF_BYTES + 4);
        let mut reader = line_reader(b"abcd", &limits);
        // The input fits, but its returned String must also be admitted.
        let error = reader.read_next(&limits, 4).err().unwrap();
        assert!(error.is::<MemoryCapExceeded>());
        assert_eq!(reader.bytes_read, 4);
        reader.close();
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn line_decoding_preserves_bom_crlf_and_lossy_utf8_behavior() {
        let limits = TenantLimits::new(1024 * 1024);
        let mut reader = line_reader(b"\xef\xbb\xbfhi\r\n\xff\n", &limits);
        assert_eq!(reader.read_next(&limits, 32).unwrap().unwrap().text, "hi");
        assert_eq!(
            reader.read_next(&limits, 32).unwrap().unwrap().text,
            "\u{fffd}"
        );
        assert!(reader.read_next(&limits, 32).unwrap().is_none());
    }

    #[test]
    fn drop_releases_bytes_back_to_limits() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let reader =
            ChargedLineReader::new(BufReader::new(Box::new(temp_file())), &limits, None).unwrap();
        assert!(limits.host_attached_bytes() > 0, "new should charge bytes");
        drop(reader);
        assert_eq!(
            limits.host_attached_bytes(),
            0,
            "Drop must refund the charged bytes",
        );
    }

    #[test]
    fn drop_releases_one_of_many_proportionally() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let r1 =
            ChargedLineReader::new(BufReader::new(Box::new(temp_file())), &limits, None).unwrap();
        let r2 = ChargedByteReader::new(Box::new(temp_file()), 4096, &limits, None).unwrap();
        let total = limits.host_attached_bytes();
        assert_eq!(total, HANDLE_BUF_BYTES + (HANDLE_BUF_BYTES + 4096));
        drop(r1);
        assert_eq!(limits.host_attached_bytes(), HANDLE_BUF_BYTES + 4096);
        drop(r2);
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn close_refunds_eagerly_and_is_idempotent() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let mut reader =
            ChargedLineReader::new(BufReader::new(Box::new(temp_file())), &limits, None).unwrap();
        assert!(limits.host_attached_bytes() > 0);
        reader.close();
        assert_eq!(limits.host_attached_bytes(), 0, "close refunds immediately");
        reader.close();
        drop(reader);
        assert_eq!(limits.host_attached_bytes(), 0, "no double refund");
    }

    async fn writer_over(vfs: &crate::runtime::Vfs, limits: &TenantLimits) -> BackendFileWriter {
        use crate::runtime::default_filesystem::DefaultFilesystem;
        use crate::runtime::filesystem::Filesystem;
        let writer = DefaultFilesystem::new(vfs.clone())
            .open_writer(&Invocation::default(), "/out.txt")
            .await
            .unwrap();
        BackendFileWriter::new(writer, limits).unwrap()
    }

    fn temp_leftovers(root: &std::path::Path) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().starts_with("out.txt."))
            .collect()
    }

    #[tokio::test]
    async fn unclosed_writer_drop_removes_temp_and_skips_rename() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let mut w = writer_over(&vfs, &limits).await;
        w.write(&Invocation::default(), b"hello\n".to_vec())
            .await
            .unwrap();
        assert!(limits.host_attached_bytes() > 0);
        drop(w);
        assert!(
            temp_leftovers(dir.path()).is_empty(),
            "temp file removed on drop"
        );
        assert!(
            !dir.path().join("out.txt").exists(),
            "no rename without explicit close"
        );
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[tokio::test]
    async fn writer_close_renames_and_refunds() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let final_path = dir.path().join("out.txt");
        let mut w = writer_over(&vfs, &limits).await;
        w.write(&Invocation::default(), b"hello\n".to_vec())
            .await
            .unwrap();
        w.close(&Invocation::default()).await.unwrap();
        assert!(final_path.exists());
        assert!(temp_leftovers(dir.path()).is_empty());
        assert_eq!(limits.host_attached_bytes(), 0);
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "hello\n");
    }
}
