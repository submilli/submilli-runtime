//! Generates the bundled skill file table from `skills/submilli`, so a file
//! added to the skill ships without a matching source edit.
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() -> io::Result<()> {
    println!("cargo:rustc-check-cfg=cfg(skip_http_tests)");
    println!("cargo:rerun-if-env-changed=SUBMILLI_SKIP_HTTP_TESTS");
    if env::var_os("SUBMILLI_SKIP_HTTP_TESTS").is_some_and(|value| value == "1") {
        println!("cargo:rustc-cfg=skip_http_tests");
    }
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let skill_dir = manifest_dir.join("../../skills/submilli").canonicalize()?;
    // Watching the directory re-runs this script when files are added or removed.
    println!("cargo:rerun-if-changed={}", skill_dir.display());

    let mut files = Vec::new();
    collect_files(&skill_dir, &mut files)?;
    files.sort();

    let mut table = String::from("&[\n");
    for file in &files {
        let relative = file
            .strip_prefix(&skill_dir)
            .expect("collected under skill_dir");
        let name = relative
            .iter()
            .map(|part| part.to_str().expect("skill file names are UTF-8"))
            .collect::<Vec<_>>()
            .join("/");
        table.push_str(&format!(
            "    ({name:?}, include_str!({:?})),\n",
            file.display().to_string()
        ));
    }
    table.push(']');

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    fs::write(out_dir.join("skill_files.rs"), table)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}
