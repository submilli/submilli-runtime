//! Type size, depth and work limits end to end: where each is reported, and
//! dependency declarations, which an embedder can build directly rather than
//! read from artifact JSON.
//!
//! Set `SUBMILLI_TEST_NIGHTLY_ONLY=1` to run these limit-sized inputs.
//! `SUBMILLI_FULL_TEST` does not enable them.
use submilli_engine::compile::{
    PackageSourceModule, compile_package_checked, compile_script_checked,
};
use submilli_engine::compiler_error::{CompilerFailure, CompilerStage};
use submilli_engine::compiler_limits::{COMPILER_STACK_BYTES, MAX_TYPE_DEPTH};
use submilli_engine::{FileId, ModulePath, PackageDeclaration, Span, Type, ValueKind, ValueSymbol};

const SCRIPT: &str = "export function main(): number { return 1; }";

fn nested(depth: u32) -> Type {
    (1..depth).fold(Type::Number, |inner, _| Type::Array(Box::new(inner)))
}

fn declaring(ty: Type) -> PackageDeclaration {
    let mut declaration = PackageDeclaration::with_package("dep");
    declaration.values.insert(
        "value".into(),
        ValueSymbol {
            name: "value".into(),
            mangled_name: submilli_engine::mangle::package_symbol("dep", "value"),
            declaration_span: Span::at(FileId(0)),
            kind: ValueKind::Const { ty, doc: None },
        },
    );
    declaration
}

fn on_compiler_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(COMPILER_STACK_BYTES)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_dependency_declaring_a_type_past_the_depth_limit_is_rejected() {
    if !nightly_only_requested() {
        return;
    }
    on_compiler_stack(|| {
        let deep = declaring(nested(MAX_TYPE_DEPTH + 1));
        let Err(error) = compile_script_checked(SCRIPT, "main.ts", FileId(0), &[&deep], &[]) else {
            panic!("an over-deep declaration must not compile");
        };
        match error.fatal {
            Some(CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                message,
                ..
            }) => assert_eq!(
                message,
                format!(
                    "package `dep` declares a type beyond a compiler limit: type nesting exceeds the compiler limit of {MAX_TYPE_DEPTH} levels"
                )
            ),
            other => panic!("expected a type limit, got {other:?}"),
        }
    });
}

#[test]
fn a_dependency_declaring_a_type_at_the_depth_limit_compiles() {
    if !nightly_only_requested() {
        return;
    }
    on_compiler_stack(|| {
        let at_limit = declaring(nested(MAX_TYPE_DEPTH));
        compile_script_checked(SCRIPT, "main.ts", FileId(0), &[&at_limit], &[])
            .expect("a declaration at the limit compiles");
    });
}

/// Aliases `A0`..`A{n}`, each holding two copies of the previous one.
fn doubling_aliases(n: usize) -> String {
    let mut source = "type A0 = { v: number };\n".to_string();
    for i in 1..=n {
        source.push_str(&format!(
            "type A{i} = {{ a: A{}; b: A{} }};\n",
            i - 1,
            i - 1
        ));
    }
    source
}

/// The limit a script fails with and the source text it points at.
fn limit_of(source: String) -> (String, String) {
    let (tx, rx) = std::sync::mpsc::channel();
    on_compiler_stack(move || {
        let Err(error) = compile_script_checked(&source, "main.ts", FileId(0), &[], &[]) else {
            panic!("the program must hit a limit");
        };
        let Some(CompilerFailure::Limit {
            message,
            span: Some(span),
            ..
        }) = error.fatal
        else {
            panic!("expected a located limit, got {:?}", error.fatal);
        };
        let text = source[span.start as usize..span.end as usize].to_string();
        tx.send((message, text)).unwrap();
    });
    rx.recv().unwrap()
}

#[test]
fn an_oversized_alias_is_reported_at_its_reference() {
    if !nightly_only_requested() {
        return;
    }
    let source = doubling_aliases(14)
        + "function f(x: A14 | null): number { return 0; }\n\
           export function main(): number { return f(null); }\n";
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        "type is larger than the compiler limit of 65536 parts"
    );
    assert_eq!(text, "A14");
}

#[test]
fn an_oversized_call_result_is_reported_at_the_call() {
    if !nightly_only_requested() {
        return;
    }
    let mut source = "function pair<T>(x: T): { a: T; b: T } { return { a: x, b: x }; }\n\
                      export function main(): number {\n  const r0 = 1;\n"
        .to_string();
    for i in 1..=16 {
        source.push_str(&format!("  const r{i} = pair(r{});\n", i - 1));
    }
    source.push_str("  return 1;\n}\n");
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        "type is larger than the compiler limit of 65536 parts"
    );
    assert_eq!(text, "pair(r15)");
}

#[test]
fn an_over_deep_inferred_type_is_reported_at_its_value() {
    if !nightly_only_requested() {
        return;
    }
    let mut source = "export function main(): number {\n  const o0 = 1;\n".to_string();
    for i in 1..=MAX_TYPE_DEPTH {
        source.push_str(&format!("  const o{i} = {{ v: o{} }};\n", i - 1));
    }
    source.push_str("  return 1;\n}\n");
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        format!("type nesting exceeds the compiler limit of {MAX_TYPE_DEPTH} levels")
    );
    assert_eq!(text, format!("{{ v: o{} }}", MAX_TYPE_DEPTH - 1));
}

#[test]
fn an_inference_limit_on_a_multi_line_value_is_cut_to_its_first_line() {
    if !nightly_only_requested() {
        return;
    }
    let mut source = "export function main(): number {\n  const o0 = 1;\n".to_string();
    for i in 1..MAX_TYPE_DEPTH {
        source.push_str(&format!("  const o{i} = {{ v: o{} }};\n", i - 1));
    }
    // The last value spans lines; `\r` ends a line as `\n` does.
    source.push_str(&format!(
        "  const deep = {{\r    v: o{},\r    w: 1,\r  }};\n  return 1;\n}}\n",
        MAX_TYPE_DEPTH - 1
    ));
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        format!("type nesting exceeds the compiler limit of {MAX_TYPE_DEPTH} levels")
    );
    assert_eq!(text, "{");
}

#[test]
fn a_limit_met_checking_a_returned_value_is_reported_at_the_value() {
    if !nightly_only_requested() {
        return;
    }
    let source = doubling_aliases(13)
        + "interface Pair<T> { p: [T, T] }\n\
           interface Other<T> { p: [T, T] }\n\
           function f(o: Other<A13>): Pair<A13> {\n  return o;\n}\n\
           function unrelated(): number {\n  const answer = 40 + 2;\n  return answer;\n}\n\
           export function main(): number { return unrelated(); }\n";
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        "type is larger than the compiler limit of 65536 parts"
    );
    assert_eq!(text, "o");
}

#[test]
fn a_limit_met_checking_a_declaration_is_reported_at_that_declaration() {
    if !nightly_only_requested() {
        return;
    }
    let mut source = "const unrelated = 42;\n\
                      class C0<T> { v: T; constructor(v: T) { this.v = v; } m(x: T): number { return 0; } }\n"
        .to_string();
    for i in 1..=18 {
        source.push_str(&format!(
            "class C{i}<T> extends C{}<{{ a: T; b: T }}> {{ constructor(v: T) {{ super({{ a: v, b: v }}); }} }}\n",
            i - 1
        ));
    }
    source.push_str(
        "class D extends C18<number> { constructor() { super(1); } m(x: unknown): number { return 1; } }\n\
         export function main(): number { return unrelated; }\n",
    );
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        "type is larger than the compiler limit of 65536 parts"
    );
    assert!(text.starts_with("m(x: unknown)"), "{text}");
}

#[test]
fn comparing_types_draws_on_the_work_limit() {
    if !nightly_only_requested() {
        return;
    }
    // Two isomorphic families of interfaces over unions of the same literals in
    // opposite orders: every level compares both members of the next.
    let members = |order: &mut dyn Iterator<Item = usize>| {
        order
            .map(|i| format!("\"s{i}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let mut source = format!(
        "type BigA = {};\ntype BigB = {};\ninterface I0 {{ v: BigA }}\ninterface J0 {{ v: BigB }}\n",
        members(&mut (0..200)),
        members(&mut (0..200).rev()),
    );
    for i in 1..=16 {
        source.push_str(&format!(
            "interface I{i} {{ a: I{p}; b: I{p} }}\ninterface J{i} {{ a: J{p}; b: J{p} }}\n",
            p = i - 1
        ));
    }
    source.push_str(
        "function f(x: I16): J16 { return x; }\nexport function main(): number { return 0; }\n",
    );
    let (message, _) = limit_of(source);
    assert_eq!(
        message,
        format!(
            "building and comparing types takes more than the compiler limit of {} steps",
            submilli_engine::compiler_limits::MAX_TYPE_WORK
        )
    );
}

#[test]
fn a_package_rejects_a_dependency_past_the_depth_limit() {
    if !nightly_only_requested() {
        return;
    }
    on_compiler_stack(|| {
        let deep = declaring(nested(MAX_TYPE_DEPTH + 1));
        let Err(error) = compile_package_checked(
            "lib",
            ModulePath::from("lib"),
            &[PackageSourceModule {
                path: ModulePath::from("lib"),
                source: "export function f(): number { return 1; }",
            }],
            &[&deep],
        ) else {
            panic!("an over-deep dependency must not compile");
        };
        assert!(
            matches!(
                &error.fatal,
                Some(CompilerFailure::Limit { message, .. })
                    if message.starts_with("package `dep` declares a type beyond a compiler limit")
            ),
            "{:?}",
            error.fatal
        );
    });
}

#[test]
fn a_runtime_check_too_large_for_one_function_is_reported_at_the_cast() {
    if !nightly_only_requested() {
        return;
    }
    // Each UTF-16 unit of a string literal tested at runtime is several
    // instructions, so eight fields of one long literal type outgrow a function
    // body long before the check runs out of steps or locals.
    let literal = "a".repeat(100_000);
    let fields: Vec<String> = (0..8).map(|i| format!("f{i}: L")).collect();
    let source = format!(
        "type L = \"{literal}\";\n\
         interface I {{ {} }}\n\
         export function main(): number {{\n  const v = JSON.parse(\"null\") as I | null;\n  return v === null ? 0 : 1;\n}}\n",
        fields.join("; ")
    );
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        format!(
            "the runtime check of `I | null` makes its function larger than the {} bytes a function may have",
            submilli_engine::compiler_limits::MAX_FUNCTION_BODY_BYTES
        )
    );
    assert_eq!(text, "JSON.parse(\"null\") as I | null");
}

#[test]
fn a_function_with_too_many_locals_is_reported_where_it_runs_out() {
    if !nightly_only_requested() {
        return;
    }
    let locals = submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS + 10;
    let body: String = (0..locals)
        .map(|i| format!("  const v{i} = {i};\n"))
        .collect();
    let source = format!("export function main(): number {{\n{body}  return 0;\n}}\n");
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        format!(
            "the function this code compiles into needs more than the {} locals a function may have",
            submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS
        )
    );
    // The initializer of the binding whose local crosses the limit, not the
    // whole body.
    assert_eq!(
        text,
        submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS.to_string()
    );
}

/// A function `name` whose `for…of` loop's own desugared locals cross the
/// locals limit. Those locals carry placeholder spans, so the error must name
/// the loop they came from, cut to its first line, not the start of the file.
fn function_whose_loop_crosses_the_locals_limit(name: &str) -> String {
    // Seven locals short of the limit leaves `arr` and `s` within it, so the
    // loop's own locals cross it.
    let consts = submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS - 7;
    let body: String = (0..consts)
        .map(|i| format!("  const v{i} = {i};\n"))
        .collect();
    format!(
        "export function {name}(): number {{\n{body}  const arr = [1, 2];\n  let s = 0;\n  for (const x of arr) {{\n    s = s + x;\n  }}\n  return s;\n}}\n"
    )
}

#[test]
fn desugared_code_crossing_the_locals_limit_is_reported_at_its_source() {
    if !nightly_only_requested() {
        return;
    }
    let (message, text) = limit_of(function_whose_loop_crosses_the_locals_limit("main"));
    assert!(
        message.starts_with("the function this code compiles into"),
        "{message}"
    );
    assert_eq!(text, "for (const x of arr) {");
}

#[test]
fn a_check_after_code_that_crossed_the_locals_limit_does_not_take_the_blame() {
    if !nightly_only_requested() {
        return;
    }
    let locals = submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS + 10;
    let body: String = (0..locals)
        .map(|i| format!("  const v{i} = {i};\n"))
        .collect();
    let source = format!(
        "interface I {{ a: string }}\n\
         export function main(): number {{\n{body}  const o = JSON.parse(\"null\") as I | null;\n  return o === null ? 0 : 1;\n}}\n"
    );
    let (message, text) = limit_of(source);
    assert_eq!(
        message,
        format!(
            "the function this code compiles into needs more than the {} locals a function may have",
            submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS
        )
    );
    assert_eq!(
        text,
        submilli_engine::compiler_limits::MAX_FUNCTION_LOCALS.to_string()
    );
}

#[test]
fn a_limit_in_another_package_module_is_cut_to_its_first_line() {
    if !nightly_only_requested() {
        return;
    }
    let helper = function_whose_loop_crosses_the_locals_limit("g");
    let (tx, rx) = std::sync::mpsc::channel();
    on_compiler_stack(move || {
        let Err(error) = compile_package_checked(
            "big",
            ModulePath::from("lib"),
            &[
                PackageSourceModule {
                    path: ModulePath::from("lib"),
                    source: "import { g } from \"./helper\";\nexport function f(): number { return g(); }\n",
                },
                PackageSourceModule {
                    path: ModulePath::from("helper"),
                    source: &helper,
                },
            ],
            &[],
        ) else {
            panic!("the helper must hit the locals limit");
        };
        let Some(CompilerFailure::Limit {
            span: Some(span), ..
        }) = error.fatal
        else {
            panic!("expected a located limit, got {:?}", error.fatal);
        };
        let text = helper
            .get(span.start as usize..span.end as usize)
            .map(str::to_owned);
        tx.send(text).unwrap();
    });
    assert_eq!(
        rx.recv().unwrap().as_deref(),
        Some("for (const x of arr) {")
    );
}

fn nightly_only_requested() -> bool {
    let requested = std::env::var("SUBMILLI_TEST_NIGHTLY_ONLY").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    });
    if !requested {
        eprintln!("type limits: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
    }
    requested
}
