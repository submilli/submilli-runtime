//! Abort regressions run in a bounded child, never in the shared test process.
use std::process::Command;
use std::time::{Duration, Instant};

use interpreter::{Asi, FileId, TokenKind, parse, parse_script};

#[test]
fn parser_depth_is_bounded() {
    if std::env::var_os("SUB633_PARSER_CHILD").is_some() {
        for stack_size in [2 * 1024 * 1024, 8 * 1024 * 1024] {
            std::thread::Builder::new()
                .stack_size(stack_size)
                .spawn(check_recursive_syntax)
                .unwrap()
                .join()
                .unwrap();
        }
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "parser_depth_is_bounded", "--nocapture"])
        .env("SUB633_PARSER_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "parser child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("parser child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn check_recursive_syntax() {
    let depth = 2048;
    let cases = [
        format!("return {}1{};", "(".repeat(depth), ")".repeat(depth)),
        format!("return {}1;", "(".repeat(depth)),
        format!("return {}true;", "!".repeat(depth)),
        format!("return {}1;", "1 ** ".repeat(depth)),
        format!("return {}1;", "x = ".repeat(depth)),
        format!("{}return 1;{}", "{".repeat(depth), "}".repeat(depth)),
        format!("{}return 1;", "if (true) ".repeat(depth)),
        format!("{}return 1;", "if (true) {{}} else ".repeat(depth)),
        format!(
            "type T = {}number{};",
            "Array<".repeat(depth),
            ">".repeat(depth)
        ),
        format!("type T = {}number;", "keyof ".repeat(depth)),
        format!("const f = (): {}number => 1;", "keyof ".repeat(depth)),
        format!("const f = (): {}number => 1;", "() => ".repeat(depth)),
        format!("const f = (): {}number => 1;", "x is ".repeat(depth)),
        format!(
            "return f<{}number{}>(1);",
            "Array<".repeat(depth),
            ">".repeat(depth)
        ),
    ];
    for (case, body) in cases.into_iter().enumerate() {
        let source = format!("function main(): number {{ {body} }}");
        let mut asi = Asi::new(&source, FileId(0));
        let mut tokens = Vec::new();
        loop {
            let token = asi.next_token();
            let eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if eof {
                break;
            }
        }
        let (_, diagnostics) = parse(&source, tokens, FileId(0));
        assert!(
            !diagnostics.is_empty(),
            "direct parser accepted case {case}"
        );
        let parsed = parse_script(&source, FileId(0));
        assert!(parsed.has_errors(), "pipeline accepted case {case}");
        if case == 0 || case == 13 {
            assert!(
                parsed
                    .diagnostics()
                    .iter()
                    .any(|d| d.message == "parser recursion limit exceeded"),
                "{case}: {:?}",
                parsed.diagnostics()
            );
        }
        let healthy = parse_script("function main(): number { return 42; }", FileId(0));
        assert!(!healthy.has_errors(), "{:?}", healthy.diagnostics());
    }
}
