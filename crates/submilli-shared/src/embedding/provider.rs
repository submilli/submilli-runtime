//! The blueprint-backed [`EmbeddingProvider`]: resolve an alias, split a call
//! into sub-batches, dispatch them at a bounded concurrency, validate and
//! normalize what comes back, and settle each sent sub-batch.
//!
//! **A sub-batch's settlement is recorded the moment it is counted as sent**, as
//! indeterminate, and overwritten when its response arrives. A sub-batch still
//! in flight when a sibling fails (and is dropped) therefore settles as the
//! table's "sent, outcome unknown" row instead of leaking its held estimate.
//!
//! **Identity comes from configuration only** (KTD5): discovery and every result
//! carry the string computed at construction.

use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Range;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use futures::stream::{FuturesUnordered, StreamExt};
use interpreter::runtime::{
    EmbeddingBatch, EmbeddingError, EmbeddingFailureReason, EmbeddingMalformedReason,
    EmbeddingModel, EmbeddingProvider, EmbeddingTokenBudget, Purpose, SubBatchSettlement,
    estimate_embedding_tokens,
};
use sha2::{Digest, Sha256};
use submilli_blueprint::{Blueprint, EmbeddingProviderType, embedding_limits};
use tokio::sync::Semaphore;

use super::{
    DispatchFailure, DispatchResponse, DispatchRow, EmbeddingDispatch, EmbeddingRequest,
    NotSentReason, Rejection, SentFailure,
};

/// Sub-batches dispatched at once.
pub const DEFAULT_MAX_CONCURRENCY: usize = 4;

/// Hex digits of the identity digest: 16 bytes of SHA-256.
const IDENTITY_DIGEST_BYTES: usize = 16;

/// An alias with everything resolved at construction.
struct ResolvedAlias {
    provider: String,
    provider_type: EmbeddingProviderType,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
    dimensions: u64,
    send_dimensions: bool,
    description: Option<String>,
    max_input_tokens: u64,
    max_input_bytes: u64,
    query_prompt_name: Option<String>,
    document_prompt_name: Option<String>,
    identity: String,
    max_inputs: usize,
    max_request_tokens: Option<u64>,
}

/// The outbound embedding provider bound to one blueprint.
pub struct BlueprintEmbeddingProvider {
    aliases: BTreeMap<String, ResolvedAlias>,
    dispatch: Arc<dyn EmbeddingDispatch>,
    max_concurrency: usize,
}

impl BlueprintEmbeddingProvider {
    /// Resolve every alias in the blueprint's `embedding:` block. An alias whose
    /// provider or provider type does not resolve (blueprint validation already
    /// refuses both) is left out and answers `UnknownModel`. A `concurrency` of
    /// zero would deadlock the semaphore, so it clamps to one.
    pub fn new(
        blueprint: &Blueprint,
        dispatch: Arc<dyn EmbeddingDispatch>,
        concurrency: usize,
    ) -> Self {
        let aliases = blueprint
            .embedding
            .models
            .iter()
            .filter_map(|(name, decl)| {
                let provider = blueprint.embedding.providers.get(&decl.provider)?;
                let provider_type = EmbeddingProviderType::parse(&provider.provider_type)?;
                let resolved = ResolvedAlias::new(
                    decl.provider.clone(),
                    provider_type,
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    decl,
                );
                Some((name.clone(), resolved))
            })
            .collect();
        Self {
            aliases,
            dispatch,
            max_concurrency: concurrency.max(1),
        }
    }

    /// Lower every alias's input-count cap per request. Only lowers: the
    /// provider's own cap still applies. Zero clamps to one.
    #[must_use]
    pub fn with_max_inputs_per_request(mut self, cap: usize) -> Self {
        let cap = cap.max(1);
        for alias in self.aliases.values_mut() {
            alias.max_inputs = alias.max_inputs.min(cap);
        }
        self
    }

    /// The embedding-space identity of an alias, for tests and wiring.
    pub fn identity(&self, alias: &str) -> Option<&str> {
        self.aliases.get(alias).map(|a| a.identity.as_str())
    }

    fn resolve<'a>(&'a self, alias: &str) -> Result<&'a ResolvedAlias, EmbeddingError> {
        self.aliases
            .get(alias)
            .ok_or_else(|| EmbeddingError::UnknownModel {
                alias: alias.to_string(),
                available: self.aliases.keys().cloned().collect(),
            })
    }

    async fn embed_call(
        &self,
        alias: &str,
        texts: &[String],
        purpose: Purpose,
        budget: &EmbeddingTokenBudget,
    ) -> Result<EmbeddingBatch, EmbeddingError> {
        let resolved = self.resolve(alias)?;
        check_input_bytes(alias, resolved, texts)?;
        let dimensions = to_usize(resolved.dimensions, alias)?;
        if texts.is_empty() {
            return build_batch(alias, resolved, Vec::new(), 0, dimensions, None, Vec::new());
        }
        self.preflight(alias, resolved).await?;

        let plan = plan_sub_batches(texts, &resolved.split_caps(purpose));
        let call = Call {
            alias,
            resolved,
            texts,
            purpose,
            budget,
            dispatch: self.dispatch.as_ref(),
            limit: Semaphore::new(self.max_concurrency),
            slots: Mutex::new(vec![None; plan.len()]),
        };
        match call.run(&plan).await {
            Ok(outputs) => assemble(alias, resolved, dimensions, texts.len(), outputs, &call),
            Err(error) => Err(error.with_settlements(call.settlements())),
        }
    }

    async fn preflight(&self, alias: &str, resolved: &ResolvedAlias) -> Result<(), EmbeddingError> {
        match self.dispatch.preflight(&resolved.provider).await {
            Ok(()) => Ok(()),
            Err(failure) => Err(map_failure(failure, alias, 0, 0)),
        }
    }
}

impl EmbeddingProvider for BlueprintEmbeddingProvider {
    fn embed<'a>(
        &'a self,
        alias: &'a str,
        texts: &'a [String],
        purpose: Purpose,
        budget: &'a EmbeddingTokenBudget,
    ) -> Pin<Box<dyn Future<Output = Result<EmbeddingBatch, EmbeddingError>> + Send + 'a>> {
        Box::pin(self.embed_call(alias, texts, purpose, budget))
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<EmbeddingModel>, EmbeddingError>> + Send + 'a>>
    {
        Box::pin(async move {
            Ok(self
                .aliases
                .iter()
                .map(|(name, alias)| EmbeddingModel {
                    name: name.clone(),
                    description: alias.description.clone(),
                    dimensions: alias.dimensions,
                    max_input_tokens: Some(alias.max_input_tokens),
                    max_input_bytes: alias.max_input_bytes,
                    identity: alias.identity.clone(),
                })
                .collect())
        })
    }

    fn max_input_bytes(&self, alias: &str) -> Option<u64> {
        self.aliases.get(alias).map(|a| a.max_input_bytes)
    }
}

impl ResolvedAlias {
    fn new(
        provider: String,
        provider_type: EmbeddingProviderType,
        base_url: Option<String>,
        api_key: Option<String>,
        decl: &submilli_blueprint::EmbeddingModelDecl,
    ) -> Self {
        let dedicated = embedding_limits::is_dedicated_endpoint(provider_type, base_url.as_deref());
        let max_input_tokens = decl.max_input_tokens.unwrap_or_else(|| {
            embedding_limits::default_max_input_tokens(provider_type, &decl.model)
        });
        let identity = identity(provider_type, decl, base_url.as_deref());
        Self {
            provider,
            provider_type,
            send_dimensions: embedding_limits::accepts_dimensions_parameter(
                provider_type,
                &decl.model,
                dedicated,
            ),
            max_input_bytes: embedding_limits::max_input_bytes(
                provider_type,
                &decl.model,
                max_input_tokens,
                base_url.as_deref(),
            ),
            max_inputs: usize::try_from(embedding_limits::max_inputs_per_request(provider_type))
                .unwrap_or(usize::MAX),
            max_request_tokens: embedding_limits::max_request_tokens(provider_type, &decl.model),
            model: decl.model.clone(),
            base_url,
            api_key,
            dimensions: decl.dimensions,
            description: decl.description.clone(),
            max_input_tokens,
            query_prompt_name: decl.query_prompt_name.clone(),
            document_prompt_name: decl.document_prompt_name.clone(),
            identity,
        }
    }

    fn split_caps(&self, purpose: Purpose) -> SplitCaps {
        SplitCaps {
            max_inputs: self.max_inputs.max(1),
            // Half the cap leaves room for scripts the estimate undercounts.
            max_tokens: self.max_request_tokens.map(|cap| cap / 2),
            text_overhead_bytes: self.text_overhead_bytes(purpose),
        }
    }

    /// Bytes the wire adds around each text: the purpose template of a templated
    /// Google model. Counted into the usage ceiling.
    fn text_overhead_bytes(&self, purpose: Purpose) -> u64 {
        if self.provider_type != EmbeddingProviderType::Google
            || !embedding_limits::is_templated_google_model(&self.model)
        {
            return 0;
        }
        let prefix = match purpose {
            Purpose::Query => embedding_limits::GOOGLE_QUERY_TEMPLATE_PREFIX,
            Purpose::Document => embedding_limits::GOOGLE_DOCUMENT_TEMPLATE_PREFIX,
        };
        prefix.len() as u64
    }
}

// --- identity ---------------------------------------------------------------

/// Version of the canonical identity record layout.
const IDENTITY_RECORD_VERSION: &str = "1";
/// Normalization applied to returned vectors; part of the identity.
const NORMALIZATION_TAG: &str = "l2";

/// `emb1:<provider-type>:<model>:<dims>:<hex digest>`.
fn identity(
    provider_type: EmbeddingProviderType,
    decl: &submilli_blueprint::EmbeddingModelDecl,
    base_url: Option<&str>,
) -> String {
    let huggingface = provider_type == EmbeddingProviderType::HuggingFace;
    let dedicated = embedding_limits::is_dedicated_endpoint(provider_type, base_url);
    let route = match (huggingface, dedicated) {
        (false, _) => "none",
        (true, false) => "shared",
        (true, true) => "dedicated",
    };
    let endpoint_host = base_url.filter(|_| dedicated).map(endpoint_host);
    let mut record = CanonicalRecord::default();
    record.field(IDENTITY_RECORD_VERSION);
    record.field(provider_type.as_str());
    record.field(&decl.model);
    record.field(&decl.dimensions.to_string());
    record.field(NORMALIZATION_TAG);
    record.field(purpose_mechanism(provider_type, decl));
    record.optional(decl.query_prompt_name.as_deref());
    record.optional(decl.document_prompt_name.as_deref());
    record.field(route);
    record.optional(endpoint_host.as_deref());
    let digest = Sha256::digest(record.0.as_bytes());
    let hex: String = digest
        .iter()
        .take(IDENTITY_DIGEST_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "emb1:{}:{}:{}:{hex}",
        provider_type.as_str(),
        decl.model,
        decl.dimensions
    )
}

/// The canonical text the identity digest covers: each field length-prefixed
/// (`<len>:<value>;`) so no two configurations share a record, and an absent
/// optional field (`-;`) cannot be mistaken for a present one. Field order is
/// the order of the calls in [`identity`]: a new field belongs at the end, and
/// the leading version field changes only if existing records must.
#[derive(Default)]
struct CanonicalRecord(String);

impl CanonicalRecord {
    fn field(&mut self, value: &str) {
        self.0.push_str(&format!("{}:{value};", value.len()));
    }

    fn optional(&mut self, value: Option<&str>) {
        match value {
            Some(value) => self.field(value),
            None => self.0.push_str("-;"),
        }
    }
}

/// How the provider distinguishes queries from documents (KTD6).
fn purpose_mechanism(
    provider_type: EmbeddingProviderType,
    decl: &submilli_blueprint::EmbeddingModelDecl,
) -> &'static str {
    match provider_type {
        EmbeddingProviderType::Voyage => "input_type",
        EmbeddingProviderType::Jina => "task",
        EmbeddingProviderType::Google => {
            if embedding_limits::is_templated_google_model(&decl.model) {
                "template"
            } else {
                "taskType"
            }
        }
        EmbeddingProviderType::OpenAi => "none",
        EmbeddingProviderType::HuggingFace => {
            if decl.query_prompt_name.is_some() || decl.document_prompt_name.is_some() {
                "prompt_name"
            } else {
                "none"
            }
        }
    }
}

/// The host (and port) that decides which model answers a dedicated endpoint.
fn endpoint_host(base_url: &str) -> String {
    match url::Url::parse(base_url) {
        Ok(parsed) => match (parsed.host_str(), parsed.port()) {
            (Some(host), Some(port)) => format!("{}:{port}", host.to_ascii_lowercase()),
            (Some(host), None) => host.to_ascii_lowercase(),
            (None, _) => base_url.to_string(),
        },
        Err(_) => base_url.to_string(),
    }
}

// --- splitting --------------------------------------------------------------

struct SplitCaps {
    max_inputs: usize,
    max_tokens: Option<u64>,
    /// Bytes the wire adds around every text, counted into [`usage_ceiling`].
    text_overhead_bytes: u64,
}

struct SubBatch {
    range: Range<usize>,
    estimate: u64,
    /// The most tokens this sub-batch can plausibly have used; see
    /// [`usage_ceiling`].
    max_usage: u64,
}

/// The most tokens a text can plausibly use: a token covers at least one byte,
/// so its bytes (plus any template the wire adds) and the special tokens bound
/// it. Provider-reported usage is clamped to the sum of this over a sub-batch
/// before it is charged, because a buggy or hostile provider reporting a huge
/// number would otherwise saturate the server-wide aggregate and starve every
/// other run's reservations.
fn usage_ceiling(text: &str, overhead_bytes: u64) -> u64 {
    (text.len() as u64)
        .saturating_add(overhead_bytes)
        .saturating_add(embedding_limits::SPECIAL_TOKENS_PER_INPUT)
}

/// Contiguous sub-batches in input order. One closes when the next input would
/// exceed the input cap or the token cap (estimated); an
/// input larger than a cap on its own still gets a sub-batch, and the provider's
/// own rejection (or the pre-send length check) deals with it.
fn plan_sub_batches(texts: &[String], caps: &SplitCaps) -> Vec<SubBatch> {
    let mut plan = Vec::new();
    let mut start = 0usize;
    let mut tokens = 0u64;
    let mut ceiling = 0u64;
    for (index, text) in texts.iter().enumerate() {
        let text_tokens = estimate_embedding_tokens(text);
        let count = index - start;
        let over = count >= caps.max_inputs
            || caps
                .max_tokens
                .is_some_and(|cap| tokens.saturating_add(text_tokens) > cap);
        if count > 0 && over {
            plan.push(SubBatch {
                range: start..index,
                estimate: tokens,
                max_usage: ceiling,
            });
            start = index;
            tokens = 0;
            ceiling = 0;
        }
        ceiling = ceiling.saturating_add(usage_ceiling(text, caps.text_overhead_bytes));
        tokens = tokens.saturating_add(text_tokens);
    }
    plan.push(SubBatch {
        range: start..texts.len(),
        estimate: tokens,
        max_usage: ceiling,
    });
    plan
}

fn check_input_bytes(
    alias: &str,
    resolved: &ResolvedAlias,
    texts: &[String],
) -> Result<(), EmbeddingError> {
    match texts
        .iter()
        .position(|text| text.len() as u64 > resolved.max_input_bytes)
    {
        Some(index) => Err(EmbeddingError::InputTooLong {
            alias: alias.to_string(),
            index: Some(index),
            limit: Some(resolved.max_input_bytes),
            settlements: Vec::new(),
        }),
        None => Ok(()),
    }
}

// --- one call ---------------------------------------------------------------

/// What one successful sub-batch contributes.
struct SubOutput {
    /// Normalized, row-major, in the sub-batch's input order.
    values: Vec<f32>,
    usage: Option<u64>,
}

struct Call<'a> {
    alias: &'a str,
    resolved: &'a ResolvedAlias,
    texts: &'a [String],
    purpose: Purpose,
    budget: &'a EmbeddingTokenBudget,
    dispatch: &'a dyn EmbeddingDispatch,
    limit: Semaphore,
    /// Per sub-batch, `None` until it is counted as sent.
    slots: Mutex<Vec<Option<SubBatchSettlement>>>,
}

impl Call<'_> {
    /// Run every sub-batch under the semaphore. The first failure is returned
    /// and the sub-batches still running are dropped with the stream.
    async fn run(&self, plan: &[SubBatch]) -> Result<Vec<SubOutput>, EmbeddingError> {
        let mut running: FuturesUnordered<_> = plan
            .iter()
            .enumerate()
            .map(|(slot, sub)| async move { (slot, self.run_one(slot, sub).await) })
            .collect();
        let mut outputs: Vec<Option<SubOutput>> = plan.iter().map(|_| None).collect();
        while let Some((slot, result)) = running.next().await {
            let output = result?;
            let place = outputs.get_mut(slot).ok_or_else(|| self.internal())?;
            *place = Some(output);
        }
        outputs
            .into_iter()
            .map(|output| output.ok_or_else(|| self.internal()))
            .collect()
    }

    async fn run_one(&self, slot: usize, sub: &SubBatch) -> Result<SubOutput, EmbeddingError> {
        let _permit = self.limit.acquire().await.map_err(|_| self.internal())?;
        let sub_texts = self
            .texts
            .get(sub.range.clone())
            .ok_or_else(|| self.internal())?;

        // Count the request, and record it as sent-and-unknown, with no await
        // in between: a future dropped mid-flight settles as indeterminate.
        self.budget.mark_sent(self.alias, sub.estimate)?;
        self.record(slot, sent_unknown(sub.estimate));

        let response = self.dispatch.dispatch(self.request(sub_texts)).await;
        let response = match response {
            Ok(response) => response,
            Err(failure) => {
                self.record(slot, failure_settlement(&failure, sub));
                return Err(map_failure(
                    failure,
                    self.alias,
                    sub.range.start,
                    sub_texts.len(),
                ));
            }
        };

        // Usage settles the same whether or not the rows turn out to be usable.
        let usage = effective_usage(response.usage, sub.max_usage);
        self.record(slot, usage_settlement(sub.estimate, usage));
        validate(response, sub_texts.len(), self.resolved.dimensions)
            .map(|values| SubOutput { values, usage })
            .map_err(|reason| EmbeddingError::Malformed {
                alias: self.alias.to_string(),
                reason,
                settlements: Vec::new(),
            })
    }

    fn request<'a>(&'a self, texts: &'a [String]) -> EmbeddingRequest<'a> {
        let resolved = self.resolved;
        EmbeddingRequest {
            provider: &resolved.provider,
            provider_type: resolved.provider_type,
            model: &resolved.model,
            base_url: resolved.base_url.as_deref(),
            api_key: resolved.api_key.as_deref(),
            dimensions: resolved.dimensions,
            send_dimensions: resolved.send_dimensions,
            purpose: self.purpose,
            query_prompt_name: resolved.query_prompt_name.as_deref(),
            document_prompt_name: resolved.document_prompt_name.as_deref(),
            texts,
        }
    }

    fn record(&self, slot: usize, settlement: SubBatchSettlement) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = slots.get_mut(slot) {
            *entry = Some(settlement);
        }
    }

    /// Settlements of every sub-batch counted as sent, in sub-batch order.
    fn settlements(&self) -> Vec<SubBatchSettlement> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        slots.iter().flatten().copied().collect()
    }

    /// A failure of this provider's own bookkeeping (a closed pool, a range
    /// outside the batch) rather than of the request or the wire.
    fn internal(&self) -> EmbeddingError {
        EmbeddingError::Internal {
            alias: self.alias.to_string(),
            settlements: Vec::new(),
        }
    }
}

// --- settlement and failure mapping ----------------------------------------

/// Zero usage is treated as unreported; anything above `max` is clamped to it.
fn effective_usage(usage: Option<u64>, max: u64) -> Option<u64> {
    usage
        .filter(|&tokens| tokens > 0)
        .map(|tokens| tokens.min(max))
}

fn sent_unknown(estimate: u64) -> SubBatchSettlement {
    SubBatchSettlement {
        estimate,
        reported: 0,
        indeterminate: estimate,
    }
}

fn usage_settlement(estimate: u64, usage: Option<u64>) -> SubBatchSettlement {
    match usage {
        Some(reported) => SubBatchSettlement {
            estimate,
            reported,
            indeterminate: 0,
        },
        None => sent_unknown(estimate),
    }
}

fn failure_settlement(failure: &DispatchFailure, sub: &SubBatch) -> SubBatchSettlement {
    let estimate = sub.estimate;
    match failure {
        // Free: the request was refused or never left.
        DispatchFailure::NotSent(_) | DispatchFailure::Rejected(_) => SubBatchSettlement {
            estimate,
            reported: 0,
            indeterminate: 0,
        },
        DispatchFailure::Failed { usage, .. } | DispatchFailure::Malformed { usage, .. } => {
            usage_settlement(estimate, effective_usage(*usage, sub.max_usage))
        }
    }
}

/// The error for a failed dispatch, without settlements. `offset` and `len`
/// place a sub-batch-local input index in the original call.
fn map_failure(failure: DispatchFailure, alias: &str, offset: usize, len: usize) -> EmbeddingError {
    let alias = alias.to_string();
    let settlements = Vec::new();
    match failure {
        DispatchFailure::NotSent(NotSentReason::CredentialUnresolved)
        | DispatchFailure::Rejected(Rejection::Unauthorized) => {
            EmbeddingError::Unauthorized { alias, settlements }
        }
        DispatchFailure::NotSent(NotSentReason::Blocked) => EmbeddingError::Provider {
            alias,
            reason: EmbeddingFailureReason::BlockedByNetworkPolicy,
            settlements,
        },
        DispatchFailure::NotSent(NotSentReason::Unreachable) => EmbeddingError::Provider {
            alias,
            reason: EmbeddingFailureReason::Transport,
            settlements,
        },
        DispatchFailure::Rejected(Rejection::InputTooLong { index }) => {
            EmbeddingError::InputTooLong {
                alias,
                index: index
                    .filter(|&local| local < len)
                    .and_then(|local| local.checked_add(offset)),
                limit: None,
                settlements,
            }
        }
        DispatchFailure::Rejected(Rejection::RateLimited { .. }) => EmbeddingError::Provider {
            alias,
            reason: EmbeddingFailureReason::RateLimited,
            settlements,
        },
        DispatchFailure::Rejected(Rejection::Other) => EmbeddingError::Provider {
            alias,
            reason: EmbeddingFailureReason::RequestRejected,
            settlements,
        },
        DispatchFailure::Failed { kind, .. } => EmbeddingError::Provider {
            alias,
            reason: match kind {
                SentFailure::ProviderUnavailable => EmbeddingFailureReason::ProviderUnavailable,
                SentFailure::Transport => EmbeddingFailureReason::Transport,
                SentFailure::Timeout => EmbeddingFailureReason::Timeout,
            },
            settlements,
        },
        DispatchFailure::Malformed { reason, .. } => EmbeddingError::Malformed {
            alias,
            reason,
            settlements,
        },
    }
}

// --- validation and normalization ------------------------------------------

/// Check one sub-batch's rows and return them normalized, in input order.
fn validate(
    response: DispatchResponse,
    expected: usize,
    dimensions: u64,
) -> Result<Vec<f32>, EmbeddingMalformedReason> {
    use EmbeddingMalformedReason as Reason;
    let rows = order_rows(response.rows, expected)?;
    let width = usize::try_from(dimensions).map_err(|_| Reason::DimensionMismatch)?;
    let mut values = Vec::with_capacity(expected.saturating_mul(width));
    for row in rows {
        if row.len() != width {
            return Err(Reason::DimensionMismatch);
        }
        values.extend(l2_normalize(&row)?);
    }
    Ok(values)
}

/// One row scaled to unit length. A row that is not finite, or whose norm is
/// zero or overflows (which cannot be normalized any more than zero can), is
/// refused.
fn l2_normalize(row: &[f32]) -> Result<Vec<f32>, EmbeddingMalformedReason> {
    use EmbeddingMalformedReason as Reason;
    if row.iter().any(|value| !value.is_finite()) {
        return Err(Reason::NonFiniteValue);
    }
    let norm = row
        .iter()
        .map(|&value| f64::from(value) * f64::from(value))
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return Err(Reason::InvalidBody);
    }
    Ok(row
        .iter()
        .map(|&value| (f64::from(value) / norm) as f32)
        .collect())
}

/// Rows in input order: by `index` when every row has one (a permutation of
/// `0..expected`), otherwise positional.
fn order_rows(
    rows: Vec<DispatchRow>,
    expected: usize,
) -> Result<Vec<Vec<f32>>, EmbeddingMalformedReason> {
    use EmbeddingMalformedReason as Reason;
    if rows.len() != expected {
        return Err(Reason::CountMismatch);
    }
    let indexed = rows.iter().filter(|row| row.index.is_some()).count();
    if indexed == 0 {
        return Ok(rows.into_iter().map(|row| row.values).collect());
    }
    if indexed != rows.len() {
        return Err(Reason::InvalidBody);
    }
    let mut placed: Vec<Option<Vec<f32>>> = (0..expected).map(|_| None).collect();
    for row in rows {
        let slot = row
            .index
            .and_then(|index| placed.get_mut(index))
            .ok_or(Reason::CountMismatch)?;
        if slot.is_some() {
            return Err(Reason::CountMismatch);
        }
        *slot = Some(row.values);
    }
    placed
        .into_iter()
        .map(|row| row.ok_or(Reason::CountMismatch))
        .collect()
}

// --- assembly ---------------------------------------------------------------

fn assemble(
    alias: &str,
    resolved: &ResolvedAlias,
    dimensions: usize,
    count: usize,
    outputs: Vec<SubOutput>,
    call: &Call<'_>,
) -> Result<EmbeddingBatch, EmbeddingError> {
    // Every sub-batch must report for the batch to; saturating sum.
    let usage = outputs
        .iter()
        .map(|output| output.usage)
        .try_fold(0u64, |total, usage| usage.map(|u| total.saturating_add(u)));
    let values: Vec<f32> = outputs.into_iter().flat_map(|o| o.values).collect();
    build_batch(
        alias,
        resolved,
        values,
        count,
        dimensions,
        usage,
        call.settlements(),
    )
}

fn build_batch(
    alias: &str,
    resolved: &ResolvedAlias,
    values: Vec<f32>,
    count: usize,
    dimensions: usize,
    usage: Option<u64>,
    settlements: Vec<SubBatchSettlement>,
) -> Result<EmbeddingBatch, EmbeddingError> {
    let batch = EmbeddingBatch::new(values, count, dimensions, resolved.identity.clone(), alias)
        .map_err(|_| EmbeddingError::Internal {
            alias: alias.to_string(),
            settlements: settlements.clone(),
        })?;
    Ok(batch.with_input_tokens(usage).with_settlements(settlements))
}

fn to_usize(value: u64, alias: &str) -> Result<usize, EmbeddingError> {
    usize::try_from(value).map_err(|_| EmbeddingError::Malformed {
        alias: alias.to_string(),
        reason: EmbeddingMalformedReason::DimensionMismatch,
        settlements: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
