use std::sync::{Arc, Mutex};

use crate::runtime::security::{CheckOutcome, SecurityCheck};
use crate::runtime::{
    MAX_PACKAGE_COST_ENTRIES, PackageCost, RuntimeConfig, StoreData, Vfs, dispatch_main_async,
    install_runtime_async,
};

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<(String, String, serde_json::Value)>>,
}

impl SecurityCheck for Recorder {
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        self.seen
            .lock()
            .unwrap()
            .push((caller.into(), capability.into(), context.clone()));
        CheckOutcome::Allow { rule: None }
    }
}

struct Run {
    result: wasmtime::Result<Option<String>>,
    costs: Vec<PackageCost>,
    dropped: u64,
}

async fn run_as(
    package: Option<&str>,
    source: &str,
    policy: Option<Arc<dyn SecurityCheck>>,
) -> Run {
    let compiled = match package {
        Some(package) => crate::compile::compile_script_owned_by(
            package,
            source,
            "cost.ts",
            crate::FileId(0),
            &[],
            &[],
        ),
        None => crate::compile_script(source, "cost.ts", crate::FileId(0), &[], &[]),
    }
    .expect("program compiles");
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
    if let Some(policy) = policy {
        data.security_check = policy;
    }
    let mut store = config.store_async(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let result = dispatch_main_async(&mut store, &instance).await;
    let costs = store.data().package_costs();
    Run {
        result,
        costs: costs.entries().to_vec(),
        dropped: costs.dropped(),
    }
}

#[tokio::test]
async fn a_package_report_is_kept_under_the_package_the_host_saw() {
    let policy = Arc::new(Recorder::default());
    let run = run_as(
        Some("@pied-piper/meter"),
        r#"
        import { report } from "submilli:cost";
        function main(): void {
            report({ usd: 0.00000042, usage: { input_tokens: 10, output_tokens: 0 }, label: "jev-1.14.0" });
            report({ usd: 0.00000084, usage: { input_tokens: 20 }, label: "jev-1.14.0" });
            report({ usage: { requests: 1.5 } });
        }
    "#,
        Some(policy.clone()),
    )
    .await;
    run.result.expect("reports succeed");
    assert_eq!(
        run.costs,
        vec![
            PackageCost {
                package: "@pied-piper/meter".into(),
                label: Some("jev-1.14.0".into()),
                pico_usd: Some(1_260_000),
                unpriced_reports: 0,
                usage: vec![("input_tokens".into(), 30.0), ("output_tokens".into(), 0.0)],
            },
            PackageCost {
                package: "@pied-piper/meter".into(),
                label: None,
                pico_usd: None,
                unpriced_reports: 1,
                usage: vec![("requests".into(), 1.5)],
            },
        ]
    );
    let seen = policy.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].0, "@pied-piper/meter");
    assert_eq!(seen[0].1, "cost.report");
    assert_eq!(seen[0].2, serde_json::json!({}));
}

#[tokio::test]
async fn main_is_refused_even_with_allow_all() {
    let run = run_as(
        None,
        r#"
        import { report } from "submilli:cost";
        function main(): void {
            let denied = false;
            try { report({ usd: 1 }); } catch (error: unknown) {
                denied = error instanceof PermissionDeniedError;
            }
            assert(denied, "main must never report cost");
        }
    "#,
        None,
    )
    .await;
    run.result.expect("the refusal is catchable");
    assert!(run.costs.is_empty());
}

#[tokio::test]
async fn a_bad_report_is_a_catchable_type_error() {
    let run = run_as(
        Some("@pied-piper/meter"),
        r#"
        import { report } from "submilli:cost";
        function refused(attempt: () => void): boolean {
            try { attempt(); } catch (error: unknown) { return error instanceof TypeError; }
            return false;
        }
        function main(): void {
            assert(refused(() => report({ usd: -1 })), "negative usd");
            assert(refused(() => report({ usd: NaN })), "NaN usd");
            assert(refused(() => report({ usd: Infinity })), "infinite usd");
            assert(refused(() => report({})), "empty report");
            assert(refused(() => report({ usage: { tokens: -3 } })), "negative usage");
            assert(refused(() => report({ usd: 1, label: "x".repeat(257) })), "long label");
            assert(refused(() => report({ usage: { "": 1 } })), "empty unit");
            const many: { [unit: string]: number } = {};
            for (let i = 0; i < 33; i++) { many["u" + i] = 1; }
            assert(refused(() => report({ usage: many })), "33 units");
            assert(refused(() => report({ usage: { "\ud800": 1 } })), "lone surrogate unit");
        }
    "#,
        None,
    )
    .await;
    run.result.expect("every refusal is caught");
    assert!(run.costs.is_empty());
}

#[tokio::test]
async fn the_longest_label_and_a_non_bmp_label_are_kept() {
    let run = run_as(
        Some("@pied-piper/meter"),
        r#"
        import { report } from "submilli:cost";
        function main(): void {
            report({ usd: 1, label: "x".repeat(256) });
            report({ usd: 1, label: "😀" });
        }
    "#,
        None,
    )
    .await;
    run.result.expect("reports succeed");
    assert_eq!(run.costs[0].label.as_deref().map(str::len), Some(256));
    assert_eq!(run.costs[1].label.as_deref(), Some("😀"));
}

/// Reports sum by package and label as they arrive, so a loop of reports holds
/// one entry, and distinct labels past the cap are counted as dropped.
#[tokio::test]
async fn reports_sum_on_arrival_and_entries_past_the_cap_are_dropped() {
    let run = run_as(
        Some("@pied-piper/meter"),
        r#"
        import { report } from "submilli:cost";
        function main(): void {
            for (let i = 0; i < 10000; i++) { report({ usd: 0.000001, label: "same" }); }
            for (let i = 0; i < 300; i++) { report({ usd: 0.000001, label: "l" + i }); }
        }
    "#,
        None,
    )
    .await;
    run.result.expect("reports succeed");
    assert_eq!(run.costs.len(), MAX_PACKAGE_COST_ENTRIES);
    assert_eq!(run.costs[0].pico_usd, Some(10_000_000_000));
    assert_eq!(run.dropped, 300 - (MAX_PACKAGE_COST_ENTRIES as u64 - 1));
}

/// A report whose new units would take its entry past the unit cap is dropped
/// whole, so an entry never holds a partial report.
#[tokio::test]
async fn a_report_past_the_unit_cap_of_its_entry_is_dropped_whole() {
    let run = run_as(
        Some("@pied-piper/meter"),
        r#"
        import { report } from "submilli:cost";
        function main(): void {
            const first: { [unit: string]: number } = {};
            for (let i = 0; i < 32; i++) { first["a" + i] = 1; }
            report({ usd: 1, usage: first });
            report({ usd: 1, usage: { b: 1 } });
            report({ usd: 1, usage: { a0: 1 } });
        }
    "#,
        None,
    )
    .await;
    run.result.expect("reports succeed");
    assert_eq!(run.dropped, 1);
    assert_eq!(run.costs[0].pico_usd, Some(2_000_000_000_000));
    assert_eq!(run.costs[0].usage[0], ("a0".to_string(), 2.0));
}
