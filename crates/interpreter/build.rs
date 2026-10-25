fn main() {
    println!("cargo:rustc-check-cfg=cfg(skip_http_tests)");
    println!("cargo:rerun-if-env-changed=SUBMILLI_SKIP_HTTP_TESTS");
    if std::env::var_os("SUBMILLI_SKIP_HTTP_TESTS").is_some_and(|value| value == "1") {
        println!("cargo:rustc-cfg=skip_http_tests");
    }
    println!(
        "cargo:rustc-env=SUBMILLI_COMPILER_FINGERPRINT={}",
        compiler_fingerprint()
    );
}

/// Hash of the compiler sources. Installed packages record it so an unreleased
/// compiler change rebuilds them without a hand-bumped schema version.
///
/// Paths use `/` and are sorted, so the stamp does not depend on the host.
/// `tests` directories and dotfiles are omitted; a change that only lives there
/// cannot affect emitted wasm.
fn compiler_fingerprint() -> String {
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let src = PathBuf::from(manifest_dir).join("src");
    println!("cargo:rerun-if-changed=src");
    let mut files = Vec::new();
    collect(&src, &src, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, bytes) in &files {
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read compiler sources in {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("read compiler source entry: {error}"));
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "tests" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("read compiler source {}: {error}", path.display()));
        out.push((relative, bytes));
    }
}
