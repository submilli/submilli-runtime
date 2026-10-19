//! Sealed, versioned initial execution inputs. Never rewritten by outcome appends.
use crate::adapters::sealing::Sealer;
use crate::database::DatabaseError;
use crate::domain::run_program::{EmbeddingLimits, ModelLimits, RunLimits, RunProgram};
use serde::{Deserialize, Serialize};
use std::time::Duration;
const MAX_PROGRAM_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn seal(
    sealer: &Sealer,
    id: &str,
    program: &RunProgram,
) -> Result<Vec<u8>, DatabaseError> {
    check_blueprint(&program.blueprint).map_err(invalid)?;
    let envelope = BorrowedEnvelope {
        version: 1,
        program,
    };
    let mut writer = ProgramWriter(Vec::new());
    envelope
        .serialize(&mut rmp_serde::Serializer::new(&mut writer).with_struct_map())
        .map_err(invalid)?;
    let plain = writer.0;
    check_collections(&plain)?;
    sealer
        .seal(&format!("run-program:v1:{id}"), &plain)
        .map_err(invalid)
}
pub(super) fn open(sealer: &Sealer, id: &str, sealed: &[u8]) -> Result<RunProgram, DatabaseError> {
    if sealed.len() > MAX_PROGRAM_BYTES + 128 {
        return Err(invalid("execution inputs exceed 16 MiB"));
    }
    let plain = sealer
        .open(&format!("run-program:v1:{id}"), sealed, "execution inputs")
        .map_err(invalid)?;
    check_collections(&plain)?;
    let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(&plain));
    decoder.set_max_depth(128);
    let envelope = Envelope::deserialize(&mut decoder).map_err(invalid)?;
    if envelope.version != 1 || decoder.position() != plain.len() as u64 {
        return Err(invalid("unsupported execution inputs"));
    }
    Ok(envelope.program)
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "RunProgram")]
struct ProgramRecord {
    source: String,
    #[serde(with = "recorded_blueprint")]
    blueprint: submilli_blueprint::Blueprint,
    variables: submilli_blueprint::VarBindings,
    #[serde(with = "LimitsRecord")]
    limits: RunLimits,
    compatibility: String,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "RunLimits")]
struct LimitsRecord {
    fuel: u64,
    max_wasm_stack: usize,
    memory_reservation: u64,
    memory_guard_size: u64,
    memory_reservation_for_growth: u64,
    max_store_bytes: u64,
    timeout: Option<Duration>,
    async_yield_fuel: Option<u64>,
    #[serde(with = "model_option")]
    llm: Option<ModelLimits>,
    #[serde(with = "embedding_option")]
    embedding: Option<EmbeddingLimits>,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "ModelLimits")]
struct ModelLimitsRecord {
    per_execution_tokens: u64,
    max_held_tokens: u64,
    max_prompt_count: u64,
    max_prompt_bytes: u64,
    default_output_cap: u64,
}
mod model_option {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &Option<ModelLimits>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Ref<'a>(#[serde(with = "ModelLimitsRecord")] &'a ModelLimits);
        value.as_ref().map(Ref).serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<Option<ModelLimits>, D::Error> {
        #[derive(Deserialize)]
        struct Owned(#[serde(with = "ModelLimitsRecord")] ModelLimits);
        Option::<Owned>::deserialize(decoder).map(|value| value.map(|value| value.0))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "EmbeddingLimits")]
struct EmbeddingLimitsRecord {
    per_execution_tokens: u64,
    max_held_tokens: u64,
    max_requests: u64,
    max_texts_per_call: u64,
    max_bytes_per_call: u64,
}
mod embedding_option {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &Option<EmbeddingLimits>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Ref<'a>(#[serde(with = "EmbeddingLimitsRecord")] &'a EmbeddingLimits);
        value.as_ref().map(Ref).serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<Option<EmbeddingLimits>, D::Error> {
        #[derive(Deserialize)]
        struct Owned(#[serde(with = "EmbeddingLimitsRecord")] EmbeddingLimits);
        Option::<Owned>::deserialize(decoder).map(|value| value.map(|value| value.0))
    }
}
#[derive(Serialize)]
struct BorrowedEnvelope<'a> {
    version: u32,
    #[serde(with = "ProgramRecord")]
    program: &'a RunProgram,
}
struct ProgramWriter(Vec<u8>);
impl std::io::Write for ProgramWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|length| *length <= MAX_PROGRAM_BYTES)
            .ok_or_else(|| std::io::Error::other("execution inputs exceed 16 MiB"))?;
        if length > self.0.capacity() {
            let capacity = length
                .max(self.0.capacity().saturating_mul(2))
                .min(MAX_PROGRAM_BYTES);
            self.0
                .try_reserve_exact(capacity - self.0.len())
                .map_err(std::io::Error::other)?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    #[serde(with = "ProgramRecord")]
    program: RunProgram,
}
fn check_collections(plain: &[u8]) -> Result<(), DatabaseError> {
    let heap =
        interpreter::runtime::effects::stored_heap_bound(plain, 512, true).map_err(invalid)?;
    if heap > 128 * 1024 * 1024 {
        return Err(invalid("execution input collections exceed 128 MiB"));
    }
    Ok(())
}
fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(format!("invalid run program: {error}"))
}

// Regex compilers own caches and automata beyond the literal DTO heap. Bound their
// count independently; the existing policy parser retains its per-pattern limits.
// This conservative admission cap is not a claim of exact compiled-policy bytes.
const MAX_RECORDED_REGEXES: usize = 16;
fn check_blueprint(blueprint: &submilli_blueprint::Blueprint) -> Result<(), String> {
    let mut count = 0;
    for rule in blueprint.permissions.values().flatten() {
        if let Some(filter) = &rule.filter {
            count += submilli_blueprint::FilterExpr::regex_count(&filter.to_string())?;
            if count > MAX_RECORDED_REGEXES {
                return Err("recorded blueprint exceeds 16 regex operators".into());
            }
        }
    }
    Ok(())
}
mod recorded_blueprint {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        blueprint: &submilli_blueprint::Blueprint,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        blueprint.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<submilli_blueprint::Blueprint, D::Error> {
        let plain = serde_json::Value::deserialize(decoder)?;
        let mut count = 0;
        if let Some(permissions) = plain
            .get("permissions")
            .and_then(serde_json::Value::as_object)
        {
            for rules in permissions.values().filter_map(serde_json::Value::as_array) {
                for rule in rules {
                    if let Some(filter) = rule.get("filter").and_then(serde_json::Value::as_str) {
                        count += submilli_blueprint::FilterExpr::regex_count(filter)
                            .map_err(serde::de::Error::custom)?;
                        if count > MAX_RECORDED_REGEXES {
                            return Err(serde::de::Error::custom(
                                "recorded blueprint exceeds 16 regex operators",
                            ));
                        }
                    }
                }
            }
        }
        serde_json::from_value(plain).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> RunProgram {
        let config = interpreter::runtime::RuntimeConfig::default();
        RunProgram {
            source: "function main(): void {}".into(),
            blueprint: submilli_blueprint::parse("name: test\ndefault: allow\n").unwrap(),
            variables: Default::default(),
            compatibility: "test-v1".into(),
            limits: RunLimits {
                fuel: config.fuel,
                max_wasm_stack: config.max_wasm_stack,
                memory_reservation: config.memory_reservation,
                memory_guard_size: config.memory_guard_size,
                memory_reservation_for_growth: config.memory_reservation_for_growth,
                max_store_bytes: config.max_store_bytes,
                timeout: config.timeout,
                async_yield_fuel: config.async_yield_fuel,
                llm: None,
                embedding: None,
            },
        }
    }
    #[test]
    fn program_roundtrip_preserves_inputs_and_rejects_wrong_owner_or_trailing_data() {
        let sealer = Sealer::new(Some(std::sync::Arc::new(
            submilli_shared::secret_store::SecretCipher::from_key([17; 32]),
        )));
        let program = program();
        let sealed = seal(&sealer, "one", &program).unwrap();
        assert_eq!(open(&sealer, "one", &sealed).unwrap(), program);
        assert!(open(&sealer, "two", &sealed).is_err());
        let mut plain = rmp_serde::to_vec_named(&BorrowedEnvelope {
            version: 1,
            program: &program,
        })
        .unwrap();
        plain.push(0xc0);
        let malformed = sealer.seal("run-program:v1:one", &plain).unwrap();
        assert!(open(&sealer, "one", &malformed).is_err());
        let malformed = sealer
            .seal("run-program:v1:one", &[0xdb, 255, 255, 255, 255])
            .unwrap();
        assert!(open(&sealer, "one", &malformed).is_err());
    }
    #[test]
    fn program_admission_bounds_encoding_and_policy_compilation_count() {
        let sealer = Sealer::new(Some(std::sync::Arc::new(
            submilli_shared::secret_store::SecretCipher::from_key([18; 32]),
        )));
        let mut program = program();
        program.source = "x".repeat(MAX_PROGRAM_BYTES);
        assert!(seal(&sealer, "one", &program).is_err());
        program.source.clear();
        let rules = (0..17).map(|_| "    - capability: fs.read\n      action: allow\n      filter: path matches \"a\"\n").collect::<String>();
        let yaml = format!("name: test\npermissions:\n  main:\n{rules}");
        program.blueprint = submilli_blueprint::parse(&yaml).unwrap();
        assert!(seal(&sealer, "one", &program).is_err());
        let plain = rmp_serde::to_vec_named(&BorrowedEnvelope {
            version: 1,
            program: &program,
        })
        .unwrap();
        let sealed = sealer.seal("run-program:v1:one", &plain).unwrap();
        assert!(open(&sealer, "one", &sealed).is_err());
        assert_eq!(
            submilli_blueprint::FilterExpr::regex_count("path == \"matches\"").unwrap(),
            0
        );
    }
}
