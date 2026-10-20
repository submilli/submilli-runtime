//! Record native outcomes before exposing them to guest code.
//!
//! Argument conversion (including guest callbacks) runs before `execute`; result
//! conversion runs afterwards. The history port owns ordering and persistence.
//! An absent history keeps ordinary execution direct. No Wasm handles are encoded.
use std::io::Write;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use wasmtime::Caller;

use super::StoreData;
use super::durable::EffectId;
use super::fuel;
use super::host::{self, fatal_host_error};

/// Independent of the filesystem journal's much smaller metadata limit.
pub const MAX_OUTCOME_BYTES: usize = 64 * 1024 * 1024;
pub const CODEC_VERSION: u32 = 4;

#[derive(Debug, thiserror::Error)]
#[error("durable execution: {0}")]
pub struct HistoryError(pub String);

/// A call's identity is allocated by the history implementation, not the interpreter.
pub struct EffectCall {
    pub id: EffectId,
    pub sequence: u64,
    pub recorded: Option<RecordedOutcome>,
}

pub type RecordedOutcome = Result<OutcomeBytes, NativeError>;

/// Encoded bytes use MessagePack's binary representation, never an integer array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutcomeBytes(#[serde(with = "binary")] pub Vec<u8>);

/// Implementations must serialize calls and commit before `complete` returns.
/// A missing outcome is an instruction to execute, regardless of idempotency.
#[async_trait::async_trait]
pub trait EffectHistory: Send + Sync {
    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<EffectCall, HistoryError>;
    async fn complete(
        &self,
        call: &EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), HistoryError>;
    /// Refuse an execution which ended before consuming all its recorded calls.
    async fn finish(&self) -> Result<(), HistoryError>;
}

/// Execute one native action, or return its committed native outcome.
///
/// `arguments` are hashed in memory only. The action owns them, avoiding copies
/// of large requests. It must not call guest code or manufacture guest exceptions:
/// return native values/errors and convert them after this boundary instead.
pub async fn execute<'store, A, T, F>(
    caller: &mut Caller<'store, StoreData>,
    operation: &'static str,
    arguments: A,
    action: F,
) -> wasmtime::Result<T>
where
    A: Serialize + Send + 'static,
    T: Serialize + DeserializeOwned + Send + 'static,
    F: for<'call> FnOnce(
            &'call mut Caller<'store, StoreData>,
            EffectId,
            A,
        ) -> BoxFuture<'call, wasmtime::Result<T>>
        + Send,
{
    let Some(history) = caller.data().effects.clone() else {
        let result = action(caller, EffectId::stub()?, arguments).await;
        return fuel::settle_result(caller, |caller| {
            result.map_err(|error| host::throw_host_error(caller, error))
        });
    };
    let fingerprint = fingerprint(caller, &arguments)?;
    let call = history
        .begin(operation, fingerprint)
        .await
        .map_err(fatal_host_error)?;
    if let Some(recorded) = &call.recorded {
        return match recorded {
            Ok(bytes) => {
                fuel::charge(&mut *caller, fuel::PARSE, bytes.0.len() as u64)?;
                decode(&bytes.0)
            }
            Err(error) => fuel::settle_result(caller, |caller| {
                Err(host::throw_host_error(caller, error.clone().into_error()))
            }),
        };
    }

    let result = action(caller, call.id.clone(), arguments).await;
    if let Err(error) = &result
        && (host::ends_the_run(error) || error.is::<wasmtime::ThrownException>())
    {
        return result;
    }
    let recorded_result = match &result {
        Ok(value) => {
            let encoded = encode(value)?;
            // Do not commit a representation the replay decoder refuses structurally.
            validate_outcome(&encoded)?;
            fuel::settle(&mut *caller, fuel::COPY, encoded.len() as u64)?;
            Ok(OutcomeBytes(encoded))
        }
        Err(error) => Err(NativeError::capture(error)),
    };
    history
        .complete(&call, recorded_result)
        .await
        .map_err(fatal_host_error)?;
    fuel::settle_result(caller, |caller| {
        result.map_err(|error| host::throw_host_error(caller, error))
    })
}

pub(crate) mod binary {
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> serde::de::Visitor<'de> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("binary outcome")
            }
            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
                Ok(bytes.to_vec())
            }
        }
        deserializer.deserialize_bytes(Bytes)
    }
}

fn fingerprint<A: Serialize>(
    caller: &mut Caller<'_, StoreData>,
    arguments: &A,
) -> wasmtime::Result<[u8; 32]> {
    let mut writer = FingerprintWriter {
        caller,
        hash: Sha256::new(),
        failure: None,
    };
    let result =
        arguments.serialize(&mut rmp_serde::Serializer::new(&mut writer).with_struct_map());
    if let Some(error) = writer.failure {
        return Err(error);
    }
    result.map_err(fatal_host_error)?;
    Ok(writer.hash.finalize().into())
}

struct FingerprintWriter<'a, 'store> {
    caller: &'a mut Caller<'store, StoreData>,
    hash: Sha256,
    failure: Option<wasmtime::Error>,
}
impl Write for FingerprintWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = fuel::charge(&mut *self.caller, fuel::HASH, bytes.len() as u64) {
            self.failure = Some(error);
            return Err(std::io::Error::other("effect fingerprint exceeded fuel"));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode<T: Serialize>(value: &T) -> wasmtime::Result<Vec<u8>> {
    let mut writer = OutcomeWriter { bytes: Vec::new() };
    value
        .serialize(&mut rmp_serde::Serializer::new(&mut writer).with_struct_map())
        .map_err(fatal_host_error)?;
    Ok(writer.bytes)
}
struct OutcomeWriter {
    bytes: Vec<u8>,
}
impl Write for OutcomeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_OUTCOME_BYTES)
            .ok_or_else(|| std::io::Error::other("native outcome exceeds history limit"))?;
        if length > self.bytes.capacity() {
            let capacity = length
                .max(self.bytes.capacity().saturating_mul(2))
                .min(MAX_OUTCOME_BYTES);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(std::io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn validate_outcome(bytes: &[u8]) -> wasmtime::Result<()> {
    if bytes.len() > MAX_OUTCOME_BYTES {
        return Err(fatal_host_error("native outcome exceeds history limit"));
    }
    // Validate lengths, nesting and complete consumption before deserialization.
    super::effects_memory::decoded_heap_bound(bytes, 1, false)?;
    Ok(())
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> wasmtime::Result<T> {
    validate_outcome(bytes)?;
    let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(bytes));
    decoder.set_max_depth(128);
    let value = T::deserialize(&mut decoder).map_err(fatal_host_error)?;
    if decoder.position() != bytes.len() as u64 {
        return Err(fatal_host_error("trailing native outcome bytes"));
    }
    Ok(value)
}

/// Guest-visible error data, captured before the host wrapper raises an exception.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeError {
    Error(String),
    Range(String),
    QuotaExceeded(String),
    Timeout(String),
    Type(String),
    Syntax(String),
    Uri(String),
    PermissionDenied(host::PermissionDenied),
}
impl NativeError {
    pub(crate) fn capture(error: &wasmtime::Error) -> Self {
        let message = error.to_string();
        if let Some(denial) = error.downcast_ref::<host::PermissionDenied>() {
            return Self::PermissionDenied(denial.clone());
        }
        if error.is::<host::RangeError>() {
            return Self::Range(message);
        }
        if error.is::<host::QuotaExceededError>() {
            return Self::QuotaExceeded(message);
        }
        if error.is::<host::TimeoutError>() {
            return Self::Timeout(message);
        }
        if error.is::<host::TypeError>() {
            return Self::Type(message);
        }
        if error.is::<host::SyntaxError>() {
            return Self::Syntax(message);
        }
        if error.is::<host::UriError>() {
            return Self::Uri(message);
        }
        Self::Error(message)
    }
    pub(crate) fn into_error(self) -> wasmtime::Error {
        match self {
            Self::Error(message) => wasmtime::Error::msg(message),
            Self::Range(message) => wasmtime::Error::new(host::RangeError(message)),
            Self::QuotaExceeded(message) => wasmtime::Error::new(host::QuotaExceededError(message)),
            Self::Timeout(message) => wasmtime::Error::new(host::TimeoutError(message)),
            Self::Type(message) => wasmtime::Error::new(host::TypeError(message)),
            Self::Syntax(message) => wasmtime::Error::new(host::SyntaxError(message)),
            Self::Uri(message) => wasmtime::Error::new(host::UriError(message)),
            Self::PermissionDenied(denial) => wasmtime::Error::new(denial),
        }
    }
}

/// Validate a stored MessagePack envelope before Serde sees allocation hints.
/// The caller must bound the blob size before validating the encoded structure.
/// `element_bytes` is the largest sequence element in the persistence schema.
pub fn stored_heap_bound(
    bytes: &[u8],
    element_bytes: usize,
    dynamic_maps: bool,
) -> Result<u64, HistoryError> {
    super::effects_memory::decoded_heap_bound(bytes, element_bytes, dynamic_maps)
        .map_err(|error| HistoryError(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_floats_keep_nan_bits_and_signed_zero() {
        for bits in [
            0x8000_0000_0000_0000,
            0x7ff8_0000_0000_1234,
            0x7ff0_0000_0000_0000,
        ] {
            let encoded = encode(&f64::from_bits(bits)).unwrap();
            let restored: f64 = rmp_serde::from_slice(&encoded).unwrap();
            assert_eq!(restored.to_bits(), bits);
        }
    }
    #[test]
    fn binary_is_not_expanded_into_integer_elements() {
        let value = OutcomeBytes(vec![255; 4096]);
        let encoded = encode(&value).unwrap();
        assert!(encoded.len() < 4100);
        let restored: OutcomeBytes = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(restored, value);
        drop(encoded);
    }
    #[test]
    fn missing_and_empty_native_strings_remain_distinct() {
        for value in [None, Some(String::new()), Some("text".to_owned())] {
            let encoded = encode(&value).unwrap();
            let restored: Option<String> = rmp_serde::from_slice(&encoded).unwrap();
            assert_eq!(restored, value);
        }
    }
}
