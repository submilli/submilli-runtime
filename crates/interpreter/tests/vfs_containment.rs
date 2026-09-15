//! Containment regression suite: a symlink inside a VFS root must not reach outside it.
//!
//! Not a `.ts` fixture. The fixture harness hands every fixture a fresh empty temp root
//! and no `submilli:fs` export can originate a symlink, so the links these cases need
//! cannot be planted from inside a fixture. Each test seeds a root by hand and drives the
//! interpreter directly.
//!
//! The layout every test shares:
//!
//! ```text
//! base/
//!   outside/secret.txt          "OUTSIDE-SECRET"   — the thing containment must keep unreachable
//!   root/                                          — the VFS root
//!     inside.txt                "INSIDE"
//!     dir/a.txt                 "NESTED"
//!     inner        -> ./dir                        relative, stays inside
//!     pkgs/a/sibling -> ../b                       relative up 1, stays inside
//!     pkgs/a/dep     -> ../../store/x              relative up 2, stays inside
//!     pkgs/a/abs     -> <abs>/pkgs/b               absolute, resolves inside — still refused
//!     pkgs/b/lib.txt            "SIBLING"
//!     store/x/dep.txt           "STORE-DEP"
//!     esc_abs      -> /                            absolute, outside
//!     esc_rel      -> ../outside                   relative, outside
//!     deep/dir/link -> ../../../outside            relative, outside, from depth
//! ```

use std::path::{Path, PathBuf};

use interpreter::runtime::{
    RuntimeConfig, StoreData, Vfs, install_runtime_async, install_tenant_limits,
};
use interpreter::{FileId, compile_script, dispatch_main_async};
use wasmtime::{Linker, Module};

const ESCAPE_DIAGNOSTIC: &str = "path escapes the VFS root";

struct Seeded {
    base: tempfile::TempDir,
}

impl Seeded {
    fn root(&self) -> PathBuf {
        self.base.path().join("root")
    }

    fn outside(&self) -> PathBuf {
        self.base.path().join("outside")
    }
}

/// Create a symlink at `link` pointing at `target`, portably.
///
/// Not gated to Unix, and deliberately not skipped when it fails: `cap-std` resolves
/// paths differently on each platform — `openat2` with `RESOLVE_BENEATH` on Linux, a
/// manual resolver on macOS, a lexical `..` pass on Windows — so a suite that quietly
/// compiled to nothing on the platform with the least-exercised path would be theatre.
/// A failure here is a real signal, including a Windows runner without symlink
/// privilege.
fn link(target: &Path, at: &Path, target_is_dir: bool) {
    #[cfg(unix)]
    {
        let _ = target_is_dir;
        std::os::unix::fs::symlink(target, at).expect("symlink");
    }
    #[cfg(windows)]
    {
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, at).expect("symlink_dir");
        } else {
            std::os::windows::fs::symlink_file(target, at).expect("symlink_file");
        }
    }
}

/// An absolute path outside any temp root: the filesystem root on Unix, the drive root
/// on Windows. This is the `link -> /` case from the acceptance examples.
fn filesystem_root(inside: &Path) -> PathBuf {
    inside
        .ancestors()
        .last()
        .expect("every path has a root ancestor")
        .to_path_buf()
}

fn seed() -> Seeded {
    let base = tempfile::tempdir().expect("tempdir");
    let root = base.path().join("root");
    let outside = base.path().join("outside");

    std::fs::create_dir_all(outside.join("sub")).expect("mkdir outside");
    std::fs::write(outside.join("secret.txt"), b"OUTSIDE-SECRET").expect("seed secret");

    std::fs::create_dir_all(root.join("dir")).expect("mkdir dir");
    std::fs::create_dir_all(root.join("pkgs/a")).expect("mkdir pkgs/a");
    std::fs::create_dir_all(root.join("pkgs/b")).expect("mkdir pkgs/b");
    std::fs::create_dir_all(root.join("store/x")).expect("mkdir store/x");
    std::fs::create_dir_all(root.join("deep/dir")).expect("mkdir deep/dir");
    std::fs::write(root.join("inside.txt"), b"INSIDE").expect("seed inside");
    std::fs::write(root.join("dir/a.txt"), b"NESTED").expect("seed nested");
    std::fs::write(root.join("pkgs/b/lib.txt"), b"SIBLING").expect("seed sibling");
    std::fs::write(root.join("store/x/dep.txt"), b"STORE-DEP").expect("seed store");

    let abs_root = std::fs::canonicalize(&root).expect("canonicalize root");
    link(Path::new("./dir"), &root.join("inner"), true);
    link(Path::new("../b"), &root.join("pkgs/a/sibling"), true);
    link(Path::new("../../store/x"), &root.join("pkgs/a/dep"), true);
    link(&abs_root.join("pkgs/b"), &root.join("pkgs/a/abs"), true);
    link(&filesystem_root(base.path()), &root.join("esc_abs"), true);
    link(Path::new("../outside"), &root.join("esc_rel"), true);
    link(
        Path::new("../../../outside"),
        &root.join("deep/dir/link"),
        true,
    );

    Seeded { base }
}

/// Compile and run `src` against a VFS mounted on `root`. Returns the program's output,
/// or the trap the runtime raised.
fn run_on(root: &Path, src: &str) -> wasmtime::Result<Option<String>> {
    let compiled = compile_script(src, "containment.ts", FileId(0), &[], &[])
        .unwrap_or_else(|d| panic!("test program must compile: {d:?}"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let vfs = Vfs::external(root.to_path_buf()).expect("external vfs");
    let mut store = cfg.store(&engine, StoreData::with_vfs(vfs)).expect("store");
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store).await?;
        let instance = linker.instantiate_async(&mut store, &module).await?;
        dispatch_main_async(&mut store, &instance).await
    })
}

/// Assert the program refused with the containment diagnostic rather than any other error.
///
/// The message matters as much as the refusal: an escape reported as "no such file" costs
/// an LLM a turn on a `mkdir` that can never succeed.
fn assert_refused(seeded: &Seeded, src: &str, what: &str) {
    let err = run_on(&seeded.root(), src).expect_err(&format!("{what} must be refused"));
    let msg = format!("{err:#}");
    assert!(
        msg.contains(ESCAPE_DIAGNOSTIC),
        "{what}: expected the containment diagnostic, got: {msg}",
    );
}

fn assert_ok(seeded: &Seeded, src: &str, what: &str) {
    if let Err(err) = run_on(&seeded.root(), src) {
        panic!("{what} must succeed, got: {err:#}");
    }
}

// ---------------------------------------------------------------------------
// AE8 — reads through an escaping link
// ---------------------------------------------------------------------------

#[test]
fn read_through_absolute_escaping_link_refuses() {
    let s = seed();
    assert_refused(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void { readText("/esc_abs/inside.txt"); }"#,
        "readText through a link to the filesystem root",
    );
}

#[test]
fn read_through_relative_escaping_link_refuses_and_leaks_nothing() {
    let s = seed();
    let src = r#"import { readText } from "submilli:fs";
                 function main(): string { const t = readText("/esc_rel/secret.txt"); return t === null ? "null" : t; }"#;
    let err = run_on(&s.root(), src).expect_err("must be refused");
    let msg = format!("{err:#}");
    assert!(msg.contains(ESCAPE_DIAGNOSTIC), "got: {msg}");
    assert!(
        !msg.contains("OUTSIDE-SECRET"),
        "the diagnostic must not carry the file it refused to read: {msg}",
    );
}

#[test]
fn every_content_read_refuses_through_an_escaping_link() {
    let s = seed();
    for (op, src) in [
        (
            "readBytes",
            r#"import { readBytes } from "submilli:fs";
                        function main(): void { readBytes("/esc_rel/secret.txt", 0, 4); }"#,
        ),
        (
            "size",
            r#"import { size } from "submilli:fs";
                    function main(): void { size("/esc_rel/secret.txt"); }"#,
        ),
        (
            "peek",
            r#"import { peek } from "submilli:fs";
                    function main(): void { peek("/esc_rel/secret.txt"); }"#,
        ),
        (
            "lines",
            r#"import { lines } from "submilli:fs";
                     function main(): void { for (const l of lines("/esc_rel/secret.txt")) { } }"#,
        ),
        (
            "bytes",
            r#"import { bytes } from "submilli:fs";
                     function main(): void { for (const b of bytes("/esc_rel/secret.txt", 16)) { } }"#,
        ),
    ] {
        assert_refused(&s, src, op);
    }
}

#[test]
fn exists_reports_an_unreachable_path_as_absent_rather_than_trapping() {
    let s = seed();
    let out = run_on(
        &s.root(),
        r#"import { exists } from "submilli:fs";
           function main(): boolean { return exists("/esc_rel/secret.txt"); }"#,
    )
    .expect("exists must not trap on an escape");
    assert_eq!(
        out.as_deref(),
        Some("false"),
        "an escape is not an existence oracle"
    );
}

// ---------------------------------------------------------------------------
// AE9 — writes, mkdir, list, writer through an escaping link
// ---------------------------------------------------------------------------

#[test]
fn writes_through_an_escaping_link_refuse_and_write_nothing_outside() {
    let s = seed();
    for (op, src) in [
        (
            "writeText",
            r#"import { writeText } from "submilli:fs";
                         function main(): void { writeText("/esc_rel/pwned.txt", "x"); }"#,
        ),
        (
            "appendText",
            r#"import { appendText } from "submilli:fs";
                          function main(): void { appendText("/esc_rel/pwned.txt", "x"); }"#,
        ),
        (
            "mkdir",
            r#"import { mkdir } from "submilli:fs";
                     function main(): void { mkdir("/esc_rel/pwned", false); }"#,
        ),
        (
            "list",
            r#"import { list } from "submilli:fs";
                    function main(): void { for (const e of list("/esc_rel", false)) { } }"#,
        ),
        (
            "writer",
            r#"import { writer, FileWriter } from "submilli:fs";
                      function main(): void { const w: FileWriter = writer("/esc_rel/pwned.txt"); w.close(); }"#,
        ),
    ] {
        assert_refused(&s, src, op);
    }
    assert!(
        !s.outside().join("pwned.txt").exists() && !s.outside().join("pwned").exists(),
        "nothing may be created outside the root",
    );
}

#[test]
fn write_to_a_missing_parent_reports_not_found_not_an_escape() {
    let s = seed();
    let err = run_on(
        &s.root(),
        r#"import { writeText } from "submilli:fs";
           function main(): void { writeText("/no/such/dir/f.txt", "x"); }"#,
    )
    .expect_err("must fail");
    let msg = format!("{err:#}");
    assert!(
        !msg.contains(ESCAPE_DIAGNOSTIC),
        "a genuinely missing parent is not an escape: {msg}",
    );
    // Asserting only the absence of the escape string would pass on any failure at all.
    // The message an LLM has to act on is the one that names the fix.
    assert!(
        msg.contains("parent directory does not exist"),
        "the diagnostic must name the fix, not just fail: {msg}",
    );
}

// ---------------------------------------------------------------------------
// AE11, AE12 — which links stay traversable
// ---------------------------------------------------------------------------

#[test]
fn relative_links_that_stay_inside_the_root_still_traverse() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void {
             assert(readText("/inner/a.txt") === "NESTED", "link into a subdirectory");
             assert(readText("/pkgs/a/sibling/lib.txt") === "SIBLING", "monorepo sibling, up one");
             assert(readText("/pkgs/a/dep/dep.txt") === "STORE-DEP", "pnpm-style store, up two");
           }"#,
        "upward traversal that stays inside the root",
    );
}

#[test]
fn an_absolute_link_is_refused_even_when_it_resolves_inside_the_root() {
    let s = seed();
    assert_refused(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void { readText("/pkgs/a/abs/lib.txt"); }"#,
        "absolute link resolving inside the root",
    );
}

/// A copied internal directory link must still traverse as a directory.
///
/// Vacuous on Unix, where there is one kind of symlink — which is the reason the
/// containment job runs this suite on Windows. There a symlink is created as either a
/// file link or a directory link and the two are not interchangeable, so a directory
/// link rebuilt as a file link is a broken copy. `../b` is the shape that exposes it:
/// deciding the flavour by statting the target through the destination's directory
/// handle refuses a target that reaches above that handle, however ordinary the link.
#[test]
fn copying_an_internal_directory_link_keeps_it_a_directory_link() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { copy, readText } from "submilli:fs";
           function main(): void {
             copy("/pkgs/a/sibling", "/pkgs/a/sibling_copy", false);
             assert(readText("/pkgs/a/sibling_copy/lib.txt") === "SIBLING", "copied link traverses");
           }"#,
        "copying `../b`, a directory link one level up",
    );
}

// ---------------------------------------------------------------------------
// AE13, AE14 — copy and move must not manufacture a traversable escape
// ---------------------------------------------------------------------------

#[test]
fn copying_an_escaping_link_leaves_no_traversable_escape() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { copy, readText } from "submilli:fs";
           function main(): void { copy("/esc_rel", "/copied", false); }"#,
        "copying a link",
    );
    let dest = s.root().join("copied");
    let meta = dest
        .symlink_metadata()
        .expect("copy must reproduce the link at the destination");
    assert!(
        meta.file_type().is_symlink(),
        "copy must reproduce a link as a link, never dereference it into a regular file",
    );
    assert_eq!(
        std::fs::read_link(&dest).expect("read_link"),
        std::fs::read_link(s.root().join("esc_rel")).expect("read_link source"),
        "the reproduced link must carry the source's target verbatim",
    );
    assert_refused(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void { readText("/copied/secret.txt"); }"#,
        "reading through the copied link",
    );
}

#[test]
fn moving_a_link_relocates_the_link_and_it_stays_untraversable() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { move } from "submilli:fs";
           function main(): void { move("/deep/dir/link", "/link"); }"#,
        "moving a link to a shallower directory",
    );
    let moved = s.root().join("link");
    assert!(
        moved
            .symlink_metadata()
            .expect("moved entry")
            .file_type()
            .is_symlink(),
        "move must relocate the link itself, not its target",
    );
    assert_refused(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void { readText("/link/secret.txt"); }"#,
        "reading through the moved link",
    );
}

// ---------------------------------------------------------------------------
// AE15, AE16 — the lexical rule keeps its existing behavior
// ---------------------------------------------------------------------------

#[test]
fn lexical_parent_escape_keeps_its_existing_error() {
    let s = seed();
    assert_refused(
        &s,
        r#"import { readText } from "submilli:fs";
           function main(): void { readText("../outside/secret.txt"); }"#,
        "lexical `..` escape",
    );
}

// ---------------------------------------------------------------------------
// AE19 — link-acting operations stay usable on a stale escaping link
// ---------------------------------------------------------------------------

#[test]
fn stat_and_remove_work_on_an_escaping_link_without_traversing_it() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { stat, Stat } from "submilli:fs";
           function main(): void {
             const a: Stat | null = stat("/esc_abs");
             assert(a !== null && a.kind === "symlink", "stat reports the link, not its target");
             const b: Stat | null = stat("/pkgs/a/sibling");
             assert(b !== null && b.kind === "symlink", "an ordinary internal link still stats");
           }"#,
        "stat on links",
    );
    assert_ok(
        &s,
        r#"import { remove, exists } from "submilli:fs";
           function main(): void { remove("/esc_rel", false); }"#,
        "remove of an escaping link",
    );
    assert!(
        s.root().join("esc_rel").symlink_metadata().is_err(),
        "the link itself must be gone",
    );
    assert!(
        s.outside().join("secret.txt").exists(),
        "removing the link must not touch what it pointed at",
    );
}

// ---------------------------------------------------------------------------
// AE20 — deferred commits resolve through the handle they were opened against
// ---------------------------------------------------------------------------

#[test]
fn a_writer_whose_parent_is_swapped_for_an_escaping_link_refuses_at_close() {
    let s = seed();
    // The swap must leave the writer's temp file intact — deleting the directory would
    // fail the close with a plain ENOENT before containment is ever consulted, which is
    // indistinguishable from the pre-fix behaviour. Moving it aside keeps the rename's
    // source path traversing the escaping link that took its place.
    let src = r#"import { writer, mkdir, move, FileWriter } from "submilli:fs";
                 function main(): void {
                   mkdir("/a/b", true);
                   const w: FileWriter = writer("/a/b/out.txt");
                   w.writeLine("payload");
                   move("/a/b", "/a/b_real");
                   move("/esc_abs", "/a/b");
                   w.close();
                 }"#;
    let err = run_on(&s.root(), src).expect_err("the commit must refuse");
    let msg = format!("{err:#}");
    assert!(
        msg.contains(ESCAPE_DIAGNOSTIC),
        "the commit must refuse as an escape, not merely fail: {msg}",
    );
    assert!(
        !s.outside().join("out.txt").exists(),
        "nothing may be committed outside the root",
    );
}

#[test]
fn a_dropped_writer_removes_only_its_in_root_temp_file() {
    let s = seed();
    let outside_before: Vec<_> = std::fs::read_dir(s.outside())
        .expect("read outside")
        .map(|e| e.expect("entry").file_name())
        .collect();

    assert_ok(
        &s,
        r#"import { writer, FileWriter } from "submilli:fs";
           function main(): void { const w: FileWriter = writer("/dropped.txt"); w.writeLine("x"); }"#,
        "a writer that is never closed",
    );

    assert!(
        !s.root().join("dropped.txt").exists(),
        "no rename without an explicit close",
    );
    let stragglers: Vec<_> = std::fs::read_dir(s.root())
        .expect("read root")
        .map(|e| e.expect("entry").file_name())
        .filter(|n| n.to_string_lossy().starts_with("dropped.txt."))
        .collect();
    assert!(
        stragglers.is_empty(),
        "temp file must be cleaned up: {stragglers:?}"
    );

    let outside_after: Vec<_> = std::fs::read_dir(s.outside())
        .expect("read outside")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(
        outside_before, outside_after,
        "drop must not touch anything outside the root"
    );
}

// ---------------------------------------------------------------------------
// The flagship use case: a checkout-shaped tree walks and reads normally
// ---------------------------------------------------------------------------

#[test]
fn a_checkout_shaped_tree_lists_and_reads_normally() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { list, readText, DirEntry } from "submilli:fs";
           function main(): void {
             let files: number = 0;
             let links: number = 0;
             for (const e of list("/pkgs", true)) {
               if (e.kind === "file") { files = files + 1; }
               if (e.kind === "symlink") { links = links + 1; }
             }
             assert(files === 1, "the sibling package's file is listed");
             assert(links === 3, "links surface as entries without being descended");
             assert(readText("/inside.txt") === "INSIDE", "ordinary reads are unaffected");
           }"#,
        "recursive listing over a checkout-shaped tree",
    );
}

#[test]
fn a_recursive_listing_does_not_descend_through_an_escaping_link() {
    let s = seed();
    assert_ok(
        &s,
        r#"import { list, DirEntry } from "submilli:fs";
           function main(): void {
             for (const e of list("/", true)) {
               assert(e.path.indexOf("secret.txt") === -1, "the walk must not reach outside the root");
             }
           }"#,
        "recursive listing from the root",
    );
}
