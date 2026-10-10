use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use submilli_engine::compiler_limits::{
    COMPILER_STACK_BYTES, MAX_NAMESPACE_DEPTH, MAX_NAMESPACE_NODES, MAX_NAMESPACE_PATH_BYTES,
};
use submilli_engine::type_size::TypeTooLarge;
use submilli_engine::{
    FileId, ModulePath, NamespaceSymbol, PackageDeclaration, PackageSourceModule, Span,
};

#[test]
fn namespace_limits_and_cleanup() {
    if std::env::var_os("SUBMILLI_NAMESPACE_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_namespace_limits)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    // Stack overflows abort; isolate the regression and bound its running time.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "namespace_limits_and_cleanup", "--nocapture"])
        .env("SUBMILLI_NAMESPACE_LIMIT_CHILD", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "namespace child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("namespace child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn namespace() -> NamespaceSymbol {
    NamespaceSymbol {
        name: "N".into(),
        mangled_prefix: submilli_engine::mangle::prelude("N"),
        declaration_span: Span::at(FileId(0)),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    }
}

fn declaration(depth: usize) -> PackageDeclaration {
    let mut child = namespace();
    for _ in 1..depth {
        let mut parent = namespace();
        parent.namespaces.insert("N".into(), child);
        child = parent;
    }
    let mut declaration = PackageDeclaration::with_package("dep");
    declaration.namespaces.insert("N".into(), child);
    declaration
}

fn check_namespace_limits() {
    for depth in [MAX_NAMESPACE_DEPTH, MAX_NAMESPACE_DEPTH + 1, 30_000, 1] {
        let declaration = declaration(depth);
        if depth <= MAX_NAMESPACE_DEPTH {
            assert_eq!(declaration.check_type_limits(), Ok(()));
            assert_eq!(declaration.clone(), declaration);
        } else {
            assert_eq!(
                declaration.check_type_limits(),
                Err(TypeTooLarge::NamespaceMetadata)
            );
        }
        // Also exercise destruction of rejected, publicly constructed trees.
        drop(declaration);
    }
    let mut wide = PackageDeclaration::with_package("dep");
    for index in 0..MAX_NAMESPACE_NODES {
        wide.namespaces.insert(index.to_string(), namespace());
    }
    assert_eq!(wide.check_type_limits(), Ok(()));
    wide.namespaces.insert("excess".into(), namespace());
    assert_eq!(
        wide.check_type_limits(),
        Err(TypeTooLarge::NamespaceMetadata)
    );
    let mut named = PackageDeclaration::with_package("dep");
    named
        .namespaces
        .insert("n".repeat(MAX_NAMESPACE_PATH_BYTES), namespace());
    assert_eq!(named.check_type_limits(), Ok(()));
    named.namespaces.insert("extra".into(), namespace());
    assert_eq!(
        named.check_type_limits(),
        Err(TypeTooLarge::NamespaceMetadata)
    );
    let package_source = "export function main(): number { return 1; }";
    let modules = [PackageSourceModule {
        path: ModulePath::from("lib"),
        source: package_source,
    }];
    let very_deep = declaration(30_000);
    assert!(
        submilli_engine::compile::compile_package_with_transitive_checked(
            "pkg",
            ModulePath::from("lib"),
            &modules,
            &[&very_deep],
            &[],
        )
        .is_err()
    );
    assert!(
        submilli_engine::compile::compile_package_with_transitive_checked(
            "pkg",
            ModulePath::from("lib"),
            &modules,
            &[],
            &[&very_deep],
        )
        .is_err()
    );
    std::thread::Builder::new()
        .stack_size(COMPILER_STACK_BYTES)
        .spawn(|| {
            let source = "export function main(): number { return 1; }";
            let deep = declaration(MAX_NAMESPACE_DEPTH + 1);
            assert!(
                submilli_engine::compile::compile_script_checked(
                    source,
                    "main.ts",
                    FileId(0),
                    &[&deep],
                    &[]
                )
                .is_err()
            );
            let healthy = declaration(2);
            assert!(
                submilli_engine::compile::compile_script_checked(
                    source,
                    "main.ts",
                    FileId(0),
                    &[&healthy],
                    &[]
                )
                .is_ok()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
