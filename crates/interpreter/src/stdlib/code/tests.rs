use super::{patch, text};
#[test]
fn anchors_and_hints() {
    assert!(
        !text::replace("a\nx\na\n", "a", "b", false, 0, usize::MAX)
            .unwrap()
            .diagnostics
            .is_empty()
    );
    assert_eq!(
        text::replace("a\nx\na\n", "a", "b", false, 3, usize::MAX)
            .unwrap()
            .text,
        "a\nx\nb\n"
    );
    assert!(
        !text::replace("a\nx\na\n", "a", "b", false, 2, usize::MAX)
            .unwrap()
            .diagnostics
            .is_empty()
    );
    assert_eq!(
        text::replace("  a\n", "a\n", "b\n", false, 0, usize::MAX)
            .unwrap()
            .text,
        "  b\n"
    );
}
#[test]
fn diff_patch_roundtrips() {
    for (a, b) in [
        ("", "hi\n"),
        ("x\na\n", "b\nx\nx\n"),
        ("a\n", ""),
        ("a", "b"),
        ("a\r\nb\r\n", "a\r\nc\r\n"),
        ("\u{feff}a\n", "\u{feff}b\n"),
        (
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            "A\nb\nc\nd\ne\nf\ng\nh\ni\nJ\n",
        ),
    ] {
        let diff = text::diff(
            &a.encode_utf16().collect::<Vec<_>>(),
            &b.encode_utf16().collect::<Vec<_>>(),
        )
        .unwrap();
        let result = patch::apply(a, &String::from_utf16(&diff).unwrap()).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "{}",
            String::from_utf16(&diff).unwrap()
        );
        assert_eq!(result.text, b);
    }
}
#[test]
fn patch_rejects_atomically() {
    let patch = "--- a\n+++ b\n@@ -99,1 +88,1 @@\n-a\n+A\n@@ -30,1 +31,1 @@\n-missing\n+B\n";
    let result = patch::apply("a\nb\n", patch).unwrap();
    assert_eq!(result.text, "a\nb\n");
    assert_eq!(result.diagnostics.len(), 1);
}
#[test]
fn diff_keeps_surrogates() {
    assert!(text::diff(&[0xd800], &[0xd801]).unwrap().contains(&0xd800));
}

async fn run(source: &str, data: crate::runtime::StoreData) -> wasmtime::Result<()> {
    use crate::runtime::{RuntimeConfig, install_tenant_limits};
    let compiled = crate::compile_script(source, "code.ts", crate::FileId(0), &[], &[])
        .unwrap_or_else(|e| panic!("{e:#?}"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine()?;
    let mut store = cfg.store(&engine, data)?;
    store.data_mut().install_type_info(compiled.type_info);
    install_tenant_limits(&mut store);
    let module = wasmtime::Module::new(&engine, &compiled.wasm)?;
    let mut linker = wasmtime::Linker::new(&engine);
    crate::runtime::install_runtime_async(&mut linker, &mut store).await?;
    let instance = linker.instantiate_async(&mut store, &module).await?;
    let result = crate::dispatch_main_async(&mut store, &instance).await;
    assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
    result.map(|_| ())
}
#[tokio::test]
async fn workspace_fixture() {
    run(
        include_str!("../../../tests/fixtures/code/workspace.ts"),
        crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().unwrap()),
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn ignore_rules_and_output_modes() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::create_dir(vfs.root().join("src")).unwrap();
    for (path, text) in [
        (".gitignore", "*.tmp\n"),
        ("src/.gitignore", "!keep.tmp\n"),
        ("src/keep.tmp", "Hello\nhello\n"),
        ("src/drop.tmp", "hello"),
        ("src/a.ts", "hello"),
        ("src/.hidden", "hello"),
    ] {
        std::fs::write(vfs.root().join(path), text).unwrap();
    }
    run(
        r#"import { search, tree, glob } from "submilli:code";
    function main(): void {
        const found = search("hello", {path:"src",caseSensitive:false,mode:"counts"});
        assert(found.counts.length === 2);
        assert(found.counts[1].count === 2);
        assert(tree("src",1).entries.length === 2);
        assert(glob("src/*.tmp").entries.length === 1);
        assert(search("hello", {path:"src",include:["*.ts"]}).matches.length === 1);
        assert(search("hello", {path:"src",exclude:["*.tmp"]}).matches.length === 1);
        assert(search("hello", {path:"src",caseSensitive:false,limit:1}).truncated);
    }"#,
        crate::runtime::StoreData::with_vfs(vfs),
    )
    .await
    .unwrap();
}
struct Deny {
    capability: &'static str,
    path: &'static str,
}
impl crate::runtime::SecurityCheck for Deny {
    fn check(
        &self,
        caller: &str,
        capability: &str,
        ctx: &serde_json::Value,
    ) -> crate::runtime::security::CheckOutcome {
        assert_eq!(caller, "main");
        assert!(capability.starts_with("fs."));
        if capability == self.capability && ctx["path"] == self.path {
            crate::runtime::security::CheckOutcome::Deny {
                reason: "test denial".into(),
            }
        } else {
            crate::runtime::security::CheckOutcome::Allow
        }
    }
}
#[tokio::test]
async fn permissions_and_no_partial_write() {
    for (capability, path, source) in [
        (
            "fs.read",
            "/secret",
            "import { search } from 'submilli:code'; function main(): void { search('secret'); }",
        ),
        (
            "fs.read",
            "/.gitignore",
            "import { tree } from 'submilli:code'; function main(): void { tree('/'); }",
        ),
        (
            "fs.write",
            "/secret",
            "import { edit } from 'submilli:code'; function main(): void { edit('/secret','secret','new'); }",
        ),
        (
            "fs.list",
            "/",
            "import { glob } from 'submilli:code'; function main(): void { glob('*'); }",
        ),
        (
            "fs.stat",
            "/secret",
            "import { tree } from 'submilli:code'; function main(): void { tree('/'); }",
        ),
    ] {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        std::fs::write(vfs.root().join("secret"), "secret").unwrap();
        std::fs::write(vfs.root().join(".gitignore"), "").unwrap();
        let mut data = crate::runtime::StoreData::with_vfs(vfs.clone());
        data.security_check = std::sync::Arc::new(Deny { capability, path });
        assert!(
            run(source, data)
                .await
                .unwrap_err()
                .to_string()
                .contains("test denial")
        );
        assert_eq!(
            std::fs::read_to_string(vfs.root().join("secret")).unwrap(),
            "secret"
        );
    }
}
#[tokio::test]
async fn invalid_utf8_limits_and_disabled_vfs() {
    let source = "import { read } from 'submilli:code'; function main(): void { read('/file'); }";
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::write(vfs.root().join("file"), [255]).unwrap();
    assert!(
        run(source, crate::runtime::StoreData::with_vfs(vfs.clone()))
            .await
            .unwrap_err()
            .to_string()
            .contains("UTF-8")
    );
    std::fs::write(vfs.root().join("file"), "123456").unwrap();
    let mut data = crate::runtime::StoreData::with_vfs(vfs);
    data.fs_max_read_size = 4;
    assert!(
        run(source, data)
            .await
            .unwrap_err()
            .to_string()
            .contains("maxReadSize")
    );
    assert!(
        run(
            source,
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::none())
        )
        .await
        .is_err()
    );
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_containment() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), vfs.root().join("escape")).unwrap();
    let data = || crate::runtime::StoreData::with_vfs(vfs.clone());
    run("import { search, tree } from 'submilli:code'; function main(): void { assert(search('secret').matches.length === 0); assert(tree('/').entries[0].kind === 'symlink'); }",data()).await.unwrap();
    assert!(run("import { read } from 'submilli:code'; function main(): void { read('/escape/secret'); }",data()).await.unwrap_err().to_string().contains("escapes"));
}

#[test]
fn overlapping_anchors_are_ambiguous() {
    assert_eq!(
        text::replace("aaa", "aa", "x", false, 0, usize::MAX)
            .unwrap()
            .diagnostics
            .len(),
        2
    );
    assert_eq!(
        text::replace("aaa", "aa", "x", true, 0, usize::MAX)
            .unwrap()
            .text,
        "xa"
    );
    let result = patch::apply("a\na\na\n", "--- a\n+++ b\n@@ -1,2 +1,1 @@\n-a\n-a\n+b\n").unwrap();
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn insertion_preserves_bom() {
    assert_eq!(
        text::insert("\u{feff}a\n", 1, "b\n").unwrap().text,
        "\u{feff}b\na\n"
    );
    assert_eq!(text::insert("\u{feff}", 1, "b").unwrap().text, "\u{feff}b");
}
#[test]
fn patch_rejects_content_after_eof_marker() {
    for patch in [
        "--- a\n+++ b\n@@ -1,2 +1,1 @@\n-a\n\\ No newline at end of file\n-b\n+c\n",
        "--- a\n+++ b\n@@ -1,1 +1,2 @@\n-a\n+b\n\\ No newline at end of file\n+c\n",
    ] {
        assert!(patch::apply("ab\n", patch).is_err());
    }
}
#[tokio::test]
async fn context_results_truncate_before_expansion() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::write(
        vfs.root().join("file"),
        "match xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n".repeat(100),
    )
    .unwrap();
    let mut data = crate::runtime::StoreData::with_vfs(vfs);
    data.fs_max_read_size = 65536;
    run("import { search } from 'submilli:code'; function main(): void { const r = search('match', {context:1000}); assert(r.truncated); assert(r.matches.length < 10); }",data).await.unwrap();
}
#[tokio::test]
async fn glob_star_stays_in_directory() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::create_dir_all(vfs.root().join("src/nested")).unwrap();
    std::fs::write(vfs.root().join("src/a.ts"), "x").unwrap();
    std::fs::write(vfs.root().join("src/nested/a.ts"), "x").unwrap();
    run("import { glob, search } from 'submilli:code'; function main(): void { assert(glob('src/*.ts').entries.length === 1); assert(glob('src/**/*.ts').entries.length === 2); assert(search('x',{path:'src',include:['*.ts']}).matches.length === 1); }",crate::runtime::StoreData::with_vfs(vfs)).await.unwrap();
}

#[test]
fn repetitive_anchor_diagnostics_are_bounded() {
    assert!(
        text::replace(
            &"a".repeat(2000),
            &"a".repeat(100),
            "b",
            false,
            0,
            usize::MAX
        )
        .err()
        .unwrap()
        .to_string()
        .contains("diagnostic limit")
    );
    assert!(
        text::replace(&"  a\n".repeat(2000), "a\n", "b\n", false, 0, usize::MAX)
            .err()
            .unwrap()
            .to_string()
            .contains("diagnostic limit")
    );
}
#[test]
fn patch_eof_marker_must_resolve_to_eof() {
    let patch = "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-a\n+A\n\\ No newline at end of file\n";
    let result = patch::apply("a\nb\n", patch).unwrap();
    assert_eq!(result.text, "a\nb\n");
    assert!(!result.diagnostics.is_empty());
    assert_eq!(patch::apply("a\n", patch).unwrap().text, "A");
}

#[tokio::test]
async fn noop_edit_near_file_limit() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::write(vfs.root().join("file"), "a".repeat(600)).unwrap();
    let mut data = crate::runtime::StoreData::with_vfs(vfs);
    data.fs_max_read_size = 1024;
    run("import { edit } from 'submilli:code'; function main(): void { const r = edit('/file','a','a',true); assert(r.success); assert(!r.changed); }",data).await.unwrap();
}

#[tokio::test]
async fn binary_search_skips_before_utf8_decoding() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::write(vfs.root().join("binary"), [0, 255, 254]).unwrap();
    run("import { search } from 'submilli:code'; function main(): void { assert(search('.').matches.length === 0); }",crate::runtime::StoreData::with_vfs(vfs)).await.unwrap();
}

#[tokio::test]
async fn nul_in_ignore_comment_does_not_disable_rules() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::write(vfs.root().join(".gitignore"), "secret\n#\0\n").unwrap();
    std::fs::write(vfs.root().join("secret"), "secret").unwrap();
    run("import { search,tree } from 'submilli:code'; function main(): void { assert(search('secret').matches.length === 0); assert(tree('/').entries.length === 0); }",crate::runtime::StoreData::with_vfs(vfs)).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn special_files_are_refused_before_reading() {
    use std::os::unix::fs::FileTypeExt;
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    let pipe = vfs.root().join("pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(vfs.root().join("ordinary"), "old").unwrap();
    std::os::unix::fs::symlink("pipe", vfs.root().join("linked-pipe")).unwrap();
    for expression in [
        "read('/pipe')",
        "read('/linked-pipe')",
        "edit('/pipe','old','new')",
        "insertAt('/pipe',1,'new')",
        "diffFiles('/ordinary','/pipe')",
        "applyPatch('/pipe','')",
    ] {
        let source = format!(
            "import {{ read, edit, insertAt, diffFiles, applyPatch }} from 'submilli:code'; function main(): void {{ {expression}; }}"
        );
        let error = run(&source, crate::runtime::StoreData::with_vfs(vfs.clone()))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("not a regular file"),
            "{expression}: {error}"
        );
    }
    assert!(
        std::fs::symlink_metadata(pipe)
            .unwrap()
            .file_type()
            .is_fifo()
    );
}

#[tokio::test]
async fn bom_prefixed_ignore_rules_hide_files() {
    for name in [".gitignore", ".ignore"] {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        std::fs::write(vfs.root().join(name), "\u{feff}secret.txt\n").unwrap();
        std::fs::write(vfs.root().join("secret.txt"), "secret").unwrap();
        run("import { search, tree, glob } from 'submilli:code'; function main(): void { assert(search('secret').matches.length === 0); assert(tree('/').entries.length === 0); assert(glob('**/*').entries.length === 0); }", crate::runtime::StoreData::with_vfs(vfs)).await.unwrap();
    }
}

#[tokio::test]
async fn custom_ignore_precedes_nested_gitignore() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::create_dir(vfs.root().join("src")).unwrap();
    std::fs::write(vfs.root().join(".ignore"), "secret.txt\n").unwrap();
    std::fs::write(vfs.root().join("src/.gitignore"), "!secret.txt\n").unwrap();
    std::fs::write(vfs.root().join("src/secret.txt"), "secret").unwrap();
    run("import { search, tree, glob } from 'submilli:code'; function main(): void { assert(search('secret').matches.length === 0); assert(tree('/src').entries.length === 0); assert(glob('**/*.txt').entries.length === 0); }", crate::runtime::StoreData::with_vfs(vfs)).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn code_mutations_preserve_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o755, 0o600] {
        for expression in [
            "edit('/file','old','new')",
            "insertAt('/file',1,'new')",
            "applyPatch('/file','--- a\\n+++ b\\n@@ -1,1 +1,1 @@\\n-old\\n+new\\n')",
        ] {
            let vfs = crate::runtime::Vfs::tempdir().unwrap();
            let path = vfs.root().join("file");
            std::fs::write(&path, "old\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let source = format!(
                "import {{ edit, insertAt, applyPatch }} from 'submilli:code'; function main(): void {{ assert({expression}.changed); }}"
            );
            run(&source, crate::runtime::StoreData::with_vfs(vfs.clone()))
                .await
                .unwrap();
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                mode,
                "{expression}"
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn navigation_root_rejects_symlink_components() {
    let vfs = crate::runtime::Vfs::tempdir().unwrap();
    std::fs::create_dir_all(vfs.root().join("real/nested")).unwrap();
    std::fs::write(vfs.root().join("real/nested/file"), "secret").unwrap();
    std::os::unix::fs::symlink("real", vfs.root().join("alias")).unwrap();
    for path in ["/alias", "/alias/nested"] {
        for expression in [
            format!("tree('{path}')"),
            format!("search('secret',{{path:'{path}'}})"),
        ] {
            let source = format!(
                "import {{ tree, search }} from 'submilli:code'; function main(): void {{ {expression}; }}"
            );
            let error = run(&source, crate::runtime::StoreData::with_vfs(vfs.clone()))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("must not traverse symlink"),
                "{expression}: {error}"
            );
        }
    }
}

#[test]
fn patch_matches_line_starts_without_byte_overlap_search() {
    let original = "a".repeat(80_000);
    let patch = format!(
        "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-{}\n\\ No newline at end of file\n+b\n",
        "a".repeat(40_000)
    );
    let result = patch::apply(&original, &patch).unwrap();
    assert_eq!(result.text, original);
    assert_eq!(result.diagnostics.len(), 1);
    let overlapping = "--- a\n+++ b\n@@ -1,2 +1,1 @@\n-a\n-a\n+b\n";
    let result = patch::apply("a\na\na\n", overlapping).unwrap();
    assert_eq!(result.text, "a\na\na\n");
    assert!(
        result.diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("ambiguous")
    );
}

#[test]
fn diff_handles_long_shared_line_prefixes() {
    let prefix = "a".repeat(1024);
    let old = format!("{prefix}b\n").repeat(200);
    let new = format!("{prefix}c\n").repeat(200);
    let diff = text::diff(
        &old.encode_utf16().collect::<Vec<_>>(),
        &new.encode_utf16().collect::<Vec<_>>(),
    )
    .unwrap();
    let patch = String::from_utf16(&diff).unwrap();
    let result = patch::apply(&old, &patch).unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.text, new);
}

#[test]
fn native_work_consumes_store_fuel_and_respects_refills() {
    use super::budget::Budget;
    use crate::runtime::{RuntimeConfig, StoreData, Vfs};
    use wasmtime::{Func, FuncType, Trap, Val, ValType};

    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut store = config
        .store(&engine, StoreData::with_vfs(Vfs::none()))
        .unwrap();
    let charge = Func::new(
        &mut store,
        FuncType::new(&engine, [ValType::I32], []),
        |mut caller, params, _| Budget::work(&mut caller, params[0].unwrap_i32() as usize),
    );

    store.set_fuel(300).unwrap();
    charge.call(&mut store, &[Val::I32(100)], &mut []).unwrap();
    assert_eq!(store.get_fuel().unwrap(), 200);
    charge.call(&mut store, &[Val::I32(200)], &mut []).unwrap();
    assert_eq!(store.get_fuel().unwrap(), 0);

    store.set_fuel(50).unwrap();
    charge.call(&mut store, &[Val::I32(20)], &mut []).unwrap();
    assert_eq!(store.get_fuel().unwrap(), 30);
    let error = charge
        .call(&mut store, &[Val::I32(31)], &mut [])
        .unwrap_err();
    assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::OutOfFuel));
    assert_eq!(store.get_fuel().unwrap(), 0);
}
#[tokio::test]
async fn edits_are_counted_against_the_size_limit() {
    let source = r#"
        import { writeText, lines } from "submilli:fs";
        import { edit, insertAt } from "submilli:code";
        function refused(write: () => void): boolean {
            try { write(); return false; } catch (e) { return e instanceof RangeError; }
        }
        function main(): void {
            writeText("/a.ts", "x".repeat(50000) + "\nfirst\n");
            assert(refused(() => insertAt("/a.ts", 1, "y".repeat(50000) + "\n")), "growth past the limit");
            assert(edit("/a.ts", "first", "second").changed, "a rewrite within the limit");
            for (const line of lines("/a.ts")) {
                assert(refused(() => insertAt("/a.ts", 1, "z\n")), "a held file's old copy still counts");
                break;
            }
            insertAt("/a.ts", 1, "z\n");
        }
    "#;
    let vfs = crate::runtime::Vfs::tempdir()
        .unwrap()
        .with_size_limit(100_000);
    run(source, crate::runtime::StoreData::with_vfs(vfs))
        .await
        .unwrap();
}
