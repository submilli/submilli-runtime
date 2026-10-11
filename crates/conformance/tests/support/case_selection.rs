//! Exact, reproducible subsets for migrations spanning unrelated directories.
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub fn read(variable: &str) -> Result<Option<BTreeSet<String>>, String> {
    let Ok(file) = std::env::var(variable) else {
        return Ok(None);
    };
    let text =
        std::fs::read_to_string(&file).map_err(|e| format!("read {variable}={file}: {e}"))?;
    parse(&text).map(Some)
}

fn parse(text: &str) -> Result<BTreeSet<String>, String> {
    let mut selected = BTreeSet::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.ends_with(".ts")
            || line.ends_with(".d.ts")
            || line.contains('\\')
            || line
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || Path::new(line)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(format!("invalid case path: {line}"));
        }
        selected.insert(line.to_string());
    }
    if selected.is_empty() {
        return Err("case list is empty".to_string());
    }
    Ok(selected)
}

pub fn retain(
    paths: &mut Vec<PathBuf>,
    root: &Path,
    selected: &BTreeSet<String>,
) -> Result<(), String> {
    let present: BTreeSet<_> = paths
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let missing: Vec<_> = selected.difference(&present).collect();
    if !missing.is_empty() {
        return Err(format!("case list names missing cases: {missing:?}"));
    }
    paths.retain(|path| {
        path.strip_prefix(root)
            .ok()
            .is_some_and(|path| selected.contains(path.to_string_lossy().as_ref()))
    });
    Ok(())
}

#[test]
fn lists_are_exact_and_reject_missing_or_escaping_paths() {
    let selected = parse("# migration\na/b.ts\na/b.ts\n").unwrap();
    assert_eq!(selected.len(), 1);
    for invalid in [
        "",
        "../a.ts",
        "/a.ts",
        "a/../b.ts",
        "a//b.ts",
        "a\\b.ts",
        "a.d.ts",
    ] {
        assert!(parse(invalid).is_err(), "{invalid}");
    }
    let root = Path::new("cases");
    let mut cases = vec![root.join("a/b.ts"), root.join("a/b.ts.extra.ts")];
    retain(&mut cases, root, &selected).unwrap();
    assert_eq!(cases, [root.join("a/b.ts")]);
    assert!(retain(&mut cases, root, &parse("missing.ts").unwrap()).is_err());
}
