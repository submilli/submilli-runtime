//! `submilli playground init`: a `submilli/` folder with a starter billing package,
//! a deny-by-default blueprint that pins charges to the signed-in customer, and an
//! example program. Nothing is left outside `submilli/`: the files are written into
//! a hidden staging folder beside it and renamed into place, so a failed `init`
//! leaves no partial `submilli/` that would make the next one refuse, and a staging
//! folder an interrupted `init` left behind is removed by the next one that succeeds.
//!
//! The starter package is the quickstart's billing package, except that its charges
//! come from a fixture file on the project's `billing` volume (`submilli/volumes/`)
//! rather than a constant: reading it is a recorded file read, so a run's data
//! summary counts it and a replay serves it from the recording.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::log::warn;

/// The folder `init` creates, beside the project's own files.
pub(crate) const FOLDER: &str = "submilli";
/// The starter's blueprint, its package, and the binding its example runs under.
pub(crate) const BLUEPRINT_NAME: &str = "billing";
pub(crate) const PACKAGE_NAME: &str = "@acme/billing";
pub(crate) const EXAMPLE: &str = "examples/total.ts";

/// The starter files, relative to `submilli/`.
const FILES: &[(&str, &str)] = &[
    ("submilli.toml", MANIFEST),
    ("packages/billing/src/lib.ts", LIB),
    ("packages/billing/docs/readme.md", README),
    ("packages/billing/tests/lib.test.ts", TEST),
    ("volumes/billing/charges.json", FIXTURE),
    ("blueprints/billing.yaml", BLUEPRINT),
    (EXAMPLE, EXAMPLE_PROGRAM),
];

/// What `init` wrote.
pub(crate) struct Scaffolded {
    /// The project root: the directory that now holds `submilli/`.
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<PathBuf>,
}

/// The directory `init` scaffolds in when run from `from`: the root of the enclosing
/// git repository, so the project is found from anywhere in it, or `from` itself.
pub(crate) fn root_for(from: &Path) -> PathBuf {
    from.ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(from)
        .to_path_buf()
}

/// Write the starter under `<root>/submilli/`. Refuses when `submilli/` already
/// exists, so nothing of the developer's is overwritten.
pub(crate) fn init(root: &Path) -> Result<Scaffolded> {
    init_with(root, FILES)
}

/// The prefix of the hidden folder `init` stages the starter in, which is followed
/// by [`STAGING_RANDOM_LEN`] random ASCII letters and digits.
const STAGING_PREFIX: &str = ".submilli-init-";
const STAGING_RANDOM_LEN: usize = 6;
/// The file `init` writes first into its staging folder and removes just before the
/// rename, so a leftover is recognized as one `init` made, not a folder of the
/// developer's that happens to share the name's shape.
const STAGING_MARKER: &str = ".submilli-init-staging";

fn init_with(root: &Path, starter: &[(&str, &str)]) -> Result<Scaffolded> {
    let folder = root.join(FOLDER);
    if folder.exists() {
        return Err(already_exists(&folder));
    }
    // Removed when dropped, so a write that fails takes the staged files with it.
    // Created with the mode a plain directory gets (0777 less the umask), not the
    // owner-only mode of a temporary directory: it becomes the developer's
    // `submilli/`.
    let mut staging = tempfile::Builder::new();
    staging
        .prefix(STAGING_PREFIX)
        .rand_bytes(STAGING_RANDOM_LEN);
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o777)
    };
    #[cfg(unix)]
    staging.permissions(permissions);
    let staging = staging
        .tempdir_in(root)
        .with_context(|| format!("creating a staging folder in {}", root.display()))?;
    let marker = staging.path().join(STAGING_MARKER);
    let moved = std::fs::write(&marker, "")
        .with_context(|| format!("creating {}", marker.display()))
        .and_then(|()| write_starter(staging.path(), starter))
        .and_then(|()| {
            std::fs::remove_file(&marker).with_context(|| format!("removing {}", marker.display()))
        })
        .and_then(|()| {
            if folder.exists() {
                bail!("another init created it");
            }
            std::fs::rename(staging.path(), &folder)
                .with_context(|| format!("moving the starter into {}", folder.display()))
        });
    if let Err(error) = moved {
        // A concurrent init that finished first, maybe removing this one's staging
        // folder as a leftover, is the reason, whatever step failed.
        if folder.exists() {
            return Err(already_exists(&folder));
        }
        return Err(error);
    }
    // The staging folder is now `submilli/`; there is nothing left to remove.
    let _ = staging.keep();
    remove_leftover_staging(root);
    Ok(Scaffolded {
        root: root.to_path_buf(),
        files: starter
            .iter()
            .map(|(relative, _)| folder.join(relative))
            .collect(),
    })
}

fn already_exists(folder: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "{} already exists; `playground init` only creates a new `submilli/` folder",
        folder.display()
    )
}

fn write_starter(staging: &Path, starter: &[(&str, &str)]) -> Result<()> {
    for (relative, text) in starter {
        let path = staging.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        file.write_all(text.as_bytes())
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// Removes the staging folders an interrupted `init` left in `root`: only a real
/// folder (not a link) whose name has the exact shape `init` gives one and that holds
/// [`STAGING_MARKER`]. Runs once this init's `submilli/` is in place. An init still
/// staging beside it then loses its folder or fails its rename, and either way
/// reports that `submilli/` already exists.
fn remove_leftover_staging(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let staged = entry.file_name().to_str().is_some_and(is_staging_name);
        if !staged || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        if !path.join(STAGING_MARKER).is_file() {
            continue;
        }
        // Not found, or not empty because another init is still writing into it:
        // that init removes its own when it finds `submilli/`.
        if let Err(error) = std::fs::remove_dir_all(&path)
            && !matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            )
        {
            warn(&format!(
                "removing {}, left by an interrupted init: {error}",
                path.display()
            ));
        }
    }
}

/// Whether `name` is one `init` gives its staging folder.
fn is_staging_name(name: &str) -> bool {
    name.strip_prefix(STAGING_PREFIX).is_some_and(|random| {
        random.len() == STAGING_RANDOM_LEN
            && random.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

const MANIFEST: &str = r#"[[package]]
name = "@acme/billing"
version = "0.1.0"
description = "Starter billing package: a charge lookup keyed by customer, read from a fixture file."
keywords = ["starter", "example", "billing", "charges"]
path = "packages/billing"
"#;

const LIB: &str = r#"// A charge lookup for a customer, standing in for a real billing API. The charges
// come from a fixture file on the project's `billing` volume, so reading them is a
// call the playground records, as a request to a billing service would be.

import { readText } from "submilli:fs";
import { check } from "submilli:security";

/** Where the blueprint's `vfs:` block mounts the fixture. */
const LEDGER_PATH = "/billing/charges.json";

/** One charge on a customer's account. */
export interface Charge {
    /** The customer the charge belongs to. */
    customerId: string;
    /** Charge identifier, as the billing system issued it. */
    id: string;
    /** Amount in cents. */
    amount: number;
}

/**
 * List the charges on one customer's account.
 * @param customerId Billing customer ID, such as `cus_northwind`.
 * @returns The customer's charges; empty when they have none.
 * @capability acme.com/charges.list { customerId: string }
 */
export function listCharges(customerId: string): Charge[] {
    check("acme.com/charges.list", { customerId });

    // In production, this would call the billing service.
    const text = readText(LEDGER_PATH);
    if (text === null) {
        throw new Error(`${LEDGER_PATH} is too large to read`);
    }
    const ledger = JSON.parse(text) as Charge[];
    const found: Charge[] = [];
    for (const charge of ledger) {
        if (charge.customerId === customerId) {
            found.push(charge);
        }
    }
    return found;
}
"#;

const README: &str = r#"# @acme/billing

The playground's starter package. `listCharges(customerId)` returns the charges on one
customer's account, read from `charges.json` on the project's `billing` volume
(`submilli/volumes/billing/`). There is no network call and no configuration.

The operation declares the capability `acme.com/charges.list` with a `customerId` field, so a
blueprint can grant it under a filter that pins the customer to a value the calling application
binds per request.
"#;

const TEST: &str = r#"import { mkdir, writeText } from "submilli:fs";
import { label } from "submilli:test";
import { listCharges } from "@acme/billing";

function main(): void {
    // The playground mounts the project's `billing` volume here; a test writes its own.
    mkdir("/billing", true);
    writeText(
        "/billing/charges.json",
        '[{"customerId":"cus_northwind","id":"ch_a1","amount":4900},' +
            '{"customerId":"cus_northwind","id":"ch_a2","amount":1250},' +
            '{"customerId":"cus_initech","id":"ch_a3","amount":39900}]',
    );

    label("lists a customer's own charges");
    const charges = listCharges("cus_northwind");
    assert(charges.length === 2, "cus_northwind has two charges in the fixture");
    assert(charges[0].amount === 4900, "the first is the 4900-cent charge");

    label("scopes the lookup to the customer asked for");
    assert(listCharges("cus_initech").length === 1, "cus_initech has one charge");
    assert(listCharges("cus_unknown").length === 0, "an unknown customer has none");
}
"#;

const FIXTURE: &str = r#"[
  { "customerId": "cus_northwind", "id": "ch_a1", "amount": 4900 },
  { "customerId": "cus_northwind", "id": "ch_a2", "amount": 1250 },
  { "customerId": "cus_initech", "id": "ch_a3", "amount": 39900 }
]
"#;

const BLUEPRINT: &str = r#"kind: blueprint
name: billing

# Bound once per request by the application, never by the program.
variables:
  customerId:
    required: true

packages:
- '@acme/billing'

# The billing fixture, read-only, from the project's `billing` volume
# (submilli/volumes/billing/).
vfs:
  mode: ephemeral
  mounts:
    /billing:
      mode: named
      volume: billing
      access: read_only

default: deny

permissions:
  # What generated code may do.
  main:
  - name: charges-for-signed-in-customer
    capability: acme.com/charges.list
    filter: customerId == ${vars.customerId}
    action: allow

  # What the package itself may do: read its fixture.
  '@acme/billing':
  - capability: fs.read
    filter: path == "/billing/charges.json"
    action: allow
"#;

const EXAMPLE_PROGRAM: &str = r#"// Total up the signed-in customer's charges. Run it with `customerId` bound to
// `cus_northwind`; asking for another customer's charges is denied.
import { listCharges } from "@acme/billing";

function main(): string {
    const charges = listCharges("cus_northwind");
    let total = 0;
    for (const charge of charges) {
        total += charge.amount;
    }
    return `${charges.length} charges, ${total} cents`;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_writes_only_under_submilli_and_refuses_an_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')\n").unwrap();
        let scaffolded = init(dir.path()).unwrap();
        assert_eq!(scaffolded.files.len(), FILES.len());
        for file in &scaffolded.files {
            assert!(
                file.starts_with(dir.path().join(FOLDER)),
                "{}",
                file.display()
            );
        }
        let mut top: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        top.sort();
        assert_eq!(top, ["app.py", "submilli"]);
        let error = init(dir.path()).err().unwrap();
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn a_failed_init_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.py"), "print('hi')\n").unwrap();
        // The second file cannot be created: the first already holds its path.
        let error = init_with(dir.path(), &[("a.txt", "one"), ("a.txt", "two")])
            .err()
            .expect("the duplicate write fails");
        assert!(error.to_string().contains("a.txt"), "{error:#}");
        let top: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(top, ["app.py"]);
        // So the next init is not refused.
        init(dir.path()).unwrap();
    }

    #[test]
    fn a_staging_folder_left_by_an_interrupted_init_is_removed_by_the_next() {
        let dir = tempfile::tempdir().unwrap();
        // An init killed before its rename leaves its staging folder behind.
        let leftover = dir.path().join(".submilli-init-abc123");
        std::fs::create_dir_all(leftover.join("packages")).unwrap();
        std::fs::write(leftover.join(STAGING_MARKER), "").unwrap();
        std::fs::write(leftover.join("submilli.toml"), "partial").unwrap();
        // The developer's own folders that only share the prefix, or the shape but
        // not the marker, stay.
        let notes = dir.path().join(".submilli-init-notes");
        std::fs::create_dir(&notes).unwrap();
        std::fs::write(notes.join("todo.txt"), "mine").unwrap();
        let unmarked = dir.path().join(".submilli-init-xyz789");
        std::fs::create_dir(&unmarked).unwrap();
        std::fs::write(unmarked.join("todo.txt"), "mine").unwrap();
        // A link with a staging name, even to a marked folder, stays, and so does
        // what it points to.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join(STAGING_MARKER), "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(other.path(), dir.path().join(".submilli-init-link12")).unwrap();
        let scaffolded = init(dir.path()).unwrap();
        assert_eq!(scaffolded.files.len(), FILES.len());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FOLDER).join("submilli.toml")).unwrap(),
            MANIFEST
        );
        assert!(!leftover.exists());
        assert!(notes.join("todo.txt").exists());
        assert!(unmarked.join("todo.txt").exists());
        assert!(
            !dir.path().join(FOLDER).join(STAGING_MARKER).exists(),
            "the marker does not end up in submilli/"
        );
        #[cfg(unix)]
        assert!(other.path().join(STAGING_MARKER).exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_gets_the_mode_a_plain_directory_gets() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        init(dir.path()).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.path().join(FOLDER)), mode(&plain));
    }

    #[test]
    fn inits_racing_in_one_folder_leave_one_starter_and_say_the_rest_lost() {
        let dir = tempfile::tempdir().unwrap();
        let barrier = std::sync::Barrier::new(8);
        let results: Vec<Result<Scaffolded>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        init(dir.path())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        for error in results.iter().filter_map(|r| r.as_ref().err()) {
            assert!(error.to_string().contains("already exists"), "{error:#}");
        }
        let top: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(top, [FOLDER], "no staging folder is left");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FOLDER).join("submilli.toml")).unwrap(),
            MANIFEST
        );
    }

    #[test]
    fn the_starter_blueprint_parses_and_pins_the_customer() {
        let blueprint = submilli_blueprint::parse(BLUEPRINT).unwrap();
        assert_eq!(blueprint.name, BLUEPRINT_NAME);
        assert_eq!(
            blueprint.packages.iter().collect::<Vec<_>>(),
            [PACKAGE_NAME]
        );
        assert!(BLUEPRINT.contains("default: deny"));
        assert!(BLUEPRINT.contains("filter: customerId == ${vars.customerId}"));
    }

    #[test]
    fn init_goes_to_the_repository_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        assert_eq!(root_for(&dir.path().join("src/deep")), dir.path());
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(root_for(bare.path()), bare.path());
    }
}
