//! What a run cost: its model calls priced, plus the costs packages reported.
//!
//! The interpreter lists the run's model calls and package reports
//! ([`StoreData::model_calls`], [`StoreData::package_costs`]). This module prices
//! them against the blueprint and folds them into one [`RunCost`], the value the
//! HTTP response, the CLI and the playground show.
//!
//! A model is priced from its blueprint `pricing:` first, then from the built-in
//! list ([`builtin`]). Anything neither prices is listed with its usage and no
//! amount, and the run's total says it is incomplete. Unknown is never zero.
//!
//! [`StoreData::model_calls`]: interpreter::runtime::StoreData::model_calls
//! [`StoreData::package_costs`]: interpreter::runtime::StoreData::package_costs

pub mod builtin;

use interpreter::runtime::{
    AttributedUsage, ModelCallUsage, ModelUsage, PackageCost, PackageCosts, StoreData,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use submilli_blueprint::{Blueprint, EmbeddingPricing, EmbeddingProviderType, LlmPricing, Usd};

use crate::llm::ProviderKind;

/// A finished run's cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCost {
    /// The sum of every amount known. When [`Self::complete`] is false, the
    /// run cost more than this by an amount nobody priced.
    pub total_usd: Usd,
    /// Every entry is priced, and no package report was dropped.
    pub complete: bool,
    /// One entry per source, name and label, in order of first appearance.
    pub entries: Vec<CostEntry>,
    /// Package reports past the per-run cap, not in any entry.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped_reports: u64,
}

/// One source's share of a run's cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostEntry {
    pub source: CostSource,
    /// The model alias the program called, or the reporting package.
    pub name: String,
    /// A package's own label for the report, such as the model it called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The sum of the priced parts. `None` when no part was priced.
    pub usd: Option<Usd>,
    /// Some part has no price, so [`Self::usd`] is short or absent.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub complete: bool,
    /// Where a model's price came from. Absent for packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<PriceSource>,
    /// Counts behind the amount, keyed by unit. A count the provider did not
    /// report is left out.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub usage: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    Llm,
    Embedding,
    Package,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceSource {
    /// The blueprint's `pricing:` for the model.
    Blueprint,
    /// The built-in price list.
    Builtin,
}

impl RunCost {
    /// The cost as text: a total line, then one indented line per entry.
    ///
    /// ```text
    /// cost: $0.00421 (incomplete: 1 unpriced)
    ///   llm claude-sonnet-4-6: $0.0039 (builtin price)
    ///   embedding docs: unpriced (input_tokens 900)
    /// ```
    pub fn lines(&self) -> Vec<String> {
        let unpriced = self.entries.iter().filter(|entry| !entry.complete).count();
        let mut gaps = Vec::new();
        if unpriced > 0 {
            gaps.push(format!("{unpriced} unpriced"));
        }
        if self.dropped_reports > 0 {
            gaps.push(format!("{} reports dropped", self.dropped_reports));
        }
        let mut lines = vec![if gaps.is_empty() {
            format!("cost: ${}", self.total_usd)
        } else {
            format!(
                "cost: ${} (incomplete: {})",
                self.total_usd,
                gaps.join(", ")
            )
        }];
        for entry in &self.entries {
            lines.push(format!("  {}", entry.line()));
        }
        lines
    }
}

impl CostEntry {
    fn line(&self) -> String {
        let source = match self.source {
            CostSource::Llm => "llm",
            CostSource::Embedding => "embedding",
            CostSource::Package => "package",
        };
        // A package names its own label and units: shown escaped, so they
        // cannot forge a line on a terminal.
        let name = match &self.label {
            Some(label) => format!("{} {}", self.name, label.escape_debug()),
            None => self.name.clone(),
        };
        let amount = match (self.usd, self.complete) {
            (Some(usd), true) => format!("${usd}"),
            (Some(usd), false) => format!("${usd}, part unpriced"),
            (None, _) => "unpriced".to_owned(),
        };
        let mut notes = Vec::new();
        match self.pricing {
            Some(PriceSource::Builtin) => notes.push("builtin price".to_owned()),
            Some(PriceSource::Blueprint) => notes.push("blueprint price".to_owned()),
            None => {}
        }
        if !self.complete {
            let usage: Vec<String> = self
                .usage
                .iter()
                .map(|(unit, count)| format!("{} {count}", unit.escape_debug()))
                .collect();
            if !usage.is_empty() {
                notes.push(usage.join(", "));
            }
        }
        if notes.is_empty() {
            format!("{source} {name}: {amount}")
        } else {
            format!("{source} {name}: {amount} ({})", notes.join("; "))
        }
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_true(value: &bool) -> bool {
    *value
}

fn yes() -> bool {
    true
}

/// Price the run a store just finished. `None` when it made no model call and
/// no package reported, so a run that cost nothing carries no cost at all.
pub fn price_run(blueprint: &Blueprint, store: &StoreData) -> Option<RunCost> {
    price(blueprint, store.model_calls(), store.package_costs())
}

fn price(
    blueprint: &Blueprint,
    model_calls: &[ModelCallUsage],
    package_costs: &PackageCosts,
) -> Option<RunCost> {
    if model_calls.is_empty() && package_costs.is_empty() {
        return None;
    }
    let mut entries: Vec<CostEntry> = Vec::new();
    for call in model_calls {
        let priced = price_model_call(blueprint, call);
        merge(&mut entries, priced);
    }
    // Already one entry per package and label: the interpreter sums on arrival.
    entries.extend(package_costs.entries().iter().map(package_entry));
    let dropped_reports = package_costs.dropped();
    let total_usd = entries
        .iter()
        .filter_map(|entry| entry.usd)
        .fold(Usd::ZERO, Usd::saturating_add);
    let complete = dropped_reports == 0 && entries.iter().all(|entry| entry.complete);
    Some(RunCost {
        total_usd,
        complete,
        entries,
        dropped_reports,
    })
}

/// Add a model call's entry to the entry with the same source and alias, or
/// append it. Model usage counts are integers.
fn merge(entries: &mut Vec<CostEntry>, next: CostEntry) {
    let same = |entry: &&mut CostEntry| entry.source == next.source && entry.name == next.name;
    let Some(entry) = entries.iter_mut().find(same) else {
        entries.push(next);
        return;
    };
    entry.usd = match (entry.usd, next.usd) {
        (None, None) => None,
        (left, right) => Some(
            left.unwrap_or(Usd::ZERO)
                .saturating_add(right.unwrap_or(Usd::ZERO)),
        ),
    };
    entry.complete &= next.complete;
    // A model's price source is the same for every call of one alias.
    entry.pricing = entry.pricing.or(next.pricing);
    for (unit, count) in next.usage {
        let summed = match (
            entry.usage.get(&unit).and_then(Value::as_u64),
            count.as_u64(),
        ) {
            (Some(existing), Some(count)) => existing.saturating_add(count).into(),
            _ => count,
        };
        entry.usage.insert(unit, summed);
    }
}

/// A count as JSON: an integer when it is one, so `10` does not print `10.0`.
fn number(value: f64) -> serde_json::Value {
    if value.fract() == 0.0 && value >= 0.0 && value < u64::MAX as f64 {
        (value as u64).into()
    } else {
        serde_json::Number::from_f64(value).map_or(serde_json::Value::Null, Into::into)
    }
}

fn price_model_call(blueprint: &Blueprint, call: &ModelCallUsage) -> CostEntry {
    match &call.usage {
        AttributedUsage::Llm(usage) => {
            let price = llm_price(blueprint, &call.model, usage.input_tokens.unwrap_or(0));
            let usd = price
                .as_ref()
                .and_then(|(price, _)| llm_amount(price, usage));
            CostEntry {
                source: CostSource::Llm,
                name: call.model.clone(),
                label: None,
                usd,
                complete: usd.is_some(),
                pricing: price.map(|(_, source)| source),
                usage: llm_usage(usage),
            }
        }
        AttributedUsage::Embedding { input_tokens } => {
            let price = embedding_price(blueprint, &call.model);
            let usd = price
                .as_ref()
                .zip(*input_tokens)
                .map(|((price, _), tokens)| price.input.per_million(tokens));
            let mut usage = serde_json::Map::new();
            if let Some(tokens) = input_tokens {
                usage.insert("input_tokens".into(), (*tokens).into());
            }
            CostEntry {
                source: CostSource::Embedding,
                name: call.model.clone(),
                label: None,
                usd,
                complete: usd.is_some(),
                pricing: price.map(|(_, source)| source),
                usage,
            }
        }
    }
}

/// One call's cost: uncached input, both cache parts and output, each at its
/// own price. `None` when the input or output count is unknown.
fn llm_amount(price: &LlmPricing, usage: &ModelUsage) -> Option<Usd> {
    let input = usage.input_tokens?;
    let output = usage.output_tokens?;
    let read = usage.cache_read_tokens.unwrap_or(0);
    let write = usage.cache_write_tokens.unwrap_or(0);
    // The cache counts are parts of the input. More cache than input is a count
    // nobody can price, not free input.
    let fresh = input.checked_sub(read)?.checked_sub(write)?;
    Some(
        price
            .input
            .per_million(fresh)
            .saturating_add(price.cache_read.unwrap_or(price.input).per_million(read))
            .saturating_add(price.cache_write.unwrap_or(price.input).per_million(write))
            .saturating_add(price.output.per_million(output)),
    )
}

fn llm_usage(usage: &ModelUsage) -> serde_json::Map<String, serde_json::Value> {
    [
        ("input_tokens", usage.input_tokens),
        ("cache_read_tokens", usage.cache_read_tokens),
        ("cache_write_tokens", usage.cache_write_tokens),
        ("output_tokens", usage.output_tokens),
    ]
    .into_iter()
    .filter_map(|(unit, count)| Some((unit.to_owned(), count?.into())))
    .collect()
}

/// The blueprint's price for an LLM alias, else the built-in one.
///
/// The built-in list applies only to a first-party provider on its default
/// endpoint: the same model id behind a gateway or a compatible server can cost
/// something else, or nothing. Each record is one prompt, so a price tiered by
/// prompt size is chosen from `prompt_tokens`.
fn llm_price(
    blueprint: &Blueprint,
    alias: &str,
    prompt_tokens: u64,
) -> Option<(LlmPricing, PriceSource)> {
    let decl = blueprint.llm.models.get(alias)?;
    if let Some(pricing) = &decl.pricing {
        return Some((pricing.clone(), PriceSource::Blueprint));
    }
    let provider = blueprint.llm.providers.get(&decl.provider)?;
    if provider.base_url.is_some() {
        return None;
    }
    let kind = ProviderKind::from_type(&provider.provider_type)?;
    // The `models:` key is the id the provider receives.
    builtin::llm(kind, alias, prompt_tokens).map(|pricing| (pricing, PriceSource::Builtin))
}

/// The blueprint's price for an embedding alias, else the built-in one, under
/// the same first-party, default-endpoint rule as [`llm_price`].
fn embedding_price(blueprint: &Blueprint, alias: &str) -> Option<(EmbeddingPricing, PriceSource)> {
    let decl = blueprint.embedding.models.get(alias)?;
    if let Some(pricing) = &decl.pricing {
        return Some((pricing.clone(), PriceSource::Blueprint));
    }
    let provider = blueprint.embedding.providers.get(&decl.provider)?;
    if provider.base_url.is_some() {
        return None;
    }
    let kind = EmbeddingProviderType::parse(&provider.provider_type)?;
    builtin::embedding(kind, &decl.model).map(|pricing| (pricing, PriceSource::Builtin))
}

fn package_entry(cost: &PackageCost) -> CostEntry {
    CostEntry {
        source: CostSource::Package,
        name: cost.package.clone(),
        label: cost.label.clone(),
        usd: cost.pico_usd.map(Usd::from_pico),
        complete: cost.unpriced_reports == 0,
        pricing: None,
        usage: cost
            .usage
            .iter()
            .map(|(unit, count)| (unit.clone(), number(*count)))
            .collect(),
    }
}

#[cfg(test)]
mod tests;
