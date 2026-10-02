//! The `submilli:fs` package declaration — the LLM-facing type surface
//! (functions, `Stat` / `Peek` / `DirEntry` / `Info` / `MountInfo` / `FileWriter`).

use std::collections::BTreeMap;

use crate::{
    Dispatch, MethodSig, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol,
    ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);

    let path_param = Param::new("path", Type::String);
    let bytes_or_null = Type::union(vec![Type::Uint8Array, Type::Null]);
    let string_or_null = Type::union(vec![Type::String, Type::Null]);

    insert_stat_interface(&mut defs);
    insert_peek_interface(&mut defs);
    insert_file_writer_interface(&mut defs);
    insert_dir_entry_interface(&mut defs);
    insert_mount_info_interface(&mut defs);
    insert_info_interface(&mut defs);

    insert_fn(
        &mut defs,
        "maxReadSize",
        Vec::new(),
        Type::Number,
        "/**\n * Maximum number of bytes `read` / `readText` will load whole; past this they return `null`. Also the upper bound on `readBytes(path, offset, length)`'s `length`. Configured by the harness per Store; defaults to the 50 MB tier memory cap.\n */",
    );
    insert_fn(
        &mut defs,
        "info",
        Vec::new(),
        Type::InterfaceRef {
            mangled: crate::mangle::package_symbol(MODULE_NAME, "Info"),
            package: crate::Package(MODULE_NAME.to_string()),
            name: "Info".to_string(),
            args: Vec::new(),
        },
        "/**\n * The active VFS configuration: the root's `mode` (`\"none\"` / `\"ephemeral\"` / `\"per_session\"` / `\"named\"`), `access`, `volume` and `sizeLimit` (the cap on the bytes its files may hold, or `-1` when none applies), plus `mounts`, the named volumes grafted below the root. Deterministic; no capability required. The host directories backing the VFS are never exposed.\n */",
    );
    insert_fn(
        &mut defs,
        "exists",
        vec![path_param.clone()],
        Type::Boolean,
        "/**\n * Returns `true` iff `path` resolves to a filesystem entry under the VFS root. Symlinks count as existing; broken symlinks count as not existing (mirrors `std::path::exists`).\n * @param path File path under the VFS root.\n * @capability fs.stat { path }\n */",
    );
    insert_fn(
        &mut defs,
        "size",
        vec![path_param.clone()],
        Type::Number,
        "/**\n * File size in bytes. Traps on directories or missing paths — call `exists(path)` first or use `stat(path)` if you need to handle absence.\n * @param path File path under the VFS root.\n * @capability fs.stat { path }\n */",
    );
    insert_fn(
        &mut defs,
        "stat",
        vec![path_param.clone()],
        Type::Union(vec![
            Type::InterfaceRef {
                mangled: crate::mangle::package_symbol(MODULE_NAME, "Stat"),
                package: crate::Package(MODULE_NAME.to_string()),
                name: "Stat".to_string(),
                args: Vec::new(),
            },
            Type::Null,
        ]),
        "/**\n * Metadata about an entry under the VFS root. Returns `null` when the path does not exist; otherwise returns a `Stat` carrying `kind` / `size` / `modifiedAt`.\n * @param path File path under the VFS root.\n * @capability fs.stat { path }\n */",
    );
    insert_fn(
        &mut defs,
        "peek",
        vec![path_param.clone()],
        Type::InterfaceRef {
            mangled: crate::mangle::package_symbol(MODULE_NAME, "Peek"),
            package: crate::Package(MODULE_NAME.to_string()),
            name: "Peek".to_string(),
            args: Vec::new(),
        },
        "/**\n * Quick file-only inspection: preview (first ~256 bytes decoded UTF-8 lossy), detected `encoding` (`utf-8` / `utf-16le` / `utf-16be` / `latin1`), `lineEnding` (`lf` / `crlf`), and total `size`. Traps on directories / missing paths.\n * @param path File path under the VFS root.\n * @capability fs.stat { path }\n */",
    );
    insert_fn(
        &mut defs,
        "read",
        vec![path_param.clone()],
        bytes_or_null.clone(),
        "/**\n * Read the whole file as a `Uint8Array`. Returns `null` when the file is larger than `maxReadSize()` — the typechecker forces narrowing before use. Traps on missing paths or directories.\n * @param path File path under the VFS root.\n * @capability fs.read { path }\n */",
    );
    insert_fn(
        &mut defs,
        "readText",
        vec![path_param.clone()],
        string_or_null.clone(),
        "/**\n * Read the whole file as a UTF-8 string. Returns `null` when the file is larger than `maxReadSize()`. Strips a leading UTF-8 BOM by default; invalid bytes are replaced by U+FFFD.\n * @param path File path under the VFS root.\n * @capability fs.read { path }\n */",
    );
    insert_fn(
        &mut defs,
        "write",
        vec![path_param.clone(), Param::new("content", Type::Uint8Array)],
        Type::Void,
        "/**\n * Write `content` to `path` atomically (temp-file + rename). Throws when the parent directory does not exist — agents must `mkdir(parent, { recursive: true })` first.\n * @param path File path under the VFS root.\n * @param content Bytes to write.\n * @capability fs.write { path, length: number }\n */",
    );
    insert_fn(
        &mut defs,
        "writeText",
        vec![path_param.clone(), Param::new("content", Type::String)],
        Type::Void,
        "/**\n * UTF-8 atomic write. Same parent-directory rule as `write`.\n * @param path File path under the VFS root.\n * @param content Text to write as UTF-8.\n * @capability fs.write { path, length: number }\n */",
    );
    insert_fn(
        &mut defs,
        "append",
        vec![path_param.clone(), Param::new("content", Type::Uint8Array)],
        Type::Void,
        "/**\n * Append `content` to `path`. Creates the file if missing; throws when the parent directory does not exist. NOT atomic — concurrent writers race.\n * @param path File path under the VFS root.\n * @param content Bytes to append.\n * @capability fs.write { path, length: number }\n */",
    );
    insert_fn(
        &mut defs,
        "appendText",
        vec![path_param.clone(), Param::new("content", Type::String)],
        Type::Void,
        "/**\n * UTF-8 append. Same parent-directory rule as `append`.\n * @param path File path under the VFS root.\n * @param content Text to append as UTF-8.\n * @capability fs.write { path, length: number }\n */",
    );
    insert_fn(
        &mut defs,
        "readBytes",
        vec![
            path_param.clone(),
            Param::new("offset", Type::Number),
            Param::new("length", Type::Number),
        ],
        Type::Uint8Array,
        "/**\n * Random-access byte-range read. Traps if `length > maxReadSize()`, if `offset` is negative, or on I/O errors. The returned `Uint8Array` may be shorter than `length` if the request runs past EOF.\n * @param path File path under the VFS root.\n * @param offset Zero-based byte offset.\n * @param length Maximum number of bytes to read.\n * @capability fs.read { path, length }\n */",
    );
    insert_fn(
        &mut defs,
        "mkdir",
        vec![path_param.clone(), Param::new("recursive", Type::Boolean)],
        Type::Void,
        "/**\n * Create the directory at `path`. `recursive=true` creates intermediate directories (like `mkdir -p`); `recursive=false` throws when the parent is missing or the target already exists. Until options objects land, the flag is positional.\n * @param path Directory path under the VFS root.\n * @param recursive Whether to create missing parents.\n * @capability fs.mkdir { path, recursive }\n */",
    );
    insert_fn(
        &mut defs,
        "remove",
        vec![path_param.clone(), Param::new("recursive", Type::Boolean)],
        Type::Void,
        "/**\n * Remove the entry at `path`. Files unconditionally; directories require `recursive=true` (matches `rm -rf`). Without the flag, a non-empty directory throws.\n * @param path File or directory path under the VFS root.\n * @param recursive Whether to remove directories recursively.\n * @capability fs.remove { path, recursive }\n */",
    );
    insert_fn(
        &mut defs,
        "move",
        vec![
            Param::new("from", Type::String),
            Param::new("to", Type::String),
        ],
        Type::Void,
        "/**\n * Move / rename. Atomic within one volume. Between volumes (the root and a mount, or two mounts — see `info().mounts`) it copies, then removes the source, so it is not atomic: if removing the source fails, the destination is kept and the call throws with both copies present. Replacing a file in another volume needs room there for both versions until the move completes, and a link the copy cannot reproduce fails the move before the source is touched.\n * @param from Source path under the VFS root.\n * @param to Destination path under the VFS root.\n * @capability fs.move { from, to }\n */",
    );
    insert_fn(
        &mut defs,
        "copy",
        vec![
            Param::new("from", Type::String),
            Param::new("to", Type::String),
            Param::new("recursive", Type::Boolean),
        ],
        Type::Void,
        "/**\n * Copy `from` to `to`. Directories require `recursive=true`. Symlinks are copied as links (the link target is preserved; the file it points at is not followed) so a writable VFS area can't be used to exfiltrate the target.\n * @param from Source path under the VFS root.\n * @param to Destination path under the VFS root.\n * @param recursive Whether to copy directories recursively.\n * @capability fs.copy { from, to, recursive }\n */",
    );
    insert_fn(
        &mut defs,
        "writer",
        vec![path_param.clone()],
        Type::InterfaceRef {
            mangled: crate::mangle::package_symbol(MODULE_NAME, "FileWriter"),
            package: crate::Package(MODULE_NAME.to_string()),
            name: "FileWriter".to_string(),
            args: Vec::new(),
        },
        "/**\n * Open `path` as an append-only `FileWriter`. The writer buffers internally and only commits on `close()` (atomic temp-file + rename) — half-written outputs from a crashed transform don't leak. Throws when the parent directory does not exist.\n * @param path Destination path under the VFS root.\n * @capability fs.write { path }\n */",
    );
    insert_fn(
        &mut defs,
        "lines",
        vec![path_param.clone()],
        Type::InterfaceRef {
            mangled: crate::mangle::prelude("Iterator"),
            package: crate::Package::prelude(),
            name: "Iterator".to_string(),
            args: vec![Type::String],
        },
        "/**\n * Stream the file at `path` as a constant-memory line iterator. Each `next()` returns one line stripped of its trailing CR/LF. A leading UTF-8 BOM on the first line is dropped. The file descriptor is released when the iterator is closed (a `for...of` loop closes it on exit, including `break`/`throw`) or, failing that, when it is garbage-collected.\n * @param path File path under the VFS root.\n * @capability fs.read { path }\n */",
    );
    insert_fn(
        &mut defs,
        "bytes",
        vec![path_param.clone(), Param::new("chunkSize", Type::Number)],
        Type::InterfaceRef {
            mangled: crate::mangle::prelude("Iterator"),
            package: crate::Package::prelude(),
            name: "Iterator".to_string(),
            args: vec![Type::Uint8Array],
        },
        "/**\n * Stream the file at `path` as a constant-memory byte iterator. Each `next()` returns up to `chunkSize` bytes; the last chunk may be shorter. Sized for large binary inputs that don't fit `maxReadSize`.\n * @param path File path under the VFS root.\n * @param chunkSize Maximum bytes per iterator chunk.\n * @capability fs.read { path, chunkSize }\n */",
    );
    insert_fn(
        &mut defs,
        "list",
        vec![path_param.clone(), Param::new("recursive", Type::Boolean)],
        Type::InterfaceRef {
            mangled: crate::mangle::prelude("Iterator"),
            package: crate::Package::prelude(),
            name: "Iterator".to_string(),
            args: vec![Type::InterfaceRef {
                mangled: crate::mangle::package_symbol(MODULE_NAME, "DirEntry"),
                package: crate::Package(MODULE_NAME.to_string()),
                name: "DirEntry".to_string(),
                args: Vec::new(),
            }],
        },
        "/**\n * List the children of the directory at `path` as a constant-memory iterator of `DirEntry`. `recursive=true` walks descendants as well; `recursive=false` yields only direct children.\n *\n * Order: a directory is always yielded before its contents, and a directory's whole subtree is yielded as one consecutive run — no entry from outside a subtree is interleaved into it. Down to 32 levels of nesting that is exactly depth-first (matching `find` / `os.walk`); deeper than that the walk runs out of directory handles and a subdirectory's contents follow the rest of its parent's entries instead of coming directly after it, so don't depend on strict depth-first order past that depth. Siblings come in whatever order the filesystem reports — not sorted.\n *\n * Symlinks surface as `kind=\"symlink\"` and are not followed. A recursive walk is best-effort: a subdirectory it cannot read — or, past a fixed ceiling on how many postponed descents one walk holds, cannot afford to remember — is skipped and the walk continues, so a listing is not proof that a path is absent.\n * @param path Directory path under the VFS root.\n * @param recursive Whether to walk descendants recursively.\n * @capability fs.list { path, recursive }\n */",
    );

    defs
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::FS),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::FS, doc),
            },
        },
    );
}

fn insert_stat_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "kind",
        Type::String,
        "/** One of `\"file\"` / `\"directory\"` / `\"symlink\"` / `\"other\"`. Symlinks are reported as `\"symlink\"` without dereferencing. */",
    );
    insert_property(
        &mut properties,
        "size",
        Type::Number,
        "/** File size in bytes. `0` for non-files (directories, symlinks, other). */",
    );
    insert_property(
        &mut properties,
        "modifiedAt",
        Type::Number,
        "/** Last-modified timestamp in milliseconds since the Unix epoch. `0` when the underlying filesystem doesn't report one. */",
    );
    defs.types.insert(
        "Stat".to_string(),
        TypeSymbol {
            name: "Stat".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Stat"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** Filesystem metadata returned by `stat(path)`. Constructed only by `stat`; user code reads `kind` / `size` / `modifiedAt` via property access. */",
                ),
            },
        },
    );
}

fn insert_peek_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "preview",
        Type::String,
        "/** First ~256 bytes of the file decoded as UTF-8 lossy. Use `lines` / `read` for full content. */",
    );
    insert_property(
        &mut properties,
        "encoding",
        Type::String,
        "/** Detected text encoding: `\"utf-8\"` / `\"utf-16le\"` / `\"utf-16be\"` / `\"latin1\"`. BOM-driven when a BOM is present; falls back to a UTF-8 validity check then `latin1`. */",
    );
    insert_property(
        &mut properties,
        "lineEnding",
        Type::String,
        "/** `\"crlf\"` if a CRLF sequence appears in the preview; `\"lf\"` otherwise (including files with no line terminators yet). */",
    );
    insert_property(
        &mut properties,
        "size",
        Type::Number,
        "/** Total file size in bytes — same value as `size(path)`. */",
    );
    defs.types.insert(
        "Peek".to_string(),
        TypeSymbol {
            name: "Peek".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Peek"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** Lightweight file-only inspection — preview + transport metadata. Constructed only by `peek`; user code reads fields via property access. */",
                ),
            },
        },
    );
}

fn insert_dir_entry_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "kind",
        Type::String,
        "/** `\"file\"` / `\"directory\"` / `\"symlink\"` / `\"other\"`. */",
    );
    insert_property(
        &mut properties,
        "name",
        Type::String,
        "/** Basename of the entry (no parent path). */",
    );
    insert_property(
        &mut properties,
        "path",
        Type::String,
        "/** Full guest-visible path under the VFS root, with a leading `/`. */",
    );
    insert_property(
        &mut properties,
        "size",
        Type::Number,
        "/** File size in bytes; `0` for non-files. */",
    );
    defs.types.insert(
        "DirEntry".to_string(),
        TypeSymbol {
            name: "DirEntry".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "DirEntry"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** Directory entry yielded by `list(path, recursive)`. Properties: `kind` / `name` / `path` / `size`. Constructed only by the iterator. */",
                ),
            },
        },
    );
}

fn insert_info_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "mode",
        Type::String,
        "/** The root's mode: `\"none\"` / `\"ephemeral\"` / `\"per_session\"` / `\"named\"`. */",
    );
    insert_property(
        &mut properties,
        "sizeLimit",
        Type::Number,
        "/** The most bytes the files in the root may hold, or `-1` when no limit applies. A write that would pass it throws a `RangeError`. A named volume's limit is shared with every session and blueprint that uses it. */",
    );
    insert_property(
        &mut properties,
        "access",
        Type::String,
        "/** `\"read_write\"`, or `\"read_only\"` when every write to the root throws a `PermissionDeniedError`. */",
    );
    insert_property(
        &mut properties,
        "volume",
        Type::String,
        "/** The named volume backing the root under `mode: \"named\"`; `\"\"` otherwise, and for a directory `submilli run --vfs` exposes. */",
    );
    insert_property(
        &mut properties,
        "mounts",
        Type::Array(Box::new(mount_info_type())),
        "/** The named volumes mounted below the root, sorted by path; empty when there are none. Paths under a mount's `path` read and write that volume. */",
    );
    defs.types.insert(
        "Info".to_string(),
        TypeSymbol {
            name: "Info".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Info"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** VFS configuration returned by `info()`: the root's `mode`, `access`, `volume` and `sizeLimit` (`-1` when no limit applies), plus its `mounts`. Never carries a host path. */",
                ),
            },
        },
    );
}

fn mount_info_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "MountInfo"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "MountInfo".to_string(),
        args: Vec::new(),
    }
}

fn insert_mount_info_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "path",
        Type::String,
        "/** The guest path the volume is mounted at, such as `/memory`. */",
    );
    insert_property(
        &mut properties,
        "mode",
        Type::String,
        "/** Always `\"named\"`: mounts are named volumes the server declares. */",
    );
    insert_property(
        &mut properties,
        "volume",
        Type::String,
        "/** The volume's name. Its files persist across runs and sessions, and other blueprints that mount it see them. */",
    );
    insert_property(
        &mut properties,
        "access",
        Type::String,
        "/** `\"read_write\"`, or `\"read_only\"` when every write under `path` throws a `PermissionDeniedError`. */",
    );
    insert_property(
        &mut properties,
        "sizeLimit",
        Type::Number,
        "/** The most bytes the volume may hold, shared with everyone who mounts it, or `-1` when no limit applies. */",
    );
    defs.types.insert(
        "MountInfo".to_string(),
        TypeSymbol {
            name: "MountInfo".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "MountInfo"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** One volume mounted below the VFS root, as `info().mounts` lists it. Mount points cannot be removed or moved. Never carries a host path. */",
                ),
            },
        },
    );
}

fn insert_file_writer_interface(defs: &mut PackageDeclaration) {
    let mut methods = BTreeMap::new();
    methods.insert(
        "close".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(crate::FileId::FS,
                "/** Flush the internal buffer, fsync the temp file, and atomically rename it over the destination. Subsequent method calls on this writer trap — close once. */",
            ),
        },
    );
    methods.insert(
        "writeBytes".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: vec![Param::new("bytes", Type::Uint8Array)],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(crate::FileId::FS,
                "/** Append `bytes` to the buffer. Crash before `close()` means nothing reaches `final_path`. */",
            ),
        },
    );
    methods.insert(
        "writeLine".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: vec![Param::new("line", Type::String)],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(crate::FileId::FS,
                "/** Append `line` followed by `\\n` (LF). Pair with `appendText` for CRLF-aware output. */",
            ),
        },
    );
    defs.types.insert(
        "FileWriter".to_string(),
        TypeSymbol {
            name: "FileWriter".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "FileWriter"),
            declaration_span: Span::at(crate::FileId::FS),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods,
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::FS,
                    "/** Append-only writer returned by `writer(path)`. Internal buffer + atomic temp-file + rename on `close()`. Construct only via `writer`; user code uses `writeLine` / `writeBytes` / `close`. */",
                ),
            },
        },
    );
}

fn insert_property(
    properties: &mut BTreeMap<String, PropertySig>,
    name: &str,
    ty: Type,
    doc: &str,
) {
    properties.insert(
        name.to_string(),
        PropertySig {
            ty,
            readonly: true,
            intrinsic: false,
            optional: false,
            doc: crate::doc(crate::FileId::FS, doc),
        },
    );
}
