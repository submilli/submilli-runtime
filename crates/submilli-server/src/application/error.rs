//! Errors exposed by application persistence ports. Backend causes remain opaque.
#[derive(Debug)]
pub enum StoreError {
    AlreadyExists,
    Database(Box<dyn std::error::Error + Send + Sync>),
    Io(String),
    Credentials(String),
    Serialization(String),
    RevisionExhausted { name: String },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(f, "blueprint database failed: {error}"),
            Self::AlreadyExists => f.write_str("blueprint already exists"),
            Self::Io(message) => write!(f, "store I/O failed: {message}"),
            Self::Credentials(message) => f.write_str(message),
            Self::Serialization(message) => write!(f, "blueprint serialization failed: {message}"),
            Self::RevisionExhausted { name } => {
                write!(f, "blueprint '{name}' revision counter exhausted")
            }
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
