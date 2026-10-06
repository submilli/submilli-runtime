use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use submilli_blueprint::FilterExpr;

#[test]
fn filter_limits_on_request_stack() {
    if std::env::var_os("SUBMILLI_FILTER_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_filter_limits)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    // Stack overflows abort; isolate the regression and bound its running time.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "filter_limits_on_request_stack", "--nocapture"])
        .env("SUBMILLI_FILTER_LIMIT_CHILD", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "filter child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("filter child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check_filter_limits() {
    for count in [128, 129, 3_000] {
        for source in [
            format!("{}x == 1", "not ".repeat(count)),
            format!("{}x == 1{}", "(".repeat(count), ")".repeat(count)),
            vec!["x == 1"; count + 1].join(" and "),
            vec!["x == 1"; count + 1].join(" or "),
        ] {
            let result = source.parse::<FilterExpr>();
            if count > 128 {
                assert!(result.unwrap_err().contains("operators/groups"));
            } else {
                let expression = result.unwrap();
                let cloned = expression.clone();
                assert_eq!(expression, cloned);
                assert!(expression.matches(&serde_json::json!({"x": 1})));
                let vars = Default::default();
                assert!(
                    expression
                        .explain_with(&serde_json::json!({"x": 1}), &vars)
                        .matched
                );
                assert!(!expression.top_level_fields().is_empty());
                assert!(expression.var_refs().is_empty());
                let _ = expression.field_matches("x");
                let encoded = serde_json::to_string(&expression).unwrap();
                assert_eq!(expression, serde_json::from_str(&encoded).unwrap());
            }
            assert!(
                "x == 1"
                    .parse::<FilterExpr>()
                    .unwrap()
                    .matches(&serde_json::json!({"x": 1}))
            );
        }
    }
    let mixed = format!("{}x == 1{}", "not (".repeat(64), ")".repeat(64));
    let expression: FilterExpr = mixed.parse().unwrap();
    assert!(expression.matches(&serde_json::json!({"x": 1})));
    assert!(format!("not {mixed}").parse::<FilterExpr>().is_err());
    // A malformed suffix drops a fully constructed maximum-height tree.
    let chain = vec!["x == 1"; 129].join(" and ");
    assert!(format!("{chain} x").parse::<FilterExpr>().is_err());
    let yaml = format!(
        "name: x\npermissions:\n  main:\n    - capability: c\n      filter: {}x == 1{}\n      action: allow\n",
        "(".repeat(3_000),
        ")".repeat(3_000)
    );
    assert!(
        submilli_blueprint::parse(&yaml)
            .unwrap_err()
            .to_string()
            .contains("operators/groups")
    );
    assert!(submilli_blueprint::parse("name: healthy").is_ok());
    assert!(
        " ".repeat(65_537)
            .parse::<FilterExpr>()
            .unwrap_err()
            .contains("bytes")
    );
}

#[test]
fn accepted_filter_byte_boundary_round_trips() {
    let source = format!("x == \"{}\"", "a".repeat(65_529));
    assert_eq!(source.len(), 65_536);
    let expression: FilterExpr = source.parse().unwrap();
    let encoded = serde_json::to_string(&expression).unwrap();
    assert_eq!(expression, serde_json::from_str(&encoded).unwrap());
    let compact = format!("x==\"{}\"", "a".repeat(65_531));
    assert_eq!(compact.len(), 65_536);
    assert!(
        compact
            .parse::<FilterExpr>()
            .unwrap_err()
            .contains("formatted filter")
    );
    let escaped = format!("x==\"{}\"", "\n".repeat(40_000));
    assert!(
        escaped
            .parse::<FilterExpr>()
            .unwrap_err()
            .contains("formatted filter")
    );
}
