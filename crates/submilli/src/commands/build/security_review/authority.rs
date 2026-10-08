//! Bounded compiler evidence from captured files, never the live project/store.
use std::collections::BTreeMap;
use std::io::{self, Write};

use anyhow::{Context, bail};
use interpreter::{AuthorityCallable, AuthorityEdge, AuthorityExposure, AuthorityMap};
use serde::Serialize;
use sha2::{Digest, Sha256};
use submilli_build::{
    CapabilitySchema, PackageName, PackageStore, ProjectManifest, build_packages,
};

use super::snapshot::{Snapshot, Source};

const MAX_MAP_BYTES: usize = 2 * 1024 * 1024;

#[derive(Serialize)]
pub(super) struct Evidence {
    pub schema_version: u32,
    pub sha256: String,
    pub source_sha256: String,
    pub packages: Vec<PackageEvidence>,
    pub limitations: &'static [&'static str],
}

#[derive(Serialize)]
pub(super) struct PackageEvidence {
    name: String,
    capabilities: CapabilitySchema,
    callables: Vec<AuthorityCallable>,
    edges: Vec<AuthorityEdge>,
    routes: Vec<String>,
}

const LIMITATIONS: &[&str] = &[
    "Potential call paths are syntactic, not proofs of feasible execution or authorization.",
    "Check sites are invocations only; success, dominance and selector correspondence require source review.",
    "Unresolved targets, callbacks, returned handles, aliases and implicit dispatch require source review.",
    "Cross-export state, denial confinement, sensitive-result egress and remote-service semantics require source review.",
    "Absence of an effect or route is not a safety claim; inspect all supplied source and public surfaces.",
];

pub(super) fn collect(
    snapshot: &Snapshot,
    manifest: &ProjectManifest,
    only: Option<&str>,
) -> anyhow::Result<Evidence> {
    let workspace = tempfile::tempdir().context("create captured compiler workspace")?;
    let root = workspace.path();
    for (name, source) in &snapshot.files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().context("snapshot file has no parent")?)?;
        std::fs::write(path, &source.content)?;
    }
    // Reuse the manifest parsed from captured text. Re-parsing here would
    // require entrypoint files for unselected packages that are not reviewed.
    let only = only.map(PackageName::new);
    // An empty private store prevents ambient installed artifacts from affecting
    // the evidence. Missing external sources are rejected by the caller.
    let store = PackageStore::new(root.join(".review-empty-store"));
    let built = build_packages(manifest, root, &store, only.as_ref())
        .context("compile captured review snapshot")?;
    let mut packages = Vec::new();
    for package in built {
        let definition = manifest
            .packages
            .iter()
            .find(|p| p.name == package.name)
            .context("compiled package absent from captured manifest")?;
        let mut map = package.authority_map;
        qualify_paths(&mut map, definition.path.as_path(), &snapshot.files)?;
        let routes = map
            .callables
            .iter()
            .filter(|c| c.exposure != AuthorityExposure::Private)
            .map(|c| c.id.clone())
            .collect();
        // Edges are the shared witness graph. Repeating transitive effects and
        // one expanded witness per route/sink swamps the review context.
        for callable in &mut map.callables {
            callable.transitive_effects.clear();
        }
        packages.push(PackageEvidence {
            name: package.name.as_str().to_owned(),
            capabilities: package.capabilities,
            callables: map.callables,
            edges: map.edges,
            routes,
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let source_sha256 = snapshot.source_hash()?;
    let mut digest = LimitedDigest::default();
    serde_json::to_writer(&mut digest, &(2u32, &source_sha256, &packages, LIMITATIONS))
        .context("authority evidence exceeds 2 MiB; select a smaller package with -p")?;
    Ok(Evidence {
        schema_version: 2,
        sha256: format!("{:x}", digest.hash.finalize()),
        source_sha256,
        packages,
        limitations: LIMITATIONS,
    })
}

fn qualify_paths(
    map: &mut AuthorityMap,
    package: &std::path::Path,
    files: &BTreeMap<String, Source>,
) -> anyhow::Result<()> {
    let source_root: std::path::PathBuf = package
        .join("src")
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .collect();
    let mut module_paths = BTreeMap::new();
    for path in files.keys() {
        let Ok(relative) = std::path::Path::new(path).strip_prefix(&source_root) else {
            continue;
        };
        if !matches!(
            relative.extension().and_then(|s| s.to_str()),
            Some("ts" | "subm")
        ) {
            continue;
        }
        // Match the compiler's module normalization, while retaining the exact
        // captured filename for citations (including literal backslashes).
        let module = relative
            .with_extension("")
            .to_str()
            .context("non-UTF8 compiler source path")?
            .replace('\\', "/");
        if module_paths.insert(module, path).is_some() {
            bail!("ambiguous authority source path");
        }
    }
    let qualify = |span: &mut interpreter::AuthoritySpan| -> anyhow::Result<()> {
        span.path = module_paths
            .get(&span.path)
            .context("authority span is outside captured source")?
            .to_string();
        Ok(())
    };
    for callable in &mut map.callables {
        qualify(&mut callable.span)?;
        for check in &mut callable.checks {
            qualify(&mut check.span)?;
        }
        for effect in &mut callable.direct_effects {
            qualify(&mut effect.sink)?;
        }
    }
    for edge in &mut map.edges {
        qualify(&mut edge.span)?;
    }
    Ok(())
}

#[derive(Default)]
struct LimitedDigest {
    bytes: usize,
    hash: Sha256,
}
impl Write for LimitedDigest {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_MAP_BYTES)
            .ok_or_else(|| io::Error::other("authority map size limit"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_serialization_refuses_overflow_without_truncating() {
        let mut digest = LimitedDigest::default();
        digest.write_all(&vec![b'x'; MAX_MAP_BYTES]).unwrap();
        assert!(digest.write_all(b"x").is_err());
    }
}
