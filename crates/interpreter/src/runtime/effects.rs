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
use super::limits::{HostBytes, HostMemory, TenantLimits};

/// Independent of the filesystem journal's much smaller metadata limit.
pub const MAX_OUTCOME_BYTES: usize = 64 * 1024 * 1024;
pub const CODEC_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
#[error("durable execution: {0}")]
pub struct HistoryError(pub String);

/// A call's identity is allocated by the history implementation, not the interpreter.
pub struct EffectCall {
    pub id: EffectId,
    pub sequence: u64,
    pub recorded: Option<RecordedOutcome>,
    /// This call consumes the last outcome in the committed replay prefix.
    pub replay_complete: bool,
    pub memory: Option<HostBytes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedOutcome {
    pub result: Result<OutcomeBytes, NativeError>,
    /// Logical native work, excluding guest argument and result conversion.
    pub host_fuel: u64,
    pub execution_time: std::time::Duration,
    pub llm_spend: TokenSpend,
    pub embedding_spend: TokenSpend,
}

/// Logical per-run spend; replay never reserves these tokens against the provider.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSpend {
    pub used: u64,
    pub held: u64,
    pub requests: u64,
}
/// Encoded bytes use MessagePack's binary representation, never an integer array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutcomeBytes(#[serde(with = "binary")] pub Vec<u8>);

/// A reviewed native representation with a bound on its decoded working heap.
///
/// Do not implement this for arbitrary Serde types. Each implementation must bound
/// allocations from untrusted encoded data, including collection capacity, and
/// preserve missing versus null (use a tagged enum for nullable optional fields).
/// No heap pointers, guest handles, or unbounded custom deserializers belong here.
pub trait NativeOutcome: Serialize + DeserializeOwned + Send + 'static {
    /// Size of the largest owned sequence element, or zero when there are none.
    const SEQUENCE_ELEMENT_BYTES: usize = 0;
    /// Dynamic maps own tree nodes; fixed Serde structs and enums do not.
    const DYNAMIC_MAPS: bool = false;
    /// Temporary buffers a custom serializer creates before writing its bytes.
    fn encoding_scratch(&self) -> u64 {
        0
    }
    fn decoded_heap_bound(encoded_bytes: usize) -> u64;
}
/// Keeps replay's native allocation charged until guest conversion finishes.
pub struct NativeResult<T> {
    pub value: T,
    pub replayed: bool,
    _memory: Option<HostBytes>,
}
impl<T> std::ops::Deref for NativeResult<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

/// Implementations must serialize calls and commit before `complete` returns.
/// A missing outcome is an instruction to execute, regardless of idempotency.
#[async_trait::async_trait]
pub trait EffectHistory: Send + Sync {
    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
        memory: HostMemory,
    ) -> Result<EffectCall, HistoryError>;
    async fn complete(
        &self,
        call: &EffectCall,
        outcome: RecordedOutcome,
        memory: HostMemory,
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
) -> wasmtime::Result<NativeResult<T>>
where
    A: Serialize + Send + 'static,
    T: NativeOutcome,
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
            result
                .map(|value| NativeResult {
                    value,
                    replayed: false,
                    _memory: None,
                })
                .map_err(|error| host::throw_host_error(caller, error))
        });
    };
    let fingerprint = fingerprint(caller, &arguments)?;
    let call = history
        .begin(
            operation,
            fingerprint,
            caller.data().tenant_limits.host_memory(),
        )
        .await
        .map_err(fatal_host_error)?;
    if let Some(recorded) = &call.recorded {
        let paused = caller.data().clock_paused;
        if let Some(clock) = &mut caller.data_mut().execution_clock {
            clock.restore(recorded.execution_time, paused);
            if call.replay_complete {
                clock.begin_live();
            }
        }
        fuel::settle_host_fuel(&mut *caller, recorded.host_fuel)?;
        if let Some(budget) = &caller.data().llm_budget {
            budget.restore_spend(recorded.llm_spend.used, recorded.llm_spend.held);
        }
        if let Some(budget) = &caller.data().embedding_budget {
            budget.restore_spend(
                recorded.embedding_spend.used,
                recorded.embedding_spend.held,
                recorded.embedding_spend.requests,
            );
        }
        return match &recorded.result {
            Ok(bytes) => decode(caller, &bytes.0),
            Err(error) => fuel::settle_result(caller, |caller| {
                Err(host::throw_host_error(caller, error.clone().into_error()))
            }),
        };
    }

    if let Some(clock) = &mut caller.data_mut().execution_clock {
        clock.begin_live();
    }
    let fuel_before = caller.data().host_fuel;
    let llm_before = llm_spend(caller.data());
    let embedding_before = embedding_spend(caller.data());
    let result = action(caller, call.id.clone(), arguments).await;
    if let Err(error) = &result
        && (host::ends_the_run(error) || error.is::<wasmtime::ThrownException>())
    {
        return result.map(|value| NativeResult {
            value,
            replayed: false,
            _memory: None,
        });
    }
    let mut encoded_memory = None;
    let recorded_result = match &result {
        Ok(value) => {
            let encoded = encode(&caller.data().tenant_limits, value)?;
            // Do not commit a representation the replay decoder refuses structurally.
            let replay_heap = super::effects_memory::decoded_heap_bound(
                &encoded.bytes,
                T::SEQUENCE_ELEMENT_BYTES,
                T::DYNAMIC_MAPS,
            )?;
            // Admission must also cover replay's owned envelope and decoded value.
            let replay_admission = HostBytes::new(
                &caller.data().tenant_limits,
                replay_heap
                    .max(T::decoded_heap_bound(encoded.bytes.len()))
                    .saturating_add(1024),
            )?;
            drop(replay_admission);
            fuel::settle(&mut *caller, fuel::COPY, encoded.bytes.len() as u64)?;
            encoded_memory = Some(encoded.memory);
            Ok(OutcomeBytes(encoded.bytes))
        }
        Err(error) => Err(NativeError::capture(error)),
    };
    let outcome = RecordedOutcome {
        result: recorded_result,
        host_fuel: caller.data().host_fuel.saturating_sub(fuel_before),
        execution_time: caller
            .data()
            .execution_clock
            .as_ref()
            .map_or(std::time::Duration::ZERO, |clock| {
                clock.elapsed(caller.data().clock_paused)
            }),
        llm_spend: llm_spend(caller.data()).since(&llm_before)?,
        embedding_spend: embedding_spend(caller.data()).since(&embedding_before)?,
    };
    history
        .complete(&call, outcome, caller.data().tenant_limits.host_memory())
        .await
        .map_err(fatal_host_error)?;
    drop(encoded_memory);
    fuel::settle_result(caller, |caller| {
        result
            .map(|value| NativeResult {
                value,
                replayed: false,
                _memory: None,
            })
            .map_err(|error| host::throw_host_error(caller, error))
    })
}

/// Convert an observed result to guest values without losing an ordinary error
/// while allocating its exception at the end of the fuel budget.
pub(crate) fn convert_result<T>(
    caller: &mut Caller<'_, StoreData>,
    convert: impl FnOnce(&mut Caller<'_, StoreData>) -> wasmtime::Result<T>,
) -> wasmtime::Result<T> {
    fuel::settle_result(caller, |caller| {
        convert(caller).map_err(|error| host::throw_host_error(caller, error))
    })
}

impl TokenSpend {
    fn since(&self, before: &Self) -> wasmtime::Result<Self> {
        let used = self
            .used
            .checked_sub(before.used)
            .ok_or_else(|| fatal_host_error("native call reduced prior token spend"))?;
        let held = self
            .held
            .checked_sub(before.held)
            .ok_or_else(|| fatal_host_error("native call reduced prior held tokens"))?;
        let requests = self
            .requests
            .checked_sub(before.requests)
            .ok_or_else(|| fatal_host_error("native call reduced prior request count"))?;
        Ok(Self {
            used,
            held,
            requests,
        })
    }
}
fn llm_spend(data: &StoreData) -> TokenSpend {
    data.llm_budget
        .as_ref()
        .map_or_else(TokenSpend::default, |budget| TokenSpend {
            used: budget.used(),
            held: budget.held(),
            requests: 0,
        })
}
fn embedding_spend(data: &StoreData) -> TokenSpend {
    data.embedding_budget
        .as_ref()
        .map_or_else(TokenSpend::default, |budget| TokenSpend {
            used: budget.used(),
            held: budget.held(),
            requests: budget.requests(),
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

impl NativeOutcome for () {
    fn decoded_heap_bound(_: usize) -> u64 {
        0
    }
}
impl NativeOutcome for bool {
    fn decoded_heap_bound(_: usize) -> u64 {
        0
    }
}
impl NativeOutcome for u64 {
    fn decoded_heap_bound(_: usize) -> u64 {
        0
    }
}
impl NativeOutcome for f64 {
    fn decoded_heap_bound(_: usize) -> u64 {
        0
    }
}
impl NativeOutcome for String {
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
impl NativeOutcome for Option<String> {
    // A present string cannot be confused with MessagePack nil.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
impl NativeOutcome for OutcomeBytes {
    // The binary visitor copies one validated slice, with no sequence preallocation.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
impl NativeOutcome for jiff::Timestamp {
    // Timestamp parses borrowed text into two integers, without retaining heap data.
    fn decoded_heap_bound(_: usize) -> u64 {
        0
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

struct EncodedOutcome {
    bytes: Vec<u8>,
    memory: HostBytes,
}
fn encode<T: NativeOutcome>(limits: &TenantLimits, value: &T) -> wasmtime::Result<EncodedOutcome> {
    let _scratch = HostBytes::new(limits, value.encoding_scratch())?;
    let mut writer = OutcomeWriter {
        limits,
        bytes: Vec::new(),
        memory: HostBytes::new(limits, 0)?,
    };
    value
        .serialize(&mut rmp_serde::Serializer::new(&mut writer).with_struct_map())
        .map_err(fatal_host_error)?;
    Ok(EncodedOutcome {
        bytes: writer.bytes,
        memory: writer.memory,
    })
}
struct OutcomeWriter<'a> {
    limits: &'a TenantLimits,
    bytes: Vec<u8>,
    memory: HostBytes,
}
impl Write for OutcomeWriter<'_> {
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
            self.memory
                .grow(self.limits, (capacity - self.bytes.capacity()) as u64)
                .map_err(std::io::Error::other)?;
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
fn decode<T: NativeOutcome>(
    caller: &Caller<'_, StoreData>,
    bytes: &[u8],
) -> wasmtime::Result<NativeResult<T>> {
    if bytes.len() > MAX_OUTCOME_BYTES {
        return Err(fatal_host_error("native outcome exceeds history limit"));
    }
    let structural_bound = super::effects_memory::decoded_heap_bound(
        bytes,
        T::SEQUENCE_ELEMENT_BYTES,
        T::DYNAMIC_MAPS,
    )?;
    let memory = HostBytes::new(
        &caller.data().tenant_limits,
        structural_bound.max(T::decoded_heap_bound(bytes.len())),
    )?;
    // Structural admission includes the decoder's reusable scratch buffer.
    let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(bytes));
    decoder.set_max_depth(128);
    let value = T::deserialize(&mut decoder).map_err(fatal_host_error)?;
    if decoder.position() != bytes.len() as u64 {
        return Err(fatal_host_error("trailing native outcome bytes"));
    }
    Ok(NativeResult {
        value,
        replayed: true,
        _memory: Some(memory),
    })
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
    fn capture(error: &wasmtime::Error) -> Self {
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
    fn into_error(self) -> wasmtime::Error {
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
/// The caller must bound the blob size and reserve the returned heap estimate.
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
        let limits = TenantLimits::new(1024 * 1024);
        for bits in [
            0x8000_0000_0000_0000,
            0x7ff8_0000_0000_1234,
            0x7ff0_0000_0000_0000,
        ] {
            let encoded = encode(&limits, &f64::from_bits(bits)).unwrap();
            let restored: f64 = rmp_serde::from_slice(&encoded.bytes).unwrap();
            assert_eq!(restored.to_bits(), bits);
        }
        assert_eq!(limits.host_attached_bytes(), 0);
    }
    #[test]
    fn binary_is_not_expanded_into_integer_elements() {
        let limits = TenantLimits::new(1024 * 1024);
        let value = OutcomeBytes(vec![255; 4096]);
        let encoded = encode(&limits, &value).unwrap();
        assert!(encoded.bytes.len() < 4100);
        let restored: OutcomeBytes = rmp_serde::from_slice(&encoded.bytes).unwrap();
        assert_eq!(restored, value);
        drop(encoded);
        assert_eq!(limits.host_attached_bytes(), 0);
        assert!(encode(&TenantLimits::new(32), &value).is_err());
    }
    #[test]
    fn missing_and_empty_native_strings_remain_distinct() {
        let limits = TenantLimits::new(1024 * 1024);
        for value in [None, Some(String::new()), Some("text".to_owned())] {
            let encoded = encode(&limits, &value).unwrap();
            let restored: Option<String> = rmp_serde::from_slice(&encoded.bytes).unwrap();
            assert_eq!(restored, value);
        }
    }
}
