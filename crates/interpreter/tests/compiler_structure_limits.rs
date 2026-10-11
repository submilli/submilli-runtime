//! Structural compiler limits beyond parser recursion. Oversized structures
//! fail with a spanned limit, and programs at each limit compile, on a thread
//! of the documented `COMPILER_STACK_BYTES` in a bounded child process.
use std::process::Command;
use std::time::{Duration, Instant};

use submilli_engine::compiler_error::CompilerFailure;
use submilli_engine::compiler_limits::COMPILER_STACK_BYTES;
use submilli_engine::{
    FileId,
    compile::{compile_script_checked, typecheck_checked},
};

const SYNTAX_LIMIT: &str = "syntax nesting exceeds the compiler limit of 256 levels";

#[test]
fn compiler_structure_limits_are_bounded() {
    if std::env::var_os("SUB633_STRUCTURE_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(COMPILER_STACK_BYTES)
            .spawn(check_structure_limits)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "compiler_structure_limits_are_bounded",
            "--nocapture",
        ])
        .env("SUB633_STRUCTURE_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "structure-limit child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("structure-limit child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn check_structure_limits() {
    let chain = |operator: &str, operand: &str, count: usize| vec![operand; count].join(operator);
    let aliases = |count: usize, body: &dyn Fn(usize) -> String| {
        (1..=count)
            .map(|i| format!("type A{i} = {};", body(i)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let rejected = [
        (
            format!(
                "function main(): number {{ return {}; }}",
                chain(" + ", "1", 10_000)
            ),
            SYNTAX_LIMIT,
        ),
        (
            format!(
                "function main(): boolean {{ const a = true; return {}; }}",
                chain(" && ", "a", 10_000)
            ),
            SYNTAX_LIMIT,
        ),
        (
            format!(
                "function main(): number {{ const f = (): unknown => f; f{}; return 1; }}",
                "()".repeat(10_000)
            ),
            SYNTAX_LIMIT,
        ),
        (
            format!(
                "function main(): number {{ const a = 1; const s = `{}`; return s.length; }}",
                "${a}x".repeat(20_000)
            ),
            "lowered expression nesting exceeds the compiler limit of 1024 levels",
        ),
        (
            format!(
                "type A0 = {{ v: number }};\n{}\nfunction main(): number {{ return 1; }}",
                aliases(10_000, &|i| format!("{{ v: A{} }}", i - 1))
            ),
            "type resolution nests deeper than the compiler limit of 256 levels",
        ),
        (
            format!(
                "type A0 = number;\n{}\nfunction main(): number {{ return 1; }}",
                aliases(10_000, &|i| format!("A{}[]", i - 1))
            ),
            "type resolution nests deeper than the compiler limit of 256 levels",
        ),
        (
            format!(
                "function main(): number {{ const x: number{} | null = null; return 1; }}",
                "[]".repeat(100_000)
            ),
            "parser recursion limit exceeded",
        ),
        (
            format!(
                "type O = {{ a?: number }};\nfunction main(): number {{ const x: O = {{}}; const y = {{ {} }}; return 1; }}",
                chain(", ", "...x", 100_000)
            ),
            "with more than 512 optional spreads",
        ),
        (optional_call_chain(20_000), SYNTAX_LIMIT),
        (
            format!(
                "type O = {{ a: O | null }};\nfunction main(): number {{ const x: O | null = null; const y = x{}; return y === null ? 0 : 1; }}",
                "?.a".repeat(20_000)
            ),
            SYNTAX_LIMIT,
        ),
        (
            (1..=500)
                .map(|i| format!("class C{i} extends C{} {{}}", i - 1))
                .fold("class C0 {}".to_string(), |source, class| {
                    source + "\n" + &class
                })
                + "\nfunction main(): number { return 1; }",
            "more than 62 classes in its inheritance chain",
        ),
    ];
    for (case, (source, expected)) in rejected.iter().enumerate() {
        let error = compile_script_checked(source, "limits.ts", FileId(0), &[], &[])
            .expect_err("oversized structure must not compile");
        let Some(CompilerFailure::Limit { message, span, .. }) = &error.fatal else {
            panic!("case {case}: expected a compiler limit, got {error:?}");
        };
        assert!(message.contains(expected), "case {case}: {message}");
        assert!(span.is_some(), "case {case}: limit lacks a source span");
        healthy_compile_succeeds();
    }

    // Programs exactly at each limit compile on the documented compiler stack.
    let accepted = [
        format!(
            "function main(): number {{ return {}; }}",
            chain(" + ", "1", 253)
        ),
        format!(
            "function main(): number {{ const a = 1; const s = `{}`; return s.length; }}",
            "${a}x".repeat(510)
        ),
        // A0..A126: 127 object-bodied aliases, the documented chain limit.
        format!(
            "type A0 = {{ v: number }};\n{}\nfunction f(x: A126): number {{ return 1; }}\nfunction main(): number {{ return 1; }}",
            aliases(126, &|i| format!("{{ v: A{} }}", i - 1))
        ),
        class_chain("class C0 {}", 61),
        class_chain("class C0 extends Error {}", 60),
        plain_renames(254),
        // The first spread supplies the field; the next 512 override it.
        optional_spreads("", 513),
        optional_spreads("a: 5, ", 512),
        optional_spreads("...required, ", 512),
        // Each `.f()` is two chain links.
        optional_call_chain(126),
        // With the chain, 449 substitutions; one more exceeds the lowered height.
        chain_in_template(448),
    ];
    for (case, source) in accepted.iter().enumerate() {
        if let Err(error) = compile_script_checked(source, "limits.ts", FileId(0), &[], &[]) {
            panic!("accepted case {case} failed: {error:?}");
        }
    }
    // One step over each limit whose boundary is exact: declaration counts,
    // spread overrides, chain links and lowered template height.
    let one_over = [
        // A0..A127: 128 object-bodied aliases.
        (
            format!(
                "type A0 = {{ v: number }};\n{}\nfunction f(x: A127): number {{ return 1; }}\nfunction main(): number {{ return 1; }}",
                aliases(127, &|i| format!("{{ v: A{} }}", i - 1))
            ),
            "type resolution nests deeper",
        ),
        (class_chain("class C0 {}", 62), "more than 62 classes"),
        (
            class_chain("class C0 extends Error {}", 61),
            "more than 62 classes",
        ),
        (plain_renames(255), "type resolution nests deeper"),
        (optional_spreads("", 514), "more than 512 optional spreads"),
        (
            optional_spreads("a: 5, ", 513),
            "more than 512 optional spreads",
        ),
        (
            optional_spreads("...required, ", 513),
            "more than 512 optional spreads",
        ),
        (optional_call_chain(127), SYNTAX_LIMIT),
        (
            chain_in_template(449),
            "lowered expression nesting exceeds the compiler limit of 1024 levels",
        ),
    ];
    for (case, (source, expected)) in one_over.iter().enumerate() {
        let error = compile_script_checked(source, "limits.ts", FileId(0), &[], &[])
            .expect_err("one step over the limit must not compile");
        let Some(CompilerFailure::Limit { message, .. }) = &error.fatal else {
            panic!("one-over case {case}: expected a compiler limit, got {error:?}");
        };
        assert!(
            message.contains(expected),
            "one-over case {case}: {message}"
        );
    }

    // The resolution limit names the annotation that started resolving the
    // chain, not whichever alias the recursion had reached.
    let source = plain_renames(255);
    let error = compile_script_checked(&source, "limits.ts", FileId(0), &[], &[]).unwrap_err();
    let Some(CompilerFailure::Limit {
        span: Some(span), ..
    }) = error.fatal
    else {
        panic!("expected a spanned limit: {error:?}");
    };
    let annotation = source.find("x: A255").unwrap() + "x: ".len();
    assert_eq!(span.start as usize, annotation);

    // At this size the condition's tree fits until the do-while is lowered,
    // so the limit comes from lowering and must name the condition, which
    // starts before its template, not the `do` keyword.
    let condition = format!("0 > `{}`.length", "${a}-".repeat(505));
    let wrapped = |statement: String| {
        format!(
            "function main(): number {{ const a = 1; let t = 0; \
             for (const v of [1]) {{ {statement} }} return t; }}"
        )
    };
    let unlowered = wrapped(format!("if ({condition}) {{ t += 1; }}"));
    if let Err(error) = compile_script_checked(&unlowered, "limits.ts", FileId(0), &[], &[]) {
        panic!("the condition alone must fit: {error:?}");
    }
    let source = wrapped(format!("do {{ t += 1; }} while ({condition});"));
    let error = compile_script_checked(&source, "limits.ts", FileId(0), &[], &[]).unwrap_err();
    let Some(CompilerFailure::Limit {
        span: Some(span),
        message,
        ..
    }) = error.fatal
    else {
        panic!("expected a spanned limit: {error:?}");
    };
    assert!(message.contains("lowered expression nesting"), "{message}");
    assert_eq!(span.start as usize, source.find("0 >").unwrap());

    // Lowering a loop deepens the typed tree; check and compile judge the
    // lowered tree alike on both sides of the limit.
    for (substitutions, compiles) in [(500, true), (510, false)] {
        let source = format!(
            "function main(): number {{ const a = 1; let total = 0; \
             for (const v of [1]) {{ total += `{}`.length; }} return total; }}",
            "${a}-".repeat(substitutions)
        );
        let compiled = compile_script_checked(&source, "limits.ts", FileId(0), &[], &[]);
        assert_eq!(compiled.is_ok(), compiles, "{substitutions}: {compiled:?}");
        let checked = typecheck_checked(&source, FileId(0));
        assert_eq!(checked.is_ok(), compiles, "{substitutions}: {checked:?}");
    }
}

/// `type Ai = A(i-1)` renames; each costs one resolution level.
fn plain_renames(count: usize) -> String {
    let renames = (1..=count)
        .map(|i| format!("type A{i} = A{};", i - 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "type A0 = number;\n{renames}\nfunction f(x: A{count}): number {{ return 1; }}\nfunction main(): number {{ return 1; }}"
    )
}

/// `root` followed by `extensions` classes, each extending the previous one.
fn class_chain(root: &str, extensions: usize) -> String {
    (1..=extensions)
        .map(|i| format!("class C{i} extends C{} {{}}", i - 1))
        .fold(root.to_string(), |source, class| source + "\n" + &class)
        + "\nfunction main(): number { return 1; }"
}

/// An object literal starting with `prefix` and spreading `x` `count` times.
fn optional_spreads(prefix: &str, count: usize) -> String {
    format!(
        "type O = {{ a?: number }};\nfunction main(): number {{ const x: O = {{}}; const required = {{ a: 1 }}; const y = {{ {prefix}{} }}; return y.a ?? 1; }}",
        vec!["...x"; count].join(", ")
    )
}

/// A method-call chain rooted in `?.`, which parses as one flat chain node.
fn optional_call_chain(calls: usize) -> String {
    format!(
        "class N {{ f(): N {{ return this; }} }}\nfunction main(): number {{ const x: N | null = new N(); const y = x?.f(){}; return y === null ? 0 : 1; }}",
        ".f()".repeat(calls.saturating_sub(1))
    )
}

/// A template whose first substitution is a 121-call optional chain, followed
/// by `trailing_substitutions` more: the chain's typed links add to the height
/// of the lowered concatenation.
fn chain_in_template(trailing_substitutions: usize) -> String {
    format!(
        "class N {{ v: number = 1; f(): N {{ return this; }} }}\nfunction maybe(): N | null {{ return new N(); }}\nfunction main(): number {{ const a = 1; const x = maybe(); const s = `${{x?.f(){}.v ?? 0}}{}`; return s.length; }}",
        ".f()".repeat(120),
        ".${a}".repeat(trailing_substitutions)
    )
}

fn healthy_compile_succeeds() {
    compile_script_checked(
        "function main(): number { return 42; }",
        "healthy.ts",
        FileId(0),
        &[],
        &[],
    )
    .expect("healthy follow-up compiles");
}
