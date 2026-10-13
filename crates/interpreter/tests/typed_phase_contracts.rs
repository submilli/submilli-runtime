//! Direct phase entry points share dependency admission with full compilation,
//! and compiler-produced deep payloads pass every post-inference phase. Run in a
//! child process so a native stack overflow is reported as a failure.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use submilli_engine::compiler_error::{CompilerFailure, CompilerStage};
use submilli_engine::compiler_limits::{COMPILER_STACK_BYTES, MAX_TYPE_DEPTH, MAX_TYPE_NODES};
use submilli_engine::{FileId, PackageDeclaration, Sources, Type};

const SOURCE: &str = "function main(): number { return 1; }";
const CHILD: &str = "SUBMILLI_TYPED_PHASE_TEST_CHILD";

#[test]
fn direct_phases_respect_type_payload_contracts() {
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(COMPILER_STACK_BYTES)
            .spawn(|| {
                dependency_limits();
                compiler_produced_payloads();
            })
            .expect("compiler thread")
            .join()
            .expect("phase checks did not panic");
        return;
    }
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "direct_phases_respect_type_payload_contracts",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phase test subprocess");
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().expect("poll child").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("stop timed out child");
            child.wait().expect("reap timed out child");
            panic!("phase test exceeded 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("read child output");
    assert!(
        output.status.success(),
        "phase test exited {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dependency_limits() {
    let (ast, diagnostics) = submilli_engine::compile::typecheck_to_typed_ast(SOURCE, FileId(0));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0)).unwrap();
    let (sources, file) = Sources::single("typed.ts", SOURCE).unwrap();
    let mut dependency = PackageDeclaration::with_package("dep");
    let symbol = submilli_engine::mangle::package_symbol("dep", "value");
    for depth in [MAX_TYPE_DEPTH, MAX_TYPE_DEPTH + 1, 100_000] {
        let ty = (1..depth).fold(Type::Number, |inner, _| Type::Array(Box::new(inner)));
        // Physical runtime metadata is cloned by lowering even when unused.
        dependency.runtime_globals.insert(symbol.clone(), ty);
        let dependencies = runtime_dependencies(Some(&dependency));
        let rules = submilli_engine::typechecker::rules::check_script(&ast, &dependencies);
        let generated = [
            submilli_engine::codegen::codegen(SOURCE, "typed.ts", file, &ast, &dependencies),
            submilli_engine::codegen::codegen_with_type_info(
                SOURCE,
                "typed.ts",
                file,
                &ast,
                &dependencies,
            )
            .map(|output| output.wasm),
            submilli_engine::codegen::codegen_owned_by(
                "owner",
                SOURCE,
                "typed.ts",
                file,
                &ast,
                &dependencies,
            )
            .map(|output| output.wasm),
            submilli_engine::codegen::codegen_package_with_type_info(
                &sources,
                file,
                &ast,
                &dependencies,
            )
            .map(|output| output.wasm),
        ];
        // The APIs borrow the declaration; its owner is responsible for disposing
        // of a deliberately out-of-contract type without recursive drop.
        let mut ty = dependency.runtime_globals.remove(&symbol).unwrap();
        while let Type::Array(inner) = ty {
            ty = *inner;
        }
        if depth == MAX_TYPE_DEPTH {
            assert!(rules.unwrap().is_empty());
            for output in generated {
                assert!(!output.unwrap().is_empty());
            }
        } else {
            assert_limit(
                rules.unwrap_err().fatal.unwrap(),
                CompilerStage::Infer,
                "type nesting",
            );
            for output in generated {
                assert_limit(output.unwrap_err(), CompilerStage::Codegen, "type nesting");
            }
        }
    }
    dependency.runtime_globals.insert(
        symbol,
        Type::Union(vec![Type::Number; MAX_TYPE_NODES as usize]),
    );
    let dependencies = runtime_dependencies(Some(&dependency));
    let failure = submilli_engine::codegen::codegen(SOURCE, "typed.ts", file, &ast, &dependencies)
        .unwrap_err();
    assert_limit(failure, CompilerStage::Codegen, "type is larger");
    // The same AST still compiles once the rejected metadata is gone.
    dependency.runtime_globals.clear();
    let dependencies = runtime_dependencies(Some(&dependency));
    assert!(
        submilli_engine::typechecker::rules::check_script(&ast, &dependencies)
            .unwrap()
            .is_empty()
    );
    let output = submilli_engine::codegen::codegen(SOURCE, "typed.ts", file, &ast, &dependencies);
    assert!(!output.unwrap().is_empty());
}

/// The runtime package declarations every script compiles against, plus `extra`.
fn runtime_dependencies(extra: Option<&PackageDeclaration>) -> Vec<&PackageDeclaration> {
    let (prelude, host, internal) =
        submilli_engine::runtime::prelude::cached_runtime_package_declarations();
    prelude
        .iter()
        .chain(host)
        .chain(internal)
        .chain(extra)
        .collect()
}

fn assert_limit(failure: CompilerFailure, expected_stage: CompilerStage, text: &str) {
    let CompilerFailure::Limit { stage, message, .. } = failure else {
        panic!("expected limit, got {failure}");
    };
    assert_eq!(stage, expected_stage);
    assert!(message.contains(text), "{message}");
}

fn compiler_produced_payloads() {
    // Inferred nested object types travel through params, closure captures,
    // declaration metadata and runtime-value lowering, without hand-built IR.
    let mut source = String::from("type A0 = number;\n");
    for depth in 1..100 {
        source.push_str(&format!("type A{depth} = {{ value: A{} }};\n", depth - 1));
    }
    source.push_str("function inspect(value: A99): void { [1].forEach((x: number) => { const copy = value; }); }\nfunction main(): number { return 1; }");
    let (ast, diagnostics) = submilli_engine::compile::typecheck_to_typed_ast(&source, FileId(0));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let extent = submilli_engine::type_size::measure(
        &ast.functions[0].params[0].ty,
        MAX_TYPE_NODES,
        MAX_TYPE_DEPTH,
    )
    .unwrap();
    assert!(extent.depth >= 100, "{extent:?}");
    assert!(submilli_engine::check(&ast).unwrap().is_empty());
    let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0)).unwrap();
    let dependencies = runtime_dependencies(None);
    let output =
        submilli_engine::codegen::codegen(&source, "typed.ts", FileId(0), &ast, &dependencies)
            .unwrap();
    assert!(!output.is_empty());
}
