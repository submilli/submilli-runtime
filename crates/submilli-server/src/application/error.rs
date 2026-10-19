//! Errors exposed by application persistence ports. Backend causes remain opaque.
#[derive(Debug)]
pub enum StoreError {
    AlreadyExists,
    Database(Box<dyn std::error::Error + Send + Sync>),
    Io(String),
    Credentials(String),
    Serialization(String),
    RevisionExhausted {
        name: String,
    },
    /// The row changed since it was read; nothing was written.
    Conflict,
    /// The store refused new work because too much is already waiting; nothing
    /// was done, so the caller may try again.
    Busy,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database failed: {error}"),
            Self::AlreadyExists => f.write_str("blueprint already exists"),
            Self::Io(message) => write!(f, "store I/O failed: {message}"),
            Self::Credentials(message) => f.write_str(message),
            Self::Serialization(message) => write!(f, "blueprint serialization failed: {message}"),
            Self::RevisionExhausted { name } => {
                write!(f, "blueprint '{name}' revision counter exhausted")
            }
            Self::Conflict => f.write_str("the record changed since it was read"),
            Self::Busy => f.write_str("the database has too much work waiting"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
