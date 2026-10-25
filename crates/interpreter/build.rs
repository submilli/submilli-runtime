fn main() {
    println!("cargo:rustc-check-cfg=cfg(skip_http_tests)");
    println!("cargo:rerun-if-env-changed=SUBMILLI_SKIP_HTTP_TESTS");
    if std::env::var_os("SUBMILLI_SKIP_HTTP_TESTS").is_some_and(|value| value == "1") {
        println!("cargo:rustc-cfg=skip_http_tests");
    }
    verify_packaged_manifests();
}

/// The published crate cannot see the workspace root, so the manifests it checks
/// are copies under `compiler-identity/`. In this repository those copies must
/// be the workspace files, or the stamp would describe a different compiler
/// than the one being built.
fn verify_packaged_manifests() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    println!("cargo:rerun-if-changed=compiler-identity/workspace-Cargo.toml");
    println!("cargo:rerun-if-changed=compiler-identity/workspace-Cargo.lock");
    let Some(workspace) = workspace_root(&manifest_dir) else {
        return;
    };
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    assert_same(
        &workspace.join("Cargo.toml"),
        &manifest_dir.join("compiler-identity/workspace-Cargo.toml"),
    );
    assert_same(
        &workspace.join("Cargo.lock"),
        &manifest_dir.join("compiler-identity/workspace-Cargo.lock"),
    );
}

fn workspace_root(manifest_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let candidate = manifest_dir.join("../..");
    let member = candidate.join("crates/interpreter");
    let manifest = std::fs::canonicalize(manifest_dir).ok()?;
    let member = std::fs::canonicalize(member).ok()?;
    if manifest == member {
        std::fs::canonicalize(candidate).ok()
    } else {
        None
    }
}

fn assert_same(workspace: &std::path::Path, packaged: &std::path::Path) {
    let left = std::fs::read(workspace)
        .unwrap_or_else(|error| panic!("read workspace manifest {}: {error}", workspace.display()));
    let right = std::fs::read(packaged).unwrap_or_else(|error| {
        panic!(
            "compiler identity file {} is missing ({error}); copy {} to {}",
            packaged.display(),
            workspace.display(),
            packaged.display()
        )
    });
    if left != right {
        panic!(
            "{} does not match {}. Copy the workspace file over the packaged one so the published crate checks the manifests this build used.",
            packaged.display(),
            workspace.display()
        );
    }
}
