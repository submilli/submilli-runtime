use super::*;
use std::io::Cursor;

/// Bytes supplied by execution/replay for one invocation, never persisted here.
/// Stream implementations must return the same authenticated bytes on each open.
/// Filesystem staging is output-in-progress, not an input archive.
#[derive(Clone)]
pub struct ReplayInput {
    content: Content,
    open: Arc<dyn Fn() -> Result<Box<dyn Read + Send>, RecoveryError> + Send + Sync>,
}
impl ReplayInput {
    pub fn bytes(id: String, bytes: Vec<u8>) -> Result<Self, RecoveryError> {
        crate::recovery::paths::validate_id(&id)?;
        let content = Content {
            id,
            bytes: bytes.len() as u64,
            sha256: Sha256::digest(&bytes).into(),
        };
        let bytes = SharedBytes(Arc::new(bytes));
        Ok(Self {
            content,
            open: Arc::new(move || Ok(Box::new(Cursor::new(bytes.clone())))),
        })
    }
    pub fn content(&self) -> Content {
        self.content.clone()
    }
    pub(crate) fn staged(
        content: Content,
        open: impl Fn() -> Result<Box<dyn Read + Send>, RecoveryError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            content,
            open: Arc::new(open),
        }
    }
}
impl InputBytes for Option<ReplayInput> {
    fn open(&mut self, content: &Content) -> Result<Box<dyn Read + Send>, RecoveryError> {
        match self {
            Some(input) if input.content == *content => (input.open)(),
            Some(_) => Err(RecoveryError::Conflict),
            None => Err(RecoveryError::ReplayRequired),
        }
    }
}

#[derive(Clone)]
struct SharedBytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}
