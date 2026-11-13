use interpreter::runtime::{ModelCallUsage, ModelUsage, PackageCosts, PackageReport};
use serde_json::json;

use super::*;

fn blueprint(yaml: &str) -> Blueprint {
    submilli_blueprint::parse(yaml).expect("test blueprint parses")
}

/// A blueprint with one priced and one unpriced LLM model behind a compatible
/// endpoint, and one priced embedding alias.
fn priced() -> Blueprint {
    blueprint(
        r#"
name: cost
secrets:
  KEY: { store: KEY }
llm:
  providers:
    gateway:
      type: openai-compatible
      base_url: https://gateway.example/v1
      api_key: ${secrets.KEY}
  models:
    priced:
      provider: gateway
      pricing: { input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 }
    unpriced:
      provider: gateway
embedding:
  providers:
    voyage:
      type: voyage
      api_key: ${secrets.KEY}
  models:
    docs:
      provider: voyage
      model: voyage-custom
      dimensions: 1024
      pricing: { input: 0.06 }
"#,
    )
}

fn llm(model: &str, usage: ModelUsage) -> ModelCallUsage {
    ModelCallUsage {
        model: model.into(),
        usage: AttributedUsage::Llm(usage),
    }
}

/// `price` with no package reports.
fn price_calls(blueprint: &Blueprint, calls: &[ModelCallUsage]) -> Option<RunCost> {
    price(blueprint, calls, &PackageCosts::default())
}

fn tokens(input: u64, output: u64) -> ModelUsage {
    ModelUsage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        ..ModelUsage::default()
    }
}

#[test]
fn a_run_with_nothing_to_price_has_no_cost() {
    assert_eq!(price_calls(&priced(), &[]), None);
}

#[test]
fn blueprint_pricing_charges_each_part_at_its_own_rate() {
    let usage = ModelUsage {
        input_tokens: Some(1_000_000),
        output_tokens: Some(100_000),
        cache_read_tokens: Some(600_000),
        cache_write_tokens: Some(200_000),
    };
    let cost = price_calls(&priced(), &[llm("priced", usage)]).unwrap();
    // fresh 200k × 3 + read 600k × 0.3 + write 200k × 3.75 + out 100k × 15
    assert_eq!(cost.total_usd.to_string(), "3.03");
    assert!(cost.complete);
    let entry = &cost.entries[0];
    assert_eq!(entry.pricing, Some(PriceSource::Blueprint));
    assert_eq!(
        serde_json::to_value(entry).unwrap(),
        json!({
            "source": "llm",
            "name": "priced",
            "usd": "3.03",
            "pricing": "blueprint",
            "usage": {
                "input_tokens": 1_000_000,
                "cache_read_tokens": 600_000,
                "cache_write_tokens": 200_000,
                "output_tokens": 100_000,
            },
        })
    );
}

#[test]
fn calls_of_one_alias_fold_into_one_entry() {
    let calls = [
        llm("priced", tokens(1_000, 10)),
        llm("priced", tokens(2_000, 20)),
    ];
    let cost = price_calls(&priced(), &calls).unwrap();
    assert_eq!(cost.entries.len(), 1);
    assert_eq!(cost.entries[0].usage["input_tokens"], json!(3_000));
    assert_eq!(cost.total_usd.to_string(), "0.00945");
}

#[test]
fn an_unpriced_model_shows_usage_and_marks_the_total_incomplete() {
    let calls = [
        llm("priced", tokens(1_000, 0)),
        llm("unpriced", tokens(500, 5)),
    ];
    let cost = price_calls(&priced(), &calls).unwrap();
    assert!(!cost.complete);
    assert_eq!(cost.total_usd.to_string(), "0.003");
    assert_eq!(
        serde_json::to_value(&cost.entries[1]).unwrap(),
        json!({
            "source": "llm",
            "name": "unpriced",
            "usd": null,
            "complete": false,
            "usage": { "input_tokens": 500, "output_tokens": 5 },
        })
    );
}

#[test]
fn an_unknown_count_is_unpriced_not_free() {
    let usage = ModelUsage {
        input_tokens: None,
        output_tokens: Some(5),
        ..ModelUsage::default()
    };
    let cost = price_calls(&priced(), &[llm("priced", usage)]).unwrap();
    assert_eq!(cost.entries[0].usd, None);
    assert!(!cost.complete);
}

#[test]
fn embedding_calls_are_priced_by_input() {
    let calls = [ModelCallUsage {
        model: "docs".into(),
        usage: AttributedUsage::Embedding {
            input_tokens: Some(1_000_000),
        },
    }];
    let cost = price_calls(&priced(), &calls).unwrap();
    assert_eq!(cost.total_usd.to_string(), "0.06");
    assert_eq!(cost.entries[0].source, CostSource::Embedding);
}

fn report(label: &str, pico: Option<u128>, input: f64) -> PackageReport {
    PackageReport {
        label: Some(label.into()),
        pico_usd: pico,
        usage: vec![("input_tokens".into(), input)],
    }
}

#[test]
fn package_reports_group_by_package_and_label() {
    let mut reports = PackageCosts::default();
    reports.add("@submilli/typesafe", report("jev-1.14.0", Some(420), 10.0));
    reports.add("@submilli/typesafe", report("jev-1.14.0", Some(840), 20.0));
    reports.add("@submilli/typesafe", report("jev-9", None, 5.0));
    let cost = price(&priced(), &[], &reports).unwrap();
    assert_eq!(cost.entries.len(), 2);
    assert_eq!(
        serde_json::to_value(&cost.entries[0]).unwrap(),
        json!({
            "source": "package",
            "name": "@submilli/typesafe",
            "label": "jev-1.14.0",
            "usd": "0.00000000126",
            "usage": { "input_tokens": 30 },
        })
    );
    assert_eq!(cost.entries[1].usd, None);
    assert!(!cost.complete);
}

#[test]
fn a_priced_and_an_unpriced_report_under_one_label_are_partly_priced() {
    let mut reports = PackageCosts::default();
    reports.add("@pied-piper/meter", report("x", Some(1_000_000), 1.0));
    reports.add("@pied-piper/meter", report("x", None, 2.5));
    let cost = price(&priced(), &[], &reports).unwrap();
    assert_eq!(cost.entries[0].usd, Some(Usd::from_pico(1_000_000)));
    assert!(!cost.entries[0].complete);
    assert_eq!(cost.entries[0].usage["input_tokens"], json!(3.5));
}

#[test]
fn dropped_reports_mark_the_total_incomplete() {
    let mut reports = PackageCosts::default();
    for i in 0..=interpreter::runtime::MAX_PACKAGE_COST_ENTRIES {
        reports.add("@pied-piper/meter", report(&format!("l{i}"), Some(1), 1.0));
    }
    let cost = price(&priced(), &[], &reports).unwrap();
    assert!(!cost.complete);
    assert_eq!(cost.dropped_reports, 1);
}

#[test]
fn more_cache_than_input_is_unpriced_not_free() {
    let usage = ModelUsage {
        input_tokens: Some(100),
        output_tokens: Some(1),
        cache_read_tokens: Some(150),
        cache_write_tokens: None,
    };
    let cost = price_calls(&priced(), &[llm("priced", usage)]).unwrap();
    assert_eq!(cost.entries[0].usd, None);
    assert!(!cost.complete);
}

#[test]
fn a_package_label_cannot_forge_a_text_line() {
    let mut reports = PackageCosts::default();
    reports.add("@pied-piper/meter", report("x\ncost: $0", None, 1.0));
    let lines = price(&priced(), &[], &reports).unwrap().lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].contains("x\\ncost"), "{lines:?}");
}

#[test]
fn builtin_prices_apply_only_on_a_first_party_default_endpoint() {
    let Some((id, expected)) = builtin::first_llm_entry_for_tests(ProviderKind::Anthropic) else {
        return;
    };
    let yaml = |base_url: &str| {
        format!(
            r#"
name: cost
secrets:
  KEY: {{ store: KEY }}
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${{secrets.KEY}}{base_url}
  models:
    {id}:
      provider: anthropic
"#
        )
    };
    let calls = [llm(id, tokens(1_000_000, 0))];
    let default = price_calls(&blueprint(&yaml("")), &calls).unwrap();
    assert_eq!(default.entries[0].pricing, Some(PriceSource::Builtin));
    assert_eq!(default.total_usd, expected.input);

    let proxied = blueprint(&yaml("\n      base_url: https://proxy.example"));
    let proxied = price_calls(&proxied, &calls).unwrap();
    assert_eq!(
        proxied.entries[0].usd, None,
        "a base_url override is unpriced"
    );
}

#[test]
fn renders_a_total_line_and_one_line_per_entry() {
    let calls = [
        llm("priced", tokens(1_000, 0)),
        llm("unpriced", tokens(500, 5)),
    ];
    let cost = price_calls(&priced(), &calls).unwrap();
    assert_eq!(
        cost.lines(),
        vec![
            "cost: $0.003 (incomplete: 1 unpriced)".to_owned(),
            "  llm priced: $0.003 (blueprint price)".to_owned(),
            "  llm unpriced: unpriced (input_tokens 500, output_tokens 5)".to_owned(),
        ]
    );
}
