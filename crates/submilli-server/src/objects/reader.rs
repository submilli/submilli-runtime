//! Bounded bridge from authenticated object streaming to blocking filesystem
//! writes. Dropping the consumer wakes a blocked producer; the producer keeps
//! the object's access guard until its I/O has drained.
use super::*;
use std::io::{self, Cursor};
use std::sync::mpsc::{self, Receiver, SyncSender};

enum Chunk {
    Data(Vec<u8>),
    End,
    Failed(String),
}
struct Sender(SyncSender<Chunk>);
struct Reader {
    receiver: Receiver<Chunk>,
    current: Cursor<Vec<u8>>,
    ended: bool,
}

impl DurableObjects {
    pub(crate) fn stream(&self, reference: ObjectReference) -> Box<dyn Read + Send> {
        let (sender, receiver) = mpsc::sync_channel(2);
        let objects = self.clone();
        tokio::spawn(async move {
            let result = objects.copy_into(&reference, Sender(sender.clone())).await;
            // The final bounded send may block too. Keep it off async workers.
            let _ = tokio::task::spawn_blocking(move || {
                let end = match result {
                    Ok(()) => Chunk::End,
                    Err(error) => Chunk::Failed(error.to_string()),
                };
                let _ = sender.send(end);
            })
            .await;
        });
        Box::new(Reader {
            receiver,
            current: Cursor::new(Vec::new()),
            ended: false,
        })
    }
}

impl Write for Sender {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for chunk in bytes.chunks(64 * 1024) {
            let mut buffer = Vec::new();
            buffer
                .try_reserve_exact(chunk.len())
                .map_err(io::Error::other)?;
            buffer.extend_from_slice(chunk);
            self.0
                .send(Chunk::Data(buffer))
                .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Reader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            let read = self.current.read(bytes)?;
            if read != 0 || self.ended {
                return Ok(read);
            }
            match self
                .receiver
                .recv()
                .map_err(|_| io::Error::other("object stream stopped before completion"))?
            {
                Chunk::Data(data) => self.current = Cursor::new(data),
                Chunk::End => self.ended = true,
                Chunk::Failed(error) => return Err(io::Error::other(error)),
            }
        }
    }
}
