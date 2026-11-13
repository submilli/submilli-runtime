//! List prices for models whose id we know, used when a blueprint names no
//! `pricing:` of its own.
//!
//! Prices go stale, so the list is guarded three ways. An entry matches only
//! the exact model id the provider receives, never a prefix and never a
//! floating alias such as `-latest`. [`super::llm_price`] and
//! [`super::embedding_price`] consult it only for a first-party provider on its
//! default endpoint. And each entry the run used says `builtin`, so a reader
//! knows where the number came from.
//!
//! Updating this list is part of every release: check each vendor's pricing
//! page and bump [`AS_OF`].

use submilli_blueprint::{EmbeddingPricing, EmbeddingProviderType, LlmPricing, Usd};

use crate::llm::ProviderKind;

/// The day the prices below were last checked against the vendors' pages.
pub const AS_OF: &str = "2026-11-13";

/// Dollars per million tokens: input, output, cache read, cache write.
type Rates = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
);

/// One model id's list price. `above` is a second price for a prompt of more
/// than that many tokens, cache included, where the vendor charges by size.
struct LlmEntry {
    kind: ProviderKind,
    id: &'static str,
    rates: Rates,
    above: Option<(u64, Rates)>,
}

const fn flat(kind: ProviderKind, id: &'static str, rates: Rates) -> LlmEntry {
    LlmEntry {
        kind,
        id,
        rates,
        above: None,
    }
}

const fn tiered(
    kind: ProviderKind,
    id: &'static str,
    rates: Rates,
    above: (u64, Rates),
) -> LlmEntry {
    LlmEntry {
        kind,
        id,
        rates,
        above: Some(above),
    }
}

use ProviderKind::{Anthropic as A, Google as G, OpenAi as O};

/// Left out on purpose: ids that can move to another model (Anthropic's undated
/// 4.5 aliases, `-latest`), ids whose price is announced to change within weeks,
/// and models with no confirmed list price.
const LLM: &[LlmEntry] = &[
    // https://platform.claude.com/docs/en/about-claude/pricing (5-minute cache write)
    flat(A, "claude-fable-5-2", ("8", "40", Some("0.2"), Some("10"))),
    flat(A, "claude-mythos-5-2", ("8", "40", Some("0.2"), Some("10"))),
    flat(
        A,
        "claude-fable-5-1",
        ("10", "50", Some("0.25"), Some("12.5")),
    ),
    flat(
        A,
        "claude-mythos-5-1",
        ("10", "50", Some("0.25"), Some("12.5")),
    ),
    flat(A, "claude-fable-5", ("10", "50", Some("1"), Some("12.5"))),
    flat(A, "claude-mythos-5", ("10", "50", Some("1"), Some("12.5"))),
    flat(A, "claude-opus-5-5", ("4", "20", Some("0.2"), Some("5"))),
    flat(A, "claude-opus-5", ("5", "25", Some("0.5"), Some("6.25"))),
    flat(A, "claude-opus-4-8", ("5", "25", Some("0.5"), Some("6.25"))),
    flat(A, "claude-opus-4-7", ("5", "25", Some("0.5"), Some("6.25"))),
    flat(A, "claude-opus-4-6", ("5", "25", Some("0.5"), Some("6.25"))),
    flat(
        A,
        "claude-opus-4-5-20251101",
        ("5", "25", Some("0.5"), Some("6.25")),
    ),
    flat(
        A,
        "claude-sonnet-5-5",
        ("2", "10", Some("0.1"), Some("2.5")),
    ),
    flat(A, "claude-sonnet-5", ("2", "10", Some("0.2"), Some("2.5"))),
    flat(
        A,
        "claude-sonnet-4-6",
        ("3", "15", Some("0.3"), Some("3.75")),
    ),
    flat(
        A,
        "claude-sonnet-4-5-20250929",
        ("3", "15", Some("0.3"), Some("3.75")),
    ),
    tiered(
        A,
        "claude-haiku-5-5",
        ("0.1", "0.5", Some("0.01"), Some("0.125")),
        (100_000, ("0.5", "2.5", Some("0.05"), Some("0.625"))),
    ),
    flat(
        A,
        "claude-haiku-4-5-20251001",
        ("1", "5", Some("0.1"), Some("1.25")),
    ),
    // https://developers.openai.com/api/docs/pricing (standard tier; no cache write)
    tiered(
        O,
        "gpt-6-astra",
        ("10", "50", Some("1"), None),
        (272_000, ("20", "75", Some("2"), None)),
    ),
    tiered(
        O,
        "gpt-6.1-sol",
        ("2", "10", Some("0.1"), None),
        (272_000, ("4", "15", Some("0.2"), None)),
    ),
    tiered(
        O,
        "gpt-6.1-terra",
        ("1", "5", Some("0.05"), None),
        (272_000, ("2", "7.5", Some("0.1"), None)),
    ),
    tiered(
        O,
        "gpt-6-sol",
        ("2", "10", Some("0.2"), None),
        (272_000, ("4", "15", Some("0.4"), None)),
    ),
    tiered(
        O,
        "gpt-6-luna",
        ("0.1", "0.5", Some("0.01"), None),
        (272_000, ("0.2", "0.75", Some("0.02"), None)),
    ),
    tiered(
        O,
        "gpt-5.6-terra",
        ("2", "12", Some("0.2"), None),
        (272_000, ("4", "18", Some("0.4"), None)),
    ),
    tiered(
        O,
        "gpt-5.6-luna",
        ("0.2", "1.2", Some("0.02"), None),
        (272_000, ("0.4", "1.8", Some("0.04"), None)),
    ),
    tiered(
        O,
        "gpt-5.5",
        ("5", "30", Some("0.5"), None),
        (272_000, ("10", "45", Some("1"), None)),
    ),
    tiered(
        O,
        "gpt-5.5-pro",
        ("30", "180", None, None),
        (272_000, ("60", "270", None, None)),
    ),
    tiered(
        O,
        "gpt-5.4-pro",
        ("30", "180", None, None),
        (272_000, ("60", "270", None, None)),
    ),
    tiered(
        O,
        "gpt-5.4",
        ("2.5", "15", Some("0.25"), None),
        (272_000, ("5", "22.5", Some("0.5"), None)),
    ),
    flat(O, "gpt-5.4-mini", ("0.75", "4.5", Some("0.075"), None)),
    flat(O, "gpt-5.4-nano", ("0.2", "1.25", Some("0.02"), None)),
    flat(O, "gpt-5.2", ("1.75", "14", Some("0.175"), None)),
    flat(O, "gpt-5.2-pro", ("21", "168", None, None)),
    flat(O, "gpt-5.1", ("1.25", "10", Some("0.125"), None)),
    flat(O, "gpt-5", ("1.25", "10", Some("0.125"), None)),
    flat(O, "gpt-5-mini", ("0.25", "2", Some("0.025"), None)),
    flat(O, "gpt-5-nano", ("0.05", "0.4", Some("0.005"), None)),
    flat(O, "gpt-5-pro", ("15", "120", None, None)),
    flat(O, "gpt-4.1", ("2", "8", Some("0.5"), None)),
    flat(O, "gpt-4.1-mini", ("0.4", "1.6", Some("0.1"), None)),
    flat(O, "gpt-4o", ("2.5", "10", Some("1.25"), None)),
    flat(O, "gpt-4o-mini", ("0.15", "0.6", Some("0.075"), None)),
    flat(O, "o3", ("2", "8", Some("0.5"), None)),
    flat(O, "o3-pro", ("20", "80", None, None)),
    // https://ai.google.dev/gemini-api/docs/pricing (paid tier, text input; the
    // cache read is the context-caching price, storage is billed apart)
    flat(G, "gemini-4-hydrogen", ("1.5", "7.5", Some("0.075"), None)),
    flat(
        G,
        "gemini-3.5-flash-lite",
        ("0.3", "2.5", Some("0.03"), None),
    ),
    flat(
        G,
        "gemini-3.1-flash-lite",
        ("0.25", "1.5", Some("0.025"), None),
    ),
    tiered(
        G,
        "gemini-3.1-pro-preview",
        ("2", "12", Some("0.2"), None),
        (200_000, ("4", "18", Some("0.4"), None)),
    ),
    flat(
        G,
        "gemini-3-flash-preview",
        ("0.5", "3", Some("0.05"), None),
    ),
    flat(
        G,
        "gemini-2.5-flash-lite",
        ("0.1", "0.4", Some("0.01"), None),
    ),
];

use EmbeddingProviderType::{Google as EG, OpenAi as EO, Voyage as EV};

/// `(kind, exact model id, input dollars per million tokens)`. Jina has no
/// per-token dollar price, and Hugging Face bills compute, so neither is here.
const EMBEDDING: &[(EmbeddingProviderType, &str, &str)] = &[
    (EO, "text-embedding-3-small", "0.02"),
    (EO, "text-embedding-3-large", "0.13"),
    (EO, "text-embedding-ada-002", "0.1"),
    (EG, "gemini-embedding-001", "0.15"),
    (EG, "gemini-embedding-2", "0.2"),
    // https://docs.voyageai.com/docs/pricing
    (EV, "voyage-4-large", "0.12"),
    (EV, "voyage-context-4", "0.12"),
    (EV, "voyage-code-4", "0.12"),
    (EV, "voyage-finance-2", "0.12"),
    (EV, "voyage-law-2", "0.12"),
    (EV, "voyage-code-2", "0.12"),
    (EV, "voyage-multilingual-2", "0.12"),
    (EV, "voyage-large-2-instruct", "0.12"),
    (EV, "voyage-large-2", "0.12"),
    (EV, "voyage-4", "0.06"),
    (EV, "voyage-3.5", "0.06"),
    (EV, "voyage-3", "0.06"),
    (EV, "voyage-4-lite", "0.02"),
    (EV, "voyage-3.5-lite", "0.02"),
    (EV, "voyage-3-lite", "0.02"),
    (EV, "voyage-context-3", "0.18"),
    (EV, "voyage-code-3", "0.18"),
    (EV, "voyage-3-large", "0.18"),
];

/// The list price of an LLM model id at a first-party provider, for one prompt
/// of `prompt_tokens` (cache included).
pub fn llm(kind: ProviderKind, model: &str, prompt_tokens: u64) -> Option<LlmPricing> {
    let entry = LLM
        .iter()
        .find(|entry| entry.kind == kind && entry.id == model)?;
    let rates = match entry.above {
        Some((threshold, above)) if prompt_tokens > threshold => above,
        _ => entry.rates,
    };
    pricing(rates)
}

fn pricing((input, output, read, write): Rates) -> Option<LlmPricing> {
    Some(LlmPricing {
        input: usd(input)?,
        output: usd(output)?,
        cache_read: optional(read)?,
        cache_write: optional(write)?,
    })
}

/// The list price of an embedding model id at a first-party provider.
pub fn embedding(kind: EmbeddingProviderType, model: &str) -> Option<EmbeddingPricing> {
    let (_, _, input) = EMBEDDING
        .iter()
        .find(|(entry_kind, id, _)| *entry_kind == kind && *id == model)?;
    Some(EmbeddingPricing { input: usd(input)? })
}

/// The first id listed for `kind` and its base price, for tests elsewhere.
#[cfg(test)]
pub(crate) fn first_llm_entry_for_tests(kind: ProviderKind) -> Option<(&'static str, LlmPricing)> {
    LLM.iter()
        .find(|entry| entry.kind == kind && entry.above.is_none())
        .and_then(|entry| Some((entry.id, pricing(entry.rates)?)))
}

/// A listed price. The test below proves every literal parses; one that did
/// not would leave its model unpriced, never free.
fn usd(text: &str) -> Option<Usd> {
    Usd::parse(text).ok()
}

/// An optional listed price: `Some(None)` when none is listed, `None` when the
/// listed one does not parse.
fn optional(text: Option<&str>) -> Option<Option<Usd>> {
    match text {
        Some(text) => usd(text).map(Some),
        None => Some(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_price_parses_and_every_id_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for entry in LLM {
            let id = entry.id;
            assert_ne!(
                entry.kind,
                ProviderKind::OpenAiCompatible,
                "{id}: not first-party"
            );
            assert!(!id.ends_with("-latest"), "{id}: a floating alias");
            let tiers = std::iter::once(entry.rates).chain(entry.above.map(|(_, rates)| rates));
            for (input, output, read, write) in tiers {
                for price in [Some(input), Some(output), read, write]
                    .into_iter()
                    .flatten()
                {
                    Usd::parse(price).unwrap_or_else(|error| panic!("{id}: {error}"));
                }
            }
            assert!(
                seen.insert(format!("{:?}/{id}", entry.kind)),
                "{id}: listed twice"
            );
        }
        for (kind, id, input) in EMBEDDING {
            assert!(
                matches!(
                    kind,
                    EmbeddingProviderType::OpenAi
                        | EmbeddingProviderType::Google
                        | EmbeddingProviderType::Voyage
                ),
                "{id}: no per-token list price"
            );
            assert!(!id.ends_with("-latest"), "{id}: a floating alias");
            Usd::parse(input).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert!(seen.insert(format!("{kind:?}/{id}")), "{id}: listed twice");
        }
    }

    #[test]
    fn a_prompt_above_the_threshold_takes_the_upper_tier() {
        let small = llm(ProviderKind::Anthropic, "claude-haiku-5-5", 100_000).unwrap();
        let large = llm(ProviderKind::Anthropic, "claude-haiku-5-5", 100_001).unwrap();
        assert_eq!(small.input.to_string(), "0.1");
        assert_eq!(large.input.to_string(), "0.5");
    }

    #[test]
    fn an_unknown_id_has_no_price() {
        assert_eq!(llm(ProviderKind::Anthropic, "claude-unknown", 0), None);
        assert_eq!(embedding(EmbeddingProviderType::OpenAi, "unknown"), None);
    }
}
