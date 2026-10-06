//! The blueprint `embedding:` block — the remote embedding providers a script
//! may reach through `submilli:embedding`, and the model aliases it may name.
//!
//! Shaped like `llm:`: a name-keyed `providers:` map (a `type`, an optional
//! `base_url`, an `api_key` holding a `${secrets.X}` placeholder) and a
//! name-keyed `models:` map of aliases, each pointing at a declared provider.
//! Declaration is authoritative: an alias the block does not declare cannot be
//! called, and a permission rule naming an undeclared alias is a validation
//! error here.
//!
//! The [`limits`] section holds the provider-limit tables later layers share, so
//! the numbers live in one place.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::endpoint::{self, ProviderBlock};
use crate::llm::{MAX_DESCRIPTION_CHARS, is_printable};
use crate::{Blueprint, BlueprintError, Fault, secret_refs, yaml_path};

/// How this block names itself in endpoint and secret-reference faults.
const PROVIDER_BLOCK: ProviderBlock = ProviderBlock {
    key: "embedding",
    label: "embedding provider",
};

/// The capability every `embedding.*` permission rule names.
const EMBEDDING_CAPABILITY: &str = "embedding.embed";

/// The filter field carrying the alias name.
const MODEL_FIELD: &str = "model";

/// The largest `dimensions` an alias may declare.
const MAX_DIMENSIONS: u64 = 8_192;

/// The longest a Hugging Face prompt name may be.
const MAX_PROMPT_NAME_CHARS: usize = 128;

/// The OpenAI model whose output size is fixed.
const ADA_002: &str = "text-embedding-ada-002";

/// The only dimensions `text-embedding-ada-002` produces.
const ADA_002_DIMENSIONS: u64 = 1_536;

/// The `embedding:` block: the providers a script may reach and the aliases it
/// may name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    /// Credential-and-endpoint rows, keyed by local provider identifier.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::maps::deserialize"
    )]
    pub providers: BTreeMap<String, EmbeddingProviderDecl>,
    /// The model aliases a program may call, keyed by the name it uses.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::maps::deserialize"
    )]
    pub models: BTreeMap<String, EmbeddingModelDecl>,
}

impl EmbeddingConfig {
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty() && self.models.is_empty()
    }
}

/// One declared provider: which API it speaks, where it lives, and the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingProviderDecl {
    /// `voyage`, `openai`, `google`, `jina`, or `huggingface`. Kept as a string
    /// so the validator can name the valid kinds with a YAML path; see
    /// [`EmbeddingProviderType::parse`].
    #[serde(rename = "type")]
    pub provider_type: String,
    /// The endpoint, when not the provider's own. Required in practice for a
    /// Hugging Face dedicated endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// `${secrets.X}` naming a declared secret. Required except for a
    /// `huggingface` provider with a `base_url` (a dedicated endpoint may be
    /// public).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl EmbeddingProviderDecl {
    /// Every string field that may carry a `${secrets.X}` placeholder; the
    /// destructuring makes a field added later a compile error here.
    fn secret_bearing_values(&self) -> Vec<&str> {
        let Self {
            provider_type: _,
            base_url,
            api_key,
        } = self;
        [base_url, api_key]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }
}

/// One declared model alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingModelDecl {
    /// The `providers:` key this alias routes through.
    pub provider: String,
    /// The provider's own model identifier.
    pub model: String,
    /// The vector length the alias produces. Required: a wrong guess corrupts
    /// an index silently.
    pub dimensions: u64,
    /// Tokens accepted per input. Absent means the provider default
    /// ([`limits::default_max_input_tokens`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Operator-authored prose describing what the alias is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Hugging Face only: the prompt name applied to queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_prompt_name: Option<String>,
    /// Hugging Face only: the prompt name applied to documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_prompt_name: Option<String>,
}

/// The provider kinds the block accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingProviderType {
    Voyage,
    OpenAi,
    Google,
    Jina,
    HuggingFace,
}

impl EmbeddingProviderType {
    /// Every kind, in message order.
    pub const ALL: [Self; 5] = [
        Self::Voyage,
        Self::OpenAi,
        Self::Google,
        Self::Jina,
        Self::HuggingFace,
    ];

    /// The YAML spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Voyage => "voyage",
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::Jina => "jina",
            Self::HuggingFace => "huggingface",
        }
    }

    /// The kind for a YAML `type` value, if it is one of the five.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    fn valid_names() -> String {
        Self::ALL.map(Self::as_str).join(", ")
    }
}

/// Provider limit tables shared by the validator and later layers.
/// Numbers here are the provider-documented request caps; unknown models fall
/// back to the conservative row rather than failing.
pub mod limits {
    use super::EmbeddingProviderType;

    /// Input bytes budgeted per token when sizing a request.
    const BYTES_PER_TOKEN: u64 = 3;

    /// Byte bound for `gemini-embedding-2` text: its 8,192-token limit, less the
    /// 29-byte longest task template ("task: search result | query: " plus
    /// margin), less [`SPECIAL_TOKENS_PER_INPUT`]. 8,192 - 29 - 16 = 8,147.
    const GEMINI_EMBEDDING_2_MAX_BYTES: u64 = 8_147;

    /// Byte bound for `gemini-embedding-001` and every unknown Google model:
    /// the 2,048-token limit less [`SPECIAL_TOKENS_PER_INPUT`]. 2,048 - 16 = 2,032.
    const GEMINI_EMBEDDING_001_MAX_BYTES: u64 = 2_032;

    /// Special tokens a provider may add around one input (BOS/EOS/separators).
    /// A token covers at least one byte of text, so an input's bytes plus this
    /// bound its token count.
    pub const SPECIAL_TOKENS_PER_INPUT: u64 = 16;

    /// The Google model that takes its task as a text template, not a `taskType`.
    const GEMINI_EMBEDDING_2: &str = "gemini-embedding-2";

    /// What `gemini-embedding-2` text is prefixed with to embed a query.
    pub const GOOGLE_QUERY_TEMPLATE_PREFIX: &str = "task: search result | query: ";
    /// What `gemini-embedding-2` text is prefixed with to embed a document.
    pub const GOOGLE_DOCUMENT_TEMPLATE_PREFIX: &str = "title: none | text: ";

    /// Google accepts `models/<name>` as well as the bare name.
    pub fn google_model_id(model: &str) -> &str {
        model.strip_prefix("models/").unwrap_or(model)
    }

    /// Whether a Google model takes its purpose as a text template.
    pub fn is_templated_google_model(model: &str) -> bool {
        google_model_id(model) == GEMINI_EMBEDDING_2
    }

    /// Whether the row talks to its own dedicated endpoint: a Hugging Face
    /// provider with a `base_url`. The complement of [`is_shared_hf_router`]
    /// among Hugging Face rows.
    pub fn is_dedicated_endpoint(
        provider_type: EmbeddingProviderType,
        base_url: Option<&str>,
    ) -> bool {
        provider_type == EmbeddingProviderType::HuggingFace && base_url.is_some()
    }

    /// Whether the row talks to the shared Hugging Face router: a Hugging Face
    /// provider without a `base_url`. The router silently truncates (live,
    /// 2026-10-05), so its inputs are bounded in bytes and it takes no prompt names.
    pub fn is_shared_hf_router(
        provider_type: EmbeddingProviderType,
        base_url: Option<&str>,
    ) -> bool {
        provider_type == EmbeddingProviderType::HuggingFace && base_url.is_none()
    }

    /// The most tokens one request may carry across all inputs, when the
    /// provider caps tokens at all.
    pub fn max_request_tokens(provider_type: EmbeddingProviderType, model: &str) -> Option<u64> {
        match provider_type {
            EmbeddingProviderType::Voyage => Some(match model {
                "voyage-4-lite" | "voyage-3.5-lite" => 1_000_000,
                "voyage-4" | "voyage-3.5" => 320_000,
                _ => 120_000,
            }),
            EmbeddingProviderType::OpenAi => Some(300_000),
            // The others publish no per-request token cap. Google's request
            // size was probed live (2026-10-05): requests of 12 KB and 81 KB
            // succeeded, so none is imposed.
            EmbeddingProviderType::Google
            | EmbeddingProviderType::Jina
            | EmbeddingProviderType::HuggingFace => None,
        }
    }

    /// The largest `max_input_tokens` an alias may declare: half the request
    /// cap, so one input never fills a whole request. `None` means uncapped.
    pub fn max_input_tokens_ceiling(
        provider_type: EmbeddingProviderType,
        model: &str,
    ) -> Option<u64> {
        max_request_tokens(provider_type, model).map(|cap| cap / 2)
    }

    /// The most inputs one request may carry.
    pub fn max_inputs_per_request(provider_type: EmbeddingProviderType) -> u64 {
        match provider_type {
            EmbeddingProviderType::Voyage => 1_000,
            EmbeddingProviderType::OpenAi => 2_048,
            EmbeddingProviderType::Google => 100,
            EmbeddingProviderType::Jina => 512,
            EmbeddingProviderType::HuggingFace => 32,
        }
    }

    /// Tokens accepted per input when the alias declares none.
    pub fn default_max_input_tokens(provider_type: EmbeddingProviderType, model: &str) -> u64 {
        match provider_type {
            EmbeddingProviderType::OpenAi | EmbeddingProviderType::Jina => 8_192,
            EmbeddingProviderType::Voyage => 32_000,
            EmbeddingProviderType::Google if is_templated_google_model(model) => 8_192,
            EmbeddingProviderType::Google => 2_048,
            EmbeddingProviderType::HuggingFace => 512,
        }
    }

    /// The most bytes one input may occupy: three per token, and for Google
    /// additionally bounded by the per-model byte bound (one byte per token,
    /// less the special tokens, as below). `gemini-embedding-001`
    /// silently truncates longer input and succeeds, even with `autoTruncate`
    /// off (confirmed live, 2026-10-05), so the bound is enforced before sending.
    ///
    /// The Hugging Face shared router behaves the same way: it ignores
    /// `truncate: false` (top level and under `parameters`) and silently cuts to
    /// the model's maximum length with HTTP 200 (confirmed live, 2026-10-05; a
    /// 24,000-byte and a 7,200-byte input returned identical vectors). A token
    /// covers at least one byte, so a shared alias is bounded to
    /// `max_input_tokens` less the special tokens, in bytes, and nothing is ever
    /// cut. A dedicated endpoint (TEI) honors `truncate: false` and keeps the
    /// three-per-token budget.
    pub fn max_input_bytes(
        provider_type: EmbeddingProviderType,
        model: &str,
        max_input_tokens: u64,
        base_url: Option<&str>,
    ) -> u64 {
        let by_tokens = max_input_tokens.saturating_mul(BYTES_PER_TOKEN);
        match provider_type {
            EmbeddingProviderType::Google => {
                // `gemini-embedding-001` and every unknown Google model take the
                // conservative 2,048-token bound.
                let bound = if is_templated_google_model(model) {
                    GEMINI_EMBEDDING_2_MAX_BYTES
                } else {
                    GEMINI_EMBEDDING_001_MAX_BYTES
                };
                by_tokens.min(bound)
            }
            EmbeddingProviderType::HuggingFace if is_shared_hf_router(provider_type, base_url) => {
                max_input_tokens.saturating_sub(SPECIAL_TOKENS_PER_INPUT)
            }
            _ => by_tokens,
        }
    }

    /// Whether the provider honors a requested output size for this model.
    ///
    /// Deliberately conservative: a model not listed is treated as fixed-size,
    /// so the runtime checks the returned length instead of sending a parameter
    /// the provider may reject. Listed: OpenAI `text-embedding-3-*`; Voyage
    /// `voyage-3.5*`, `voyage-4*`, `voyage-3-large`, `voyage-code-3` (those with
    /// `output_dimension`); Gemini embedding models; Jina v3 and later; Hugging
    /// Face dedicated endpoints (the operator's own server). Not
    /// `text-embedding-ada-002` and not the Hugging Face shared endpoint.
    pub fn accepts_dimensions_parameter(
        provider_type: EmbeddingProviderType,
        model: &str,
        dedicated: bool,
    ) -> bool {
        match provider_type {
            EmbeddingProviderType::OpenAi => model.starts_with("text-embedding-3-"),
            EmbeddingProviderType::Voyage => {
                model.starts_with("voyage-3.5")
                    || model.starts_with("voyage-4")
                    || matches!(model, "voyage-3-large" | "voyage-code-3")
            }
            EmbeddingProviderType::Google => google_model_id(model).starts_with("gemini-embedding"),
            EmbeddingProviderType::Jina => {
                ["jina-embeddings-v3", "jina-embeddings-v4"]
                    .iter()
                    .any(|prefix| model.starts_with(prefix))
                    || model.starts_with("jina-embeddings-v5")
            }
            EmbeddingProviderType::HuggingFace => dedicated,
        }
    }
}

/// Validate every `embedding:` entry, plus the `embedding.embed` permission
/// rules whose `model` filter references one. Runs even when the block is
/// empty, so a rule filtering on an undeclared alias is still caught.
pub(crate) fn validate_embedding(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    for (name, provider) in &blueprint.embedding.providers {
        validate_provider(blueprint, name, provider)?;
    }
    for (name, model) in &blueprint.embedding.models {
        validate_model(blueprint, name, model)?;
    }
    validate_permission_models(blueprint)
}

fn validate_provider(
    blueprint: &Blueprint,
    name: &str,
    provider: &EmbeddingProviderDecl,
) -> Result<(), BlueprintError> {
    let Some(kind) = EmbeddingProviderType::parse(&provider.provider_type) else {
        return Err(fault(
            yaml_path!["embedding", "providers", name, "type"],
            format!(
                "embedding provider '{name}': unknown type '{}' (use one of: {})",
                provider.provider_type,
                EmbeddingProviderType::valid_names()
            ),
        ));
    };
    if let Some(base_url) = &provider.base_url {
        endpoint::validate_endpoint(PROVIDER_BLOCK, name, base_url)
            .map_err(BlueprintError::InvalidEmbedding)?;
    }
    validate_api_key(name, kind, provider)?;
    endpoint::check_secret_refs(
        PROVIDER_BLOCK,
        name,
        provider.secret_bearing_values(),
        blueprint,
    )
    .map_err(BlueprintError::InvalidEmbedding)
}

/// `api_key` is required, and must be a `${secrets.X}` reference rather than a
/// literal key, except for a Hugging Face provider with a `base_url`, whose
/// dedicated endpoint may be public.
fn validate_api_key(
    name: &str,
    kind: EmbeddingProviderType,
    provider: &EmbeddingProviderDecl,
) -> Result<(), BlueprintError> {
    let Some(api_key) = &provider.api_key else {
        let public_dedicated =
            kind == EmbeddingProviderType::HuggingFace && provider.base_url.is_some();
        if public_dedicated {
            return Ok(());
        }
        return Err(fault(
            yaml_path!["embedding", "providers", name],
            format!(
                "embedding provider '{name}': 'api_key' is required; set it to \
                 \"${{secrets.NAME}}\" naming a declared secret (only a huggingface provider \
                 with a 'base_url' may omit it)"
            ),
        ));
    };
    if secret_refs(api_key).is_empty() {
        return Err(fault(
            yaml_path!["embedding", "providers", name, "api_key"],
            format!(
                "embedding provider '{name}': 'api_key' must be a \"${{secrets.NAME}}\" \
                 reference, not a literal key; declare the key under 'secrets:'"
            ),
        ));
    }
    Ok(())
}

fn validate_model(
    blueprint: &Blueprint,
    name: &str,
    model: &EmbeddingModelDecl,
) -> Result<(), BlueprintError> {
    let Some(provider) = blueprint.embedding.providers.get(&model.provider) else {
        return Err(fault(
            yaml_path!["embedding", "models", name, "provider"],
            format!(
                "embedding model '{name}' names undeclared provider '{}'; declare it under \
                 'embedding.providers:' or point the model at one of: {}",
                model.provider,
                endpoint::declared_names(&blueprint.embedding.providers)
            ),
        ));
    };
    // An unknown provider type was already refused when providers were checked.
    let Some(kind) = EmbeddingProviderType::parse(&provider.provider_type) else {
        return Ok(());
    };
    if model.model.trim().is_empty() {
        return Err(fault(
            yaml_path!["embedding", "models", name, "model"],
            format!("embedding model '{name}': 'model' must name the provider's model"),
        ));
    }
    validate_dimensions(name, kind, model)?;
    validate_max_input_tokens(name, kind, provider.base_url.as_deref(), model)?;
    validate_prompt_names(name, kind, provider.base_url.as_deref(), model)?;
    if let Some(description) = &model.description {
        validate_description(name, description)?;
    }
    Ok(())
}

fn validate_dimensions(
    name: &str,
    kind: EmbeddingProviderType,
    model: &EmbeddingModelDecl,
) -> Result<(), BlueprintError> {
    let path = yaml_path!["embedding", "models", name, "dimensions"];
    let dimensions = model.dimensions;
    if dimensions == 0 || dimensions > MAX_DIMENSIONS {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': dimensions {dimensions} is out of range; use a \
                 whole number from 1 to {MAX_DIMENSIONS}"
            ),
        ));
    }
    if kind == EmbeddingProviderType::OpenAi
        && model.model == ADA_002
        && dimensions != ADA_002_DIMENSIONS
    {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': {ADA_002} always returns {ADA_002_DIMENSIONS} \
                 dimensions and cannot be shortened; set 'dimensions: {ADA_002_DIMENSIONS}'"
            ),
        ));
    }
    Ok(())
}

fn validate_max_input_tokens(
    name: &str,
    kind: EmbeddingProviderType,
    base_url: Option<&str>,
    model: &EmbeddingModelDecl,
) -> Result<(), BlueprintError> {
    let Some(tokens) = model.max_input_tokens else {
        return Ok(());
    };
    let path = yaml_path!["embedding", "models", name, "max_input_tokens"];
    if tokens == 0 {
        return Err(fault(
            path,
            format!("embedding model '{name}': max_input_tokens must be greater than zero"),
        ));
    }
    // The shared Hugging Face router silently truncates (live, 2026-10-05), so
    // its byte bound is the token limit less the special tokens; it needs room.
    if limits::is_shared_hf_router(kind, base_url) && tokens <= limits::SPECIAL_TOKENS_PER_INPUT {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': max_input_tokens {tokens} leaves no room on the shared \
                 Hugging Face router, which reserves {} tokens per input for special tokens; \
                 use a value above {}",
                limits::SPECIAL_TOKENS_PER_INPUT,
                limits::SPECIAL_TOKENS_PER_INPUT
            ),
        ));
    }
    if let Some(ceiling) = limits::max_input_tokens_ceiling(kind, &model.model)
        && tokens > ceiling
    {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': max_input_tokens {tokens} is over {ceiling}, half \
                 of the {} provider's per-request token cap; lower it",
                kind.as_str()
            ),
        ));
    }
    Ok(())
}

/// Prompt names are a Hugging Face concept; elsewhere they would be silently
/// ignored, so they are refused.
fn validate_prompt_names(
    name: &str,
    kind: EmbeddingProviderType,
    base_url: Option<&str>,
    model: &EmbeddingModelDecl,
) -> Result<(), BlueprintError> {
    let fields = [
        ("query_prompt_name", &model.query_prompt_name),
        ("document_prompt_name", &model.document_prompt_name),
    ];
    for (field, value) in fields {
        let Some(value) = value else { continue };
        let path = yaml_path!["embedding", "models", name, field];
        if kind != EmbeddingProviderType::HuggingFace {
            return Err(fault(
                path,
                format!(
                    "embedding model '{name}': '{field}' applies only to huggingface \
                     providers; remove it"
                ),
            ));
        }
        if limits::is_shared_hf_router(kind, base_url) {
            return Err(fault(
                path,
                format!(
                    "embedding model '{name}': '{field}' is supported only on a dedicated \
                     Hugging Face endpoint (set base_url on the provider); the shared router \
                     may truncate the prompt-prefixed text"
                ),
            ));
        }
        let bad = value.trim().is_empty()
            || value.chars().count() > MAX_PROMPT_NAME_CHARS
            || value.chars().any(|c| c.is_control() || !is_printable(c));
        if bad {
            return Err(fault(
                path,
                format!(
                    "embedding model '{name}': '{field}' must be 1 to {MAX_PROMPT_NAME_CHARS} \
                     printable characters"
                ),
            ));
        }
    }
    Ok(())
}

/// Same rule as the `llm:` description: single line, printable, bounded.
fn validate_description(name: &str, description: &str) -> Result<(), BlueprintError> {
    let path = yaml_path!["embedding", "models", name, "description"];
    if description
        .chars()
        .any(|c| c.is_control() || !is_printable(c))
    {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': description must be a single line of printable \
                 text (no control or invisible formatting characters)"
            ),
        ));
    }
    let length = description.chars().count();
    if length > MAX_DESCRIPTION_CHARS {
        return Err(fault(
            path,
            format!(
                "embedding model '{name}': description is {length} characters, over the \
                 {MAX_DESCRIPTION_CHARS} allowed; shorten it"
            ),
        ));
    }
    Ok(())
}

/// Every `model` filter on an `embedding.embed` rule names a declared alias.
fn validate_permission_models(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    let models: Vec<&str> = blueprint
        .embedding
        .models
        .keys()
        .map(String::as_str)
        .collect();
    endpoint::check_permission_names(
        blueprint,
        &endpoint::PermissionNames {
            capability: EMBEDDING_CAPABILITY,
            field: MODEL_FIELD,
            noun: "embedding model",
            declared_in: "embedding.models:",
        },
        &models,
    )
    .map_err(BlueprintError::InvalidEmbedding)
}

fn fault(path: crate::YamlPath, message: String) -> BlueprintError {
    BlueprintError::InvalidEmbedding(Fault::at(path, message))
}

#[cfg(test)]
mod tests {
    use super::limits::*;
    use super::*;
    use crate::{parse, to_yaml};

    const FULL_BP: &str = "\
name: search
secrets:
  VOYAGE_KEY: { store: voyage }
  OPENAI_KEY: { store: openai }
  GOOGLE_KEY: { store: google }
  JINA_KEY: { store: jina }
  HF_KEY: { store: hf }
embedding:
  providers:
    voyage: { type: voyage, api_key: \"${secrets.VOYAGE_KEY}\" }
    openai: { type: openai, api_key: \"${secrets.OPENAI_KEY}\" }
    google: { type: google, api_key: \"${secrets.GOOGLE_KEY}\" }
    jina: { type: jina, api_key: \"${secrets.JINA_KEY}\" }
    hf:
      type: huggingface
      base_url: https://abc.endpoints.huggingface.cloud
      api_key: \"${secrets.HF_KEY}\"
  models:
    docs:
      provider: voyage
      model: voyage-3.5
      dimensions: 1024
      max_input_tokens: 16000
      description: \"General documents.\"
    small:
      provider: openai
      model: text-embedding-3-small
      dimensions: 512
    gem: { provider: google, model: gemini-embedding-001, dimensions: 768 }
    jn: { provider: jina, model: jina-embeddings-v3, dimensions: 1024 }
    hfm:
      provider: hf
      model: BAAI/bge-small-en-v1.5
      dimensions: 384
      query_prompt_name: query
      document_prompt_name: passage
permissions:
  main:
    - capability: embedding.embed
      filter: model == \"docs\"
      action: allow
    - capability: embedding.embed
      filter: model glob \"g*\"
      action: allow
";

    fn embedding_fault(yaml: &str) -> Fault {
        match parse(yaml).expect_err("blueprint must be rejected") {
            BlueprintError::InvalidEmbedding(fault) => fault,
            other => panic!("expected an embedding fault, got {other:?}"),
        }
    }

    /// A blueprint with secret `K` and one provider `p` of the given type.
    fn with_provider(kind: &str, models: &str) -> String {
        format!(
            "name: x\nsecrets:\n  K: {{ store: K }}\nembedding:\n  providers:\n    p:\n      \
             type: {kind}\n      api_key: \"${{secrets.K}}\"\n  models:\n{models}"
        )
    }

    fn model(kind: &str, body: &str) -> String {
        with_provider(kind, &format!("    m:\n      provider: p\n{body}"))
    }

    fn path_of(fault: &Fault) -> Vec<String> {
        fault
            .path
            .as_ref()
            .map(|p| p.iter().map(|seg| format!("{seg:?}")).collect())
            .unwrap_or_default()
    }

    #[test]
    fn all_five_provider_types_round_trip() {
        let parsed = parse(FULL_BP).expect("valid");
        assert_eq!(parsed.embedding.providers.len(), 5);
        assert_eq!(parse(&to_yaml(&parsed)).expect("round trip"), parsed);
    }

    #[test]
    fn an_absent_embedding_block_is_not_serialized() {
        let b = parse("name: x\n").expect("valid");
        assert!(b.embedding.is_empty());
        assert!(!to_yaml(&b).contains("embedding"));
    }

    #[test]
    fn an_unknown_provider_type_is_refused_with_its_path() {
        let fault = embedding_fault(&with_provider("carrier_pigeon", ""));
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["embedding", "providers", "p", "type"][..])
        );
        for kind in EmbeddingProviderType::ALL {
            assert!(fault.message.contains(kind.as_str()), "{}", fault.message);
        }
    }

    #[test]
    fn an_alias_naming_an_undeclared_provider_is_refused() {
        let yaml =
            "name: x\nembedding:\n  models:\n    m: { provider: ghost, model: a, dimensions: 8 }\n";
        let fault = embedding_fault(yaml);
        assert_eq!(
            fault.path.as_deref(),
            Some(&yaml_path!["embedding", "models", "m", "provider"][..])
        );
    }

    #[test]
    fn missing_dimensions_is_refused_with_a_path() {
        let err = parse(&model("openai", "      model: text-embedding-3-small\n"))
            .expect_err("dimensions is required");
        let fault = err.fault().expect("fault");
        assert!(fault.path.is_some(), "{fault:?}");
        assert!(fault.message.contains("dimensions"), "{}", fault.message);
    }

    #[test]
    fn out_of_range_dimensions_are_refused() {
        for dims in [0, 9000] {
            let yaml = model(
                "openai",
                &format!("      model: text-embedding-3-small\n      dimensions: {dims}\n"),
            );
            let fault = embedding_fault(&yaml);
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["embedding", "models", "m", "dimensions"][..])
            );
        }
        let edge = model(
            "openai",
            "      model: text-embedding-3-large\n      dimensions: 8192\n",
        );
        assert!(parse(&edge).is_ok());
    }

    #[test]
    fn a_bad_base_url_is_refused_for_any_type() {
        for url in ["http://api.example.com", "https://10.0.0.1/v1"] {
            let yaml = format!(
                "name: x\nsecrets:\n  K: {{ store: K }}\nembedding:\n  providers:\n    p:\n      \
                 type: voyage\n      base_url: {url}\n      api_key: \"${{secrets.K}}\"\n"
            );
            let fault = embedding_fault(&yaml);
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["embedding", "providers", "p", "base_url"][..]),
                "{url}"
            );
        }
    }

    #[test]
    fn ada_002_must_declare_1536_dimensions() {
        let bad = model(
            "openai",
            "      model: text-embedding-ada-002\n      dimensions: 512\n",
        );
        assert!(embedding_fault(&bad).message.contains("1536"));
        let good = model(
            "openai",
            "      model: text-embedding-ada-002\n      dimensions: 1536\n",
        );
        assert!(parse(&good).is_ok());
    }

    #[test]
    fn prompt_names_are_huggingface_only() {
        for field in ["query_prompt_name", "document_prompt_name"] {
            let yaml = model(
                "voyage",
                &format!("      model: voyage-3.5\n      dimensions: 1024\n      {field}: q\n"),
            );
            let fault = embedding_fault(&yaml);
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["embedding", "models", "m", field][..])
            );
        }
    }

    #[test]
    fn prompt_names_are_refused_on_the_shared_huggingface_router() {
        for field in ["query_prompt_name", "document_prompt_name"] {
            let shared = format!(
                "name: x\nsecrets:\n  K: {{ store: K }}\nembedding:\n  providers:\n    p: {{ type: huggingface, api_key: \"${{secrets.K}}\" }}\n  models:\n    m:\n      provider: p\n      model: bge\n      dimensions: 384\n      {field}: q\n"
            );
            let fault = embedding_fault(&shared);
            assert_eq!(
                fault.path.as_deref(),
                Some(&yaml_path!["embedding", "models", "m", field][..])
            );
            assert!(
                fault.message.contains("dedicated Hugging Face endpoint"),
                "{}",
                fault.message
            );
            let dedicated = shared.replace(
                "type: huggingface,",
                "type: huggingface, base_url: \"https://e.example.com\",",
            );
            assert!(parse(&dedicated).is_ok());
        }
    }

    #[test]
    fn an_undeclared_secret_is_refused() {
        let yaml = "name: x\nembedding:\n  providers:\n    p: { type: voyage, api_key: \"${secrets.NOPE}\" }\n";
        let fault = embedding_fault(yaml);
        assert!(fault.message.contains("NOPE"), "{}", fault.message);
    }

    #[test]
    fn a_literal_api_key_is_refused() {
        let yaml = "name: x\nembedding:\n  providers:\n    p: { type: voyage, api_key: sk-live }\n";
        let fault = embedding_fault(yaml);
        assert!(fault.message.contains("secrets"), "{}", fault.message);
        assert!(!fault.message.contains("sk-live"));
    }

    #[test]
    fn permission_rules_must_name_declared_aliases() {
        let base = |filter: &str| {
            format!(
                "{}permissions:\n  main:\n    - capability: embedding.embed\n      filter: {filter}\n      action: allow\n",
                model(
                    "voyage",
                    "      model: voyage-3.5\n      dimensions: 1024\n"
                )
            )
        };
        let fault = embedding_fault(&base("model == \"missing\""));
        assert!(fault.message.contains("missing"), "{}", fault.message);
        assert!(parse(&base("model glob \"z*\"")).is_ok());
        assert!(parse(&base("model == \"m\"")).is_ok());
    }

    #[test]
    fn max_input_tokens_is_bounded_by_half_the_request_cap() {
        let over = model(
            "openai",
            "      model: text-embedding-3-small\n      dimensions: 512\n      max_input_tokens: 200000\n",
        );
        assert!(embedding_fault(&over).message.contains("150000"));
        let zero = model(
            "openai",
            "      model: text-embedding-3-small\n      dimensions: 512\n      max_input_tokens: 0\n",
        );
        embedding_fault(&zero);
        let edge = model(
            "openai",
            "      model: text-embedding-3-small\n      dimensions: 512\n      max_input_tokens: 150000\n",
        );
        assert!(parse(&edge).is_ok());
        // Providers with no token cap accept any positive value.
        let jina = model(
            "jina",
            "      model: jina-embeddings-v3\n      dimensions: 1024\n      max_input_tokens: 500000\n",
        );
        assert!(parse(&jina).is_ok());
    }

    #[test]
    fn huggingface_shared_max_input_tokens_must_exceed_the_special_tokens() {
        let shared = |tokens: u64| {
            format!(
                "name: x\nsecrets:\n  K: {{ store: K }}\nembedding:\n  providers:\n    p: {{ type: huggingface, api_key: \"${{secrets.K}}\" }}\n  models:\n    m:\n      provider: p\n      model: BAAI/bge-small-en-v1.5\n      dimensions: 384\n      max_input_tokens: {tokens}\n"
            )
        };
        let fault = embedding_fault(&shared(16));
        assert!(
            fault.message.contains("special tokens"),
            "{}",
            fault.message
        );
        assert!(parse(&shared(17)).is_ok());
        let dedicated = "name: x\nembedding:\n  providers:\n    p: { type: huggingface, base_url: \"https://e.example.com\" }\n  models:\n    m:\n      provider: p\n      model: bge\n      dimensions: 384\n      max_input_tokens: 8\n";
        assert!(parse(dedicated).is_ok());
    }

    #[test]
    fn huggingface_may_omit_the_key_only_with_a_base_url() {
        let public = "name: x\nembedding:\n  providers:\n    p: { type: huggingface, base_url: \"https://e.example.com\" }\n";
        assert!(parse(public).is_ok());
        let shared = "name: x\nembedding:\n  providers:\n    p: { type: huggingface }\n";
        assert!(embedding_fault(shared).message.contains("api_key"));
        let other = "name: x\nembedding:\n  providers:\n    p: { type: voyage, base_url: \"https://e.example.com\" }\n";
        assert!(embedding_fault(other).message.contains("api_key"));
    }

    #[test]
    fn descriptions_are_bounded_and_printable() {
        let newline = model(
            "voyage",
            "      model: voyage-3.5\n      dimensions: 8\n      description: \"a\\nb\"\n",
        );
        assert!(embedding_fault(&newline).message.contains("single line"));
        let long = "a".repeat(MAX_DESCRIPTION_CHARS + 1);
        let too_long = model(
            "voyage",
            &format!(
                "      model: voyage-3.5\n      dimensions: 8\n      description: \"{long}\"\n"
            ),
        );
        assert!(embedding_fault(&too_long).message.contains("shorten"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let yaml = "name: x\nembedding:\n  providers:\n    p: { type: voyage, api_ky: x }\n";
        assert!(matches!(parse(yaml), Err(BlueprintError::Parse(_))));
    }

    #[test]
    fn every_embedding_fault_carries_an_embedding_path() {
        for yaml in [
            with_provider("nope", ""),
            model(
                "openai",
                "      model: text-embedding-3-small\n      dimensions: 0\n",
            ),
        ] {
            let fault = embedding_fault(&yaml);
            assert_eq!(
                path_of(&fault).first().map(String::as_str),
                Some("Key(\"embedding\")")
            );
        }
    }

    #[test]
    fn request_token_caps_follow_the_table() {
        use EmbeddingProviderType::*;
        assert_eq!(max_request_tokens(Voyage, "voyage-4-lite"), Some(1_000_000));
        assert_eq!(
            max_request_tokens(Voyage, "voyage-3.5-lite"),
            Some(1_000_000)
        );
        assert_eq!(max_request_tokens(Voyage, "voyage-4"), Some(320_000));
        assert_eq!(max_request_tokens(Voyage, "voyage-3.5"), Some(320_000));
        assert_eq!(max_request_tokens(Voyage, "voyage-code-3"), Some(120_000));
        assert_eq!(max_request_tokens(Voyage, "unknown"), Some(120_000));
        assert_eq!(max_request_tokens(OpenAi, "x"), Some(300_000));
        for kind in [Google, Jina, HuggingFace] {
            assert_eq!(max_request_tokens(kind, "x"), None);
        }
        assert_eq!(max_input_tokens_ceiling(OpenAi, "x"), Some(150_000));
        assert_eq!(max_input_tokens_ceiling(Jina, "x"), None);
    }

    #[test]
    fn defaults_stay_within_the_ceiling() {
        use EmbeddingProviderType::*;
        assert_eq!(default_max_input_tokens(OpenAi, "x"), 8_192);
        assert_eq!(default_max_input_tokens(Voyage, "x"), 32_000);
        assert_eq!(default_max_input_tokens(Jina, "x"), 8_192);
        assert_eq!(default_max_input_tokens(HuggingFace, "x"), 512);
        assert_eq!(
            default_max_input_tokens(Google, "gemini-embedding-001"),
            2_048
        );
        assert_eq!(default_max_input_tokens(Google, "mystery"), 2_048);
        assert_eq!(
            default_max_input_tokens(Google, "gemini-embedding-2"),
            8_192
        );
        for kind in EmbeddingProviderType::ALL {
            let default = default_max_input_tokens(kind, "x");
            if let Some(ceiling) = max_input_tokens_ceiling(kind, "x") {
                assert!(default <= ceiling, "{kind:?}");
            }
        }
    }

    #[test]
    fn input_bytes_are_three_per_token_capped_for_google() {
        use EmbeddingProviderType::*;
        assert_eq!(max_input_bytes(OpenAi, "x", 8_192, None), 24_576);
        assert_eq!(
            max_input_bytes(Google, "gemini-embedding-001", 2_048, None),
            2_032
        );
        assert_eq!(
            max_input_bytes(Google, "gemini-embedding-001", 100, None),
            300
        );
        assert_eq!(
            max_input_bytes(Google, "gemini-embedding-2", 8_192, None),
            8_147
        );
        assert_eq!(max_input_bytes(Google, "mystery", 8_192, None), 2_032);
        assert_eq!(max_input_bytes(Jina, "x", u64::MAX, None), u64::MAX);
    }

    #[test]
    fn huggingface_shared_bytes_are_the_token_limit_less_special_tokens() {
        use EmbeddingProviderType::*;
        assert_eq!(max_input_bytes(HuggingFace, "x", 512, None), 496);
        assert_eq!(
            max_input_bytes(HuggingFace, "x", 512, Some("https://e.example")),
            1_536
        );
        assert_eq!(max_input_bytes(HuggingFace, "x", 256, None), 240);
        assert_eq!(max_input_bytes(HuggingFace, "x", 10, None), 0);
    }

    #[test]
    fn google_model_helpers_own_the_prefix_and_the_templated_model() {
        assert_eq!(
            google_model_id("models/gemini-embedding-2"),
            "gemini-embedding-2"
        );
        assert_eq!(
            google_model_id("gemini-embedding-001"),
            "gemini-embedding-001"
        );
        assert!(is_templated_google_model("gemini-embedding-2"));
        assert!(is_templated_google_model("models/gemini-embedding-2"));
        assert!(!is_templated_google_model("gemini-embedding-001"));
    }

    #[test]
    fn only_a_huggingface_provider_with_a_base_url_is_dedicated() {
        use EmbeddingProviderType::*;
        assert!(is_dedicated_endpoint(
            HuggingFace,
            Some("https://x.example")
        ));
        assert!(!is_dedicated_endpoint(HuggingFace, None));
        assert!(!is_dedicated_endpoint(OpenAi, Some("https://x.example")));
    }

    #[test]
    fn only_a_huggingface_provider_without_a_base_url_is_the_shared_router() {
        use EmbeddingProviderType::*;
        assert!(is_shared_hf_router(HuggingFace, None));
        assert!(!is_shared_hf_router(HuggingFace, Some("https://x.example")));
        assert!(!is_shared_hf_router(OpenAi, None));
    }

    #[test]
    fn per_request_limits_follow_the_table() {
        use EmbeddingProviderType::*;
        assert_eq!(max_inputs_per_request(Voyage), 1_000);
        assert_eq!(max_inputs_per_request(OpenAi), 2_048);
        assert_eq!(max_inputs_per_request(Google), 100);
        assert_eq!(max_inputs_per_request(Jina), 512);
        assert_eq!(max_inputs_per_request(HuggingFace), 32);
    }

    #[test]
    fn dimension_parameter_support_is_conservative() {
        use EmbeddingProviderType::*;
        assert!(accepts_dimensions_parameter(
            OpenAi,
            "text-embedding-3-small",
            false
        ));
        assert!(!accepts_dimensions_parameter(
            OpenAi,
            "text-embedding-ada-002",
            false
        ));
        assert!(accepts_dimensions_parameter(
            Voyage,
            "voyage-4-large",
            false
        ));
        assert!(accepts_dimensions_parameter(Voyage, "voyage-code-3", false));
        assert!(!accepts_dimensions_parameter(Voyage, "voyage-2", false));
        assert!(accepts_dimensions_parameter(
            Google,
            "gemini-embedding-001",
            false
        ));
        assert!(accepts_dimensions_parameter(
            Jina,
            "jina-embeddings-v3",
            false
        ));
        assert!(accepts_dimensions_parameter(
            Jina,
            "jina-embeddings-v5-text-small",
            false
        ));
        assert!(!accepts_dimensions_parameter(
            Jina,
            "jina-embeddings-v2-base-en",
            false
        ));
        assert!(accepts_dimensions_parameter(HuggingFace, "m", true));
        assert!(!accepts_dimensions_parameter(HuggingFace, "m", false));
    }
}
