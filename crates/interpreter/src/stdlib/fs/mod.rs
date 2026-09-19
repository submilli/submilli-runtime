//! `submilli:fs` — agent-facing filesystem library.
//!
//! Pure Rust host functions registered directly under the package name. Each
//! gated op runs `check_security` before touching the disk; the set of gated
//! capabilities is cataloged in [`crate::stdlib::capabilities`] — keep it in
//! sync when adding or removing a gate (see CLAUDE.md).
//!
//! `Stat` / `Peek` / `DirEntry` / `Info` are host-built backing structs the
//! guest holds opaquely and reads through the registered getters. `FileWriter`
//! and the `lines` / `bytes` / `list` iterators wrap [`handles`] payloads in
//! `externref`s; the iterators are closable (`for…of` releases the OS handle
//! on every loop exit) and GC reclaim is the backstop.

pub mod declaration;
pub mod handles;

use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use wasmtime::{
    Caller, ExternRef, FieldType, Finality, Func, FuncType, HeapType, Linker, Mutability, RefType,
    Rooted, StorageType, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fs::{ContainError, ContentPath, LinkPath, guest_normalize, resolve_link};
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::{
    read_string_arg, read_uint8_array_arg, register_host_fn, write_submilli_string_struct,
    write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::prelude::iterator::{
    as_struct, build_closable_iterator, iter_done, iter_yield, next_closure_type, void_closure_type,
};
use crate::stdlib::abi::{
    self, backing_struct, externref_field, f64_field, install_field_getters, string_field,
};
use crate::stdlib::shared::{
    DEFAULT_CWD, check_security, contain_trap, resolve_content_or_trap, resolve_link_or_trap,
    write_target_trap,
};
use handles::{
    ChargedByteReader, ChargedDirIter, ChargedFileWriter, ChargedLineReader, ContainedWalk, kind_of,
};

pub const MODULE_NAME: &str = "submilli:fs";

pub use declaration::package_declaration;

// `$StatBacking` field indices (0 is the vtable).
const STAT_KIND: usize = 1;
const STAT_SIZE: usize = 2;
const STAT_MODIFIED_AT: usize = 3;

// `$PeekBacking` field indices.
const PEEK_PREVIEW: usize = 1;
const PEEK_ENCODING: usize = 2;
const PEEK_LINE_ENDING: usize = 3;
const PEEK_SIZE: usize = 4;

// `$DirEntryBacking` field indices.
const ENTRY_KIND: usize = 1;
const ENTRY_NAME: usize = 2;
const ENTRY_PATH: usize = 3;
const ENTRY_SIZE: usize = 4;

// `$InfoBacking` field indices.
const INFO_MODE: usize = 1;
const INFO_SIZE_LIMIT: usize = 2;
const INFO_PATH_LIMIT: usize = 3;

// `$FileWriterBacking`: vtable + the externref handle.
const WRITER_HANDLE: usize = 1;

fn stat_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // kind
            f64_field(),         // size
            f64_field(),         // modifiedAt
        ],
    )
}

fn peek_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // preview
            string_field(&intr), // encoding
            string_field(&intr), // lineEnding
            f64_field(),         // size
        ],
    )
}

fn dir_entry_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // kind
            string_field(&intr), // name
            string_field(&intr), // path
            f64_field(),         // size
            wasmtime::FieldType::new(Mutability::Const, StorageType::ValType(ValType::I64)), // nominal marker
        ],
    )
}

fn info_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // mode
            f64_field(),         // sizeLimit
            f64_field(),         // pathLimit
            wasmtime::FieldType::new(Mutability::Const, StorageType::ValType(ValType::I64)), // nominal marker
        ],
    )
}

/// `$FileWriterBacking` — vtable + the `externref` holding the
/// [`ChargedFileWriter`].
fn file_writer_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(engine, &intr, vec![externref_field()])
}

/// The iterator env: a host-private single-field struct carrying the handle
/// `externref` through the closure ABI's `(ref any)` slot.
fn handle_env_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    singleton_struct(
        engine,
        Finality::Final,
        None::<StructType>,
        vec![FieldType::new(
            Mutability::Const,
            StorageType::ValType(ValType::EXTERNREF),
        )],
    )
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let uint8 = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.uint8_array.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    let fs_fn = |name: &str| crate::mangle::package_symbol(MODULE_NAME, name);

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("maxReadSize"),
        FuncType::new(&engine, [], [ValType::F64]),
        /* deterministic = */ true,
        |caller, _params, results| {
            results[0] = Val::F64((caller.data().fs_max_read_size as f64).to_bits());
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("info"),
        FuncType::new(&engine, [], [nullable_object.clone()]),
        /* deterministic = */ true,
        |caller, _params, results| {
            results[0] = build_info(caller)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("exists"),
        FuncType::new(&engine, [string.clone()], [ValType::I32]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.exists")?;
            check_security(
                &*caller,
                "fs.stat",
                serde_json::json!({ "op": "exists", "path": &path }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.exists")?;
            // A path the sandbox cannot reach is reported absent, not trapped: that
            // preserves the documented broken-link behaviour and denies the guest an
            // existence oracle over host layout.
            // `exists` is declared to mirror `std::path::exists` — a total predicate that
            // never throws. Every way a path can be unreachable answers `false`: outside the
            // sandbox, missing, a file used as a directory component, a symlink cycle. A
            // guard clause written as `if (!exists(p))` is exactly what an LLM writes, and it
            // must not abort on the malformed-path case it exists to handle.
            let found = match resolved.try_exists() {
                Ok(found) => found,
                Err(ContainError::Escape | ContainError::Io(_)) => false,
                Err(e) => return Err(contain_trap("fs.exists", &path, &e)),
            };
            results[0] = Val::I32(i32::from(found));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("size"),
        FuncType::new(&engine, [string.clone()], [ValType::F64]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.size")?;
            check_security(
                &*caller,
                "fs.stat",
                serde_json::json!({ "op": "size", "path": &path }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.size")?;
            let meta = resolved
                .metadata()
                .map_err(|e| contain_trap("fs.size", &path, &e))?;
            if meta.is_dir() {
                wasmtime::bail!("fs.size {}: path is a directory", path);
            }
            results[0] = Val::F64((meta.len() as f64).to_bits());
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("stat"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.stat")?;
            check_security(
                &*caller,
                "fs.stat",
                serde_json::json!({ "op": "stat", "path": &path }),
            )?;
            results[0] = stat_entry(caller, &path)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("peek"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.peek")?;
            check_security(
                &*caller,
                "fs.stat",
                serde_json::json!({ "op": "peek", "path": &path }),
            )?;
            results[0] = peek_file(caller, &path)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("read"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.read")?;
            results[0] = match read_whole_capped(caller, &path, "read")? {
                Some(bytes) => {
                    let arr = write_submilli_uint8array_struct(caller, &bytes)?;
                    Val::AnyRef(Some(arr.to_anyref()))
                }
                None => Val::AnyRef(None),
            };
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("readText"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.readText")?;
            results[0] = match read_whole_capped(caller, &path, "readText")? {
                Some(bytes) => {
                    // Lossy UTF-8: invalid bytes become U+FFFD rather than trapping.
                    let mut text = String::from_utf8_lossy(&bytes).into_owned();
                    if text.starts_with('\u{FEFF}') {
                        text.remove(0);
                    }
                    let st = write_submilli_string_struct(caller, &text)?;
                    Val::AnyRef(Some(st.to_anyref()))
                }
                None => Val::AnyRef(None),
            };
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("readBytes"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64, ValType::F64],
            [uint8.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.readBytes")?;
            let offset = f64_arg(&params[1], "fs.readBytes (offset)")? as i64;
            let length = f64_arg(&params[2], "fs.readBytes (length)")? as i64;
            check_security(
                &*caller,
                "fs.read",
                serde_json::json!({
                    "op": "readBytes",
                    "path": &path,
                    "length": length,
                }),
            )?;
            let bytes = read_byte_range(caller, &path, offset, length)?;
            let arr = write_submilli_uint8array_struct(caller, &bytes)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    // write / writeText / append / appendText share the shape
    // `(path, content) -> void`; only the op name, encoding, and disk action differ.
    for (name, op, text_content, atomic) in [
        ("write", "write", false, true),
        ("writeText", "writeText", true, true),
        ("append", "append", false, false),
        ("appendText", "appendText", true, false),
    ] {
        let content_ty = if text_content {
            string.clone()
        } else {
            uint8.clone()
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            fs_fn(name),
            FuncType::new(&engine, [string.clone(), content_ty], []),
            /* deterministic = */ false,
            move |caller, params, _results| {
                let ctx = format!("fs.{op}");
                let path = read_string_arg(&mut *caller, &params[0], &ctx)?;
                let bytes = if text_content {
                    read_string_arg(&mut *caller, &params[1], &ctx)?.into_bytes()
                } else {
                    read_uint8_array_arg(&mut *caller, &params[1], &ctx)?
                };
                check_security(
                    &*caller,
                    "fs.write",
                    serde_json::json!({
                        "op": op,
                        "path": &path,
                        "length": bytes.len(),
                    }),
                )?;
                let resolved = resolve_content_or_trap(caller.data(), &path, &ctx)?;
                if resolved.is_root() {
                    wasmtime::bail!("{ctx} {}: the VFS root is a directory, not a file", path);
                }
                if atomic {
                    atomic_write(&resolved, &bytes, &path, &ctx)
                } else {
                    append_bytes(&resolved, &bytes, &path, &ctx)
                }
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("mkdir"),
        FuncType::new(&engine, [string.clone(), ValType::I32], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.mkdir")?;
            let recursive = i32_flag(&params[1], "fs.mkdir (recursive)")?;
            check_security(
                &*caller,
                "fs.mkdir",
                serde_json::json!({ "op": "mkdir", "path": &path, "recursive": recursive }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.mkdir")?;
            let res = if recursive {
                resolved.create_dir_all()
            } else {
                resolved.create_dir()
            };
            res.map_err(|e| write_target_trap("fs.mkdir", &path, &e))?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("remove"),
        FuncType::new(&engine, [string.clone(), ValType::I32], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.remove")?;
            let recursive = i32_flag(&params[1], "fs.remove (recursive)")?;
            check_security(
                &*caller,
                "fs.remove",
                serde_json::json!({ "op": "remove", "path": &path, "recursive": recursive }),
            )?;
            let resolved = resolve_link_or_trap(caller.data(), &path, "fs.remove")?;
            // `remove_dir_all(".")` drains the root and only then fails on the self-unlink,
            // so without this the guest destroys the operator's volume and is told the call
            // failed. Refuse before touching anything.
            if resolved.is_root() {
                wasmtime::bail!(
                    "fs.remove {}: cannot remove the VFS root itself; remove its entries instead",
                    path
                );
            }
            let meta = resolved
                .symlink_metadata()
                .map_err(|e| contain_trap("fs.remove", &path, &e))?;
            // `is_dir` on link metadata is false for a symlink to a directory, so an
            // escaping link is unlinked rather than followed and recursively deleted.
            let res = if meta.is_dir() {
                if recursive {
                    resolved.remove_dir_all()
                } else {
                    resolved.remove_dir()
                }
            } else {
                resolved.remove_file()
            };
            res.map_err(|e| contain_trap("fs.remove", &path, &e))?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("move"),
        FuncType::new(&engine, [string.clone(), string.clone()], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let from = read_string_arg(&mut *caller, &params[0], "fs.move (from)")?;
            let to = read_string_arg(&mut *caller, &params[1], "fs.move (to)")?;
            check_security(
                &*caller,
                "fs.move",
                serde_json::json!({ "op": "move", "from": &from, "to": &to }),
            )?;
            let from_resolved = resolve_link_or_trap(caller.data(), &from, "fs.move")?;
            let to_resolved = resolve_link_or_trap(caller.data(), &to, "fs.move")?;
            from_resolved
                .rename_to(&to_resolved)
                .map_err(|e| contain_trap("fs.move", &format!("{from} -> {to}"), &e))?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("copy"),
        FuncType::new(&engine, [string.clone(), string.clone(), ValType::I32], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let from = read_string_arg(&mut *caller, &params[0], "fs.copy (from)")?;
            let to = read_string_arg(&mut *caller, &params[1], "fs.copy (to)")?;
            let recursive = i32_flag(&params[2], "fs.copy (recursive)")?;
            check_security(
                &*caller,
                "fs.copy",
                serde_json::json!({
                    "op": "copy",
                    "from": &from,
                    "to": &to,
                    "recursive": recursive,
                }),
            )?;
            let from_resolved = resolve_link_or_trap(caller.data(), &from, "fs.copy")?;
            // A recursive copy used to reach `create_dir_all(to)`, which built the whole
            // destination chain. Resolving the destination now opens its parent, so that
            // chain has to exist first or a working call starts failing.
            if recursive
                && from_resolved
                    .symlink_metadata()
                    .is_ok_and(|meta| meta.is_dir())
            {
                resolve_content_or_trap(caller.data(), &to, "fs.copy")?
                    .create_dir_all()
                    .map_err(|e| write_target_trap("fs.copy", &to, &e))?;
            }
            let to_resolved = resolve_link_or_trap(caller.data(), &to, "fs.copy")?;
            copy_recursive(&from_resolved, &to_resolved, recursive, &from, &to)
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("writer"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.writer (path)")?;
            check_security(
                &*caller,
                "fs.write",
                serde_json::json!({ "op": "writer", "path": &path }),
            )?;
            results[0] = open_writer(caller, &path)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("lines"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.lines (path)")?;
            check_security(
                &*caller,
                "fs.read",
                serde_json::json!({ "op": "lines", "path": &path }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.lines")?;
            let file = resolved
                .open()
                .map_err(|e| contain_trap("fs.lines", &path, &e))?
                .into_std();
            let reader = ChargedLineReader::new(BufReader::new(file), &caller.data().tenant_limits)
                .map_err(|e| wasmtime::Error::msg(format!("fs.lines {path}: {e}")))?;
            results[0] = make_handle_iterator(
                caller,
                reader,
                lines_next,
                close_handle_of::<ChargedLineReader>,
            )?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("bytes"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64],
            [nullable_object.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.bytes (path)")?;
            let chunk_size = f64_arg(&params[1], "fs.bytes (chunkSize)")? as i64;
            if chunk_size <= 0 {
                return Err(crate::runtime::host::range_error(format!(
                    "fs.bytes: chunkSize must be > 0, got {chunk_size}"
                )));
            }
            check_security(
                &*caller,
                "fs.read",
                serde_json::json!({ "op": "bytes", "path": &path, "chunkSize": chunk_size }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.bytes")?;
            let file = resolved
                .open()
                .map_err(|e| contain_trap("fs.bytes", &path, &e))?
                .into_std();
            let reader =
                ChargedByteReader::new(file, chunk_size as usize, &caller.data().tenant_limits)
                    .map_err(|e| wasmtime::Error::msg(format!("fs.bytes {path}: {e}")))?;
            results[0] = make_handle_iterator(
                caller,
                reader,
                bytes_next,
                close_handle_of::<ChargedByteReader>,
            )?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("list"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::I32],
            [nullable_object.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            let path = read_string_arg(&mut *caller, &params[0], "fs.list (path)")?;
            let recursive = i32_flag(&params[1], "fs.list (recursive)")?;
            check_security(
                &*caller,
                "fs.list",
                serde_json::json!({ "op": "list", "path": &path, "recursive": recursive }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.list")?;
            let meta = resolved
                .metadata()
                .map_err(|e| contain_trap("fs.list", &path, &e))?;
            if !meta.is_dir() {
                wasmtime::bail!("fs.list {}: path is not a directory", path);
            }
            let base = Arc::new(
                resolved
                    .open_dir()
                    .map_err(|e| contain_trap("fs.list", &path, &e))?,
            );
            let walk = ContainedWalk::new(base, list_prefix(&path)?, recursive)
                .map_err(|e| contain_trap("fs.list", &path, &ContainError::from(e)))?;
            let dir = ChargedDirIter::new(walk, &caller.data().tenant_limits)
                .map_err(|e| wasmtime::Error::msg(format!("fs.list {path}: {e}")))?;
            results[0] =
                make_handle_iterator(caller, dir, list_next, close_handle_of::<ChargedDirIter>)?;
            Ok(())
        },
    )?;

    install_getters(linker, &engine, &intr, object.clone())?;
    install_file_writer_methods(linker, &engine, &intr, object)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Extracted op bodies
// ---------------------------------------------------------------------------

/// `info()`: the active VFS configuration as an `$InfoBacking`.
fn build_info(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let info = caller.data().vfs_info.clone();
    let mode = match info.mode {
        crate::runtime::VfsMode::None => "none",
        crate::runtime::VfsMode::Ephemeral => "ephemeral",
        crate::runtime::VfsMode::PerSession => "per_session",
        crate::runtime::VfsMode::Persistent => "persistent",
    };
    // -1 sentinel = "no limit / not applicable" (none + persistent modes);
    // the language has no `undefined`, so the field is always present.
    let size_limit = info.size_limit.map_or(-1.0, |v| v as f64);
    let path_limit = info.path_limit.map_or(-1.0, |v| v as f64);
    let mode = write_submilli_string_struct(caller, mode)?.to_anyref();
    let ty = info_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(mode)),
            Val::F64(size_limit.to_bits()),
            Val::F64(path_limit.to_bits()),
            Val::I64(0),
        ],
    )
}

/// `stat(path)`: a `$StatBacking`, or null when the path does not exist.
fn stat_entry(caller: &mut Caller<'_, StoreData>, path: &str) -> wasmtime::Result<Val> {
    // `stat` is declared `Stat | null` so a program can probe for absence. Resolution now
    // opens the parent, so a missing *ancestor* fails here rather than at the metadata call
    // below — and answering that with a trap would break the contract every caller branches
    // on. Escapes still trap: unreachable-because-outside is not the same as absent.
    let resolved = match resolve_link(&caller.data().vfs, DEFAULT_CWD, path) {
        Ok(resolved) => resolved,
        Err(ContainError::Io(e)) if is_absent(&e) => return Ok(Val::AnyRef(None)),
        Err(e) => return Err(contain_trap("fs.stat", path, &e)),
    };
    let meta = match resolved.symlink_metadata() {
        Ok(m) => m,
        Err(ContainError::Io(e)) if is_absent(&e) => return Ok(Val::AnyRef(None)),
        Err(e) => return Err(contain_trap("fs.stat", path, &e)),
    };
    let ft = meta.file_type();
    let size = if ft.is_file() { meta.len() as f64 } else { 0.0 };
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.into_std().duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_millis() as f64);
    let kind = write_submilli_string_struct(caller, kind_of(&ft))?.to_anyref();
    let ty = stat_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(kind)),
            Val::F64(size.to_bits()),
            Val::F64(modified_ms.to_bits()),
        ],
    )
}

/// `peek(path)`: sniff the first ~256 bytes into a `$PeekBacking`.
fn peek_file(caller: &mut Caller<'_, StoreData>, path: &str) -> wasmtime::Result<Val> {
    let resolved = resolve_content_or_trap(caller.data(), path, "fs.peek")?;
    let meta = resolved
        .metadata()
        .map_err(|e| contain_trap("fs.peek", path, &e))?;
    if !meta.is_file() {
        wasmtime::bail!("fs.peek {}: path is not a regular file", path);
    }
    let mut f = resolved
        .open()
        .map_err(|e| contain_trap("fs.peek", path, &e))?;
    let mut buf = vec![0u8; 256];
    let n = f
        .read(&mut buf)
        .map_err(|e| wasmtime::Error::msg(format!("fs.peek {path}: {e}")))?;
    buf.truncate(n);

    let (encoding, content_start) = detect_encoding(&buf);
    let content = &buf[content_start..];
    let line_ending = if content.windows(2).any(|w| w == b"\r\n") {
        "crlf"
    } else {
        "lf"
    };
    let preview = String::from_utf8_lossy(content).into_owned();

    let preview = write_submilli_string_struct(caller, &preview)?.to_anyref();
    let encoding = write_submilli_string_struct(caller, encoding)?.to_anyref();
    let line_ending = write_submilli_string_struct(caller, line_ending)?.to_anyref();
    let ty = peek_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(preview)),
            Val::AnyRef(Some(encoding)),
            Val::AnyRef(Some(line_ending)),
            Val::F64((meta.len() as f64).to_bits()),
        ],
    )
}

/// BOM-first encoding sniff; falls back to a UTF-8 validity check, then latin1.
/// Returns the detected label and the byte offset where content starts.
fn detect_encoding(buf: &[u8]) -> (&'static str, usize) {
    if buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
        ("utf-8", 3)
    } else if buf.starts_with(&[0xFF, 0xFE]) {
        ("utf-16le", 2)
    } else if buf.starts_with(&[0xFE, 0xFF]) {
        ("utf-16be", 2)
    } else if std::str::from_utf8(buf).is_ok() {
        ("utf-8", 0)
    } else {
        ("latin1", 0)
    }
}

/// The shared `read`/`readText` path: gate, resolve, refuse directories, and
/// load the whole file — or `None` when it exceeds `fs.maxReadSize` (the
/// caller surfaces that as a guest `null`).
fn read_whole_capped(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
    op: &str,
) -> wasmtime::Result<Option<Vec<u8>>> {
    let ctx = format!("fs.{op}");
    check_security(
        &*caller,
        "fs.read",
        serde_json::json!({ "op": op, "path": path }),
    )?;
    let resolved = resolve_content_or_trap(caller.data(), path, &ctx)?;
    let meta = resolved
        .metadata()
        .map_err(|e| contain_trap(&ctx, path, &e))?;
    if meta.is_dir() {
        wasmtime::bail!("{ctx} {}: path is a directory", path);
    }
    if meta.len() > caller.data().fs_max_read_size {
        return Ok(None);
    }
    let bytes = resolved.read().map_err(|e| contain_trap(&ctx, path, &e))?;
    Ok(Some(bytes))
}

/// `readBytes(path, offset, length)`: validated random-access range read; may
/// come back short at EOF.
fn read_byte_range(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
    offset: i64,
    length: i64,
) -> wasmtime::Result<Vec<u8>> {
    if offset < 0 {
        return Err(crate::runtime::host::range_error(
            "fs.readBytes: negative offset",
        ));
    }
    if length < 0 {
        return Err(crate::runtime::host::range_error(
            "fs.readBytes: negative length",
        ));
    }
    if (length as u64) > caller.data().fs_max_read_size {
        return Err(crate::runtime::host::range_error(format!(
            "fs.readBytes: length {length} exceeds fs.maxReadSize"
        )));
    }
    let resolved = resolve_content_or_trap(caller.data(), path, "fs.readBytes")?;
    let mut f = resolved
        .open()
        .map_err(|e| contain_trap("fs.readBytes", path, &e))?;
    f.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| wasmtime::Error::msg(format!("fs.readBytes {path}: seek: {e}")))?;
    let mut buf = vec![0u8; length as usize];
    let mut filled = 0;
    while filled < buf.len() {
        let n = f
            .read(&mut buf[filled..])
            .map_err(|e| wasmtime::Error::msg(format!("fs.readBytes {path}: {e}")))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

/// `writer(path)`: open the temp sibling and wrap the [`ChargedFileWriter`] in
/// a `$FileWriterBacking`.
fn open_writer(caller: &mut Caller<'_, StoreData>, path: &str) -> wasmtime::Result<Val> {
    let resolved = resolve_content_or_trap(caller.data(), path, "fs.writer")?;
    if resolved.is_root() {
        wasmtime::bail!(
            "fs.writer {}: the VFS root is a directory, not a file",
            path
        );
    }
    let tmp = resolved.temp_sibling();
    let file = tmp
        .create_new()
        .map_err(|e| write_target_trap("fs.writer", path, &e))?
        .into_std();
    let writer = ChargedFileWriter::new(file, tmp, resolved, &caller.data().tenant_limits)
        .map_err(|e| wasmtime::Error::msg(format!("fs.writer {path}: {e}")))?;
    let handle = ExternRef::new(&mut *caller, writer)?;
    let ty = file_writer_backing_struct(caller.engine())?;
    abi::new_backing(caller, ty, &[Val::ExternRef(Some(handle))])
}

// ---------------------------------------------------------------------------
// Iterators
// ---------------------------------------------------------------------------

type IterCallback = fn(Caller<'_, StoreData>, &[Val], &mut [Val]) -> wasmtime::Result<()>;

/// Wrap a charged handle in a closable iterator: the handle rides an
/// `externref` inside a host-private env struct; `next_step` / `close_step`
/// are the per-kind callbacks.
fn make_handle_iterator(
    caller: &mut Caller<'_, StoreData>,
    payload: impl Send + Sync + 'static,
    next_step: IterCallback,
    close_step: IterCallback,
) -> wasmtime::Result<Val> {
    let handle = ExternRef::new(&mut *caller, payload)?;
    let env_ty = handle_env_struct(caller.engine())?;
    let pre = StructRefPre::new(&mut *caller, env_ty);
    let env = StructRef::new(&mut *caller, &pre, &[Val::ExternRef(Some(handle))])?;

    let intr = build_intrinsic_types(caller.engine())?;
    let (next_ty, _) = next_closure_type(caller.engine(), &intr)?;
    let (close_ty, _) = void_closure_type(caller.engine(), &intr)?;
    let next_fn = Func::new(&mut *caller, next_ty, next_step);
    let close_fn = Func::new(&mut *caller, close_ty, close_step);
    build_closable_iterator(
        caller,
        next_fn,
        close_fn,
        Val::AnyRef(Some(env.to_anyref())),
    )
}

/// Pull the handle `externref` back out of the iterator env struct.
fn env_handle(
    caller: &mut Caller<'_, StoreData>,
    env: &Val,
) -> wasmtime::Result<Option<Rooted<ExternRef>>> {
    let st = as_struct(caller, env, "fs iterator env")?;
    match st.field(&mut *caller, 0)? {
        Val::ExternRef(handle) => Ok(handle),
        other => Err(wasmtime::Error::msg(format!(
            "fs iterator env: expected externref, got {other:?}"
        ))),
    }
}

/// Borrow the typed handle payload out of the env, erroring with `ctx` when
/// the externref was reclaimed or holds an unexpected payload.
fn handle_payload<'a, T: 'static>(
    caller: &'a mut Caller<'_, StoreData>,
    handle: &Rooted<ExternRef>,
    ctx: &str,
) -> wasmtime::Result<&'a mut T> {
    handle
        .data_mut(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg(format!("{ctx}: handle externref reclaimed")))?
        .downcast_mut::<T>()
        .ok_or_else(|| wasmtime::Error::msg(format!("{ctx}: unexpected externref payload")))
}

fn lines_next(
    mut caller: Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let caller = &mut caller;
    let Some(handle) = env_handle(caller, &params[0])? else {
        results[0] = iter_done(caller)?;
        return Ok(());
    };
    let line = handle_payload::<ChargedLineReader>(caller, &handle, "fs.lines.next")?
        .read_next()
        .map_err(|e| wasmtime::Error::msg(format!("fs.lines.next: {e}")))?;
    results[0] = match line {
        Some(text) => {
            let st = write_submilli_string_struct(caller, &text)?;
            iter_yield(caller, Val::AnyRef(Some(st.to_anyref())))?
        }
        None => iter_done(caller)?,
    };
    Ok(())
}

fn bytes_next(
    mut caller: Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let caller = &mut caller;
    let Some(handle) = env_handle(caller, &params[0])? else {
        results[0] = iter_done(caller)?;
        return Ok(());
    };
    let chunk = handle_payload::<ChargedByteReader>(caller, &handle, "fs.bytes.next")?
        .read_next()
        .map_err(|e| wasmtime::Error::msg(format!("fs.bytes.next: {e}")))?;
    results[0] = match chunk {
        Some(bytes) => {
            let arr = write_submilli_uint8array_struct(caller, &bytes)?;
            iter_yield(caller, Val::AnyRef(Some(arr.to_anyref())))?
        }
        None => iter_done(caller)?,
    };
    Ok(())
}

fn list_next(
    mut caller: Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let caller = &mut caller;
    let Some(handle) = env_handle(caller, &params[0])? else {
        results[0] = iter_done(caller)?;
        return Ok(());
    };
    let entry = handle_payload::<ChargedDirIter>(caller, &handle, "fs.list.next")?.next_entry();
    let Some(entry) = entry else {
        results[0] = iter_done(caller)?;
        return Ok(());
    };
    let size = entry.size as f64;
    let kind = write_submilli_string_struct(caller, entry.kind)?.to_anyref();
    let name = write_submilli_string_struct(caller, &entry.name)?.to_anyref();
    let path = write_submilli_string_struct(caller, &entry.guest_path)?.to_anyref();
    let ty = dir_entry_backing_struct(caller.engine())?;
    let entry = abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(kind)),
            Val::AnyRef(Some(name)),
            Val::AnyRef(Some(path)),
            Val::F64(size.to_bits()),
            Val::I64(0),
        ],
    )?;
    results[0] = iter_yield(caller, entry)?;
    Ok(())
}

/// Eager close: drops the OS handle and refunds bytes now. A reclaimed
/// externref is a no-op — `for…of` fires `close` unconditionally, so a
/// double close must not trap.
fn close_handle_of<T: handles::Closable + 'static>(
    mut caller: Caller<'_, StoreData>,
    params: &[Val],
    _results: &mut [Val],
) -> wasmtime::Result<()> {
    let caller = &mut caller;
    if let Some(handle) = env_handle(caller, &params[0])?
        && let Some(payload) = handle
            .data_mut(&mut *caller)?
            .and_then(|d| d.downcast_mut::<T>())
    {
        payload.close();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Getters and FileWriter methods
// ---------------------------------------------------------------------------

fn install_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    receiver: ValType,
) -> wasmtime::Result<()> {
    let string = || {
        ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.string.clone()),
        ))
    };
    install_field_getters(
        linker,
        MODULE_NAME,
        "Stat",
        engine,
        &receiver,
        &[
            ("kind", STAT_KIND, string()),
            ("size", STAT_SIZE, ValType::F64),
            ("modifiedAt", STAT_MODIFIED_AT, ValType::F64),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "Peek",
        engine,
        &receiver,
        &[
            ("preview", PEEK_PREVIEW, string()),
            ("encoding", PEEK_ENCODING, string()),
            ("lineEnding", PEEK_LINE_ENDING, string()),
            ("size", PEEK_SIZE, ValType::F64),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "DirEntry",
        engine,
        &receiver,
        &[
            ("kind", ENTRY_KIND, string()),
            ("name", ENTRY_NAME, string()),
            ("path", ENTRY_PATH, string()),
            ("size", ENTRY_SIZE, ValType::F64),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "Info",
        engine,
        &receiver,
        &[
            ("mode", INFO_MODE, string()),
            ("sizeLimit", INFO_SIZE_LIMIT, ValType::F64),
            ("pathLimit", INFO_PATH_LIMIT, ValType::F64),
        ],
    )?;
    Ok(())
}

fn install_file_writer_methods(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    receiver: ValType,
) -> wasmtime::Result<()> {
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let uint8 = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.uint8_array.clone()),
    ));
    let writer_key = crate::mangle::package_symbol(MODULE_NAME, "FileWriter");

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "close"),
        FuncType::new(engine, [receiver.clone()], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            writer_payload(caller, &params[0], "fs.writer.close")?
                .close()
                .map_err(|e| wasmtime::Error::msg(format!("fs.writer.close: {e}")))
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "writeLine"),
        FuncType::new(engine, [receiver.clone(), string], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let line = read_string_arg(&mut *caller, &params[1], "fs.writer.writeLine")?;
            writer_payload(caller, &params[0], "fs.writer.writeLine")?
                .write_line(&line)
                .map_err(|e| wasmtime::Error::msg(format!("fs.writer.writeLine: {e}")))
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "writeBytes"),
        FuncType::new(engine, [receiver, uint8], []),
        /* deterministic = */ false,
        |caller, params, _results| {
            let bytes = read_uint8_array_arg(&mut *caller, &params[1], "fs.writer.writeBytes")?;
            writer_payload(caller, &params[0], "fs.writer.writeBytes")?
                .write_bytes(&bytes)
                .map_err(|e| wasmtime::Error::msg(format!("fs.writer.writeBytes: {e}")))
        },
    )?;

    Ok(())
}

/// Borrow the [`ChargedFileWriter`] out of a `$FileWriterBacking` receiver.
fn writer_payload<'a>(
    caller: &'a mut Caller<'_, StoreData>,
    receiver: &Val,
    ctx: &str,
) -> wasmtime::Result<&'a mut ChargedFileWriter> {
    let st = abi::backing_receiver(caller, receiver)?;
    let handle = match st.field(&mut *caller, WRITER_HANDLE)? {
        Val::ExternRef(Some(handle)) => handle,
        Val::ExternRef(None) => {
            return Err(wasmtime::Error::msg(format!(
                "{ctx}: writer handle is null"
            )));
        }
        other => {
            return Err(wasmtime::Error::msg(format!(
                "{ctx}: expected externref handle, got {other:?}"
            )));
        }
    };
    handle_payload::<ChargedFileWriter>(caller, &handle, ctx)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Reproduce a symlink at `to` with `from`'s target stored verbatim.
///
/// Verbatim is the point: the target is never resolved, so a link that escapes the root is
/// reproduced as one that still refuses rather than as a working door out of it.
fn copy_link(from: &LinkPath, to: &LinkPath) -> Result<(), ContainError> {
    to.symlink(&from.read_link_contents()?, from.link_kind()?)
}

/// Whether an I/O error means "this path is not there", for the two operations declared
/// to answer that without trapping. `NotADirectory` counts: a file used as a path
/// component is a path that cannot exist, not an error to propagate.
fn is_absent(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// The guest-relative prefix a listing's entries carry: `""` at the root, else `"tree/"`.
fn list_prefix(path: &str) -> wasmtime::Result<String> {
    let normalized = guest_normalize(crate::stdlib::shared::DEFAULT_CWD, path)
        .map_err(|e| contain_trap("fs.list", path, &ContainError::from(e)))?;
    Ok(if normalized == "/" {
        String::new()
    } else {
        format!("{}/", &normalized[1..])
    })
}

fn f64_arg(val: &Val, ctx: &str) -> wasmtime::Result<f64> {
    match val {
        Val::F64(bits) => Ok(f64::from_bits(*bits)),
        other => Err(crate::runtime::host::type_error(format!(
            "{ctx}: expected f64, got {other:?}"
        ))),
    }
}

fn i32_flag(val: &Val, ctx: &str) -> wasmtime::Result<bool> {
    match val {
        Val::I32(v) => Ok(*v != 0),
        other => Err(crate::runtime::host::type_error(format!(
            "{ctx}: expected i32 flag, got {other:?}"
        ))),
    }
}

/// No `parent.is_dir()` pre-check: `Dir` reports a missing parent as `NotFound` the way
/// `std` does, and the old gate turned an escape into "parent directory does not exist" —
/// costing an LLM a turn on a `mkdir` that could never succeed.
fn atomic_write(
    final_path: &ContentPath,
    bytes: &[u8],
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<()> {
    let tmp = final_path.temp_sibling();
    {
        let mut f = tmp
            .create_new()
            .map_err(|e| write_target_trap(op, guest_path, &e))?;
        f.write_all(bytes)
            .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: {e}")))?;
        f.sync_all()
            .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: fsync: {e}")))?;
    }
    tmp.rename_to(final_path).map_err(|e| {
        let _ = tmp.remove_file();
        contain_trap(op, guest_path, &e)
    })?;
    Ok(())
}

fn append_bytes(
    final_path: &ContentPath,
    bytes: &[u8],
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<()> {
    let mut f = final_path
        .append()
        .map_err(|e| write_target_trap(op, guest_path, &e))?;
    f.write_all(bytes)
        .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: {e}")))?;
    Ok(())
}

/// Preserves symlinks as links rather than dereferencing — prevents exfiltrating targets
/// outside the VFS. `cap_std::fs::Dir::copy` follows the source link and would write a
/// regular file, so the walk stays manual.
fn copy_recursive(
    from: &LinkPath,
    to: &LinkPath,
    recursive: bool,
    guest_from: &str,
    guest_to: &str,
) -> wasmtime::Result<()> {
    let pair = format!("{guest_from} -> {guest_to}");
    let meta = from
        .symlink_metadata()
        .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        copy_link(from, to).map_err(|e| contain_trap("fs.copy", &pair, &e))?;
    } else if ft.is_dir() {
        if !recursive {
            wasmtime::bail!(
                "fs.copy {} -> {}: source is a directory (use {{ recursive: true }})",
                guest_from,
                guest_to
            );
        }
        to.create_dir_all()
            .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
        let from_dir = Arc::new(
            from.open_dir()
                .map_err(|e| contain_trap("fs.copy", &pair, &e))?,
        );
        let to_dir = Arc::new(
            to.open_dir()
                .map_err(|e| contain_trap("fs.copy", &pair, &e))?,
        );
        let entries = from_dir
            .entries()
            .map_err(|e| contain_trap("fs.copy", &pair, &ContainError::from(e)))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| contain_trap("fs.copy", &pair, &ContainError::from(e)))?;
            let name = entry.file_name();
            let child_from = LinkPath::new(Arc::clone(&from_dir), name.clone());
            let child_to = LinkPath::new(Arc::clone(&to_dir), name);
            if entry.file_type().is_ok_and(|ft| ft.is_symlink()) {
                // A `.venv/bin/python -> /usr/bin/python3` is one the platform may refuse to
                // reproduce, and that must not fail a whole checkout copy. Scoped to the link
                // reproduction itself: recursing and swallowing every error would report
                // success for a tree that silently lost entries to a full disk or a
                // read-only destination.
                let _ = copy_link(&child_from, &child_to);
                continue;
            }
            copy_recursive(&child_from, &child_to, true, guest_from, guest_to)?;
        }
    } else {
        from.copy_to(to)
            .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::compile_script;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, VfsInfo, VfsMode, dispatch_main_async, install_runtime_async,
    };

    struct DenyAllFs;
    impl SecurityCheck for DenyAllFs {
        fn check(
            &self,
            _caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            if capability.starts_with("fs.") {
                CheckOutcome::Deny {
                    reason: format!("denied {capability} in test"),
                }
            } else {
                CheckOutcome::Allow
            }
        }
    }

    #[tokio::test]
    async fn narrowed_host_carriers() {
        let source = r#"
import { info, stat, peek, list, writer, writeText, Info, Stat, Peek, DirEntry, FileWriter } from "submilli:fs";

class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
function rejects(read: () => void): void {
 let caught = false;
 try { read(); } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "unrelated carrier must throw TypeError");
}

class InfoField extends Parent { value: Info = info(); }
class StatField extends Parent { value: Stat | null = null; }
class PeekField extends Parent { value: Peek | null = null; }
class EntryField extends Parent { value: DirEntry | null = null; }
class WriterField extends Parent { value: FileWriter | null = null; }
function main(): void {
 writeText("/input.txt", "text");
 const i = new InfoField(); assert(i.value.mode.length > 0, "Info");
 const s = new StatField(); s.reset(stat("/input.txt")); assert(s.value!.size === 4, "Stat");
 i.reset(s.value); rejects(() => { const v = i.value; });
 s.reset(info()); rejects(() => { const v = s.value; });
 const p = new PeekField(); p.reset(peek("/input.txt")); assert(p.value!.size === 4, "Peek");
 const e = new EntryField();
 for (const entry of list("/", false)) { e.reset(entry); }
 assert(e.value!.name === "input.txt", "DirEntry");
 p.reset(e.value); rejects(() => { const v = p.value; });
 e.reset(peek("/input.txt")); rejects(() => { const v = e.value; });
 const w = new WriterField(); w.reset(writer("/output.txt")); w.value!.close();
 w.reset(info()); rejects(() => { const v = w.value; });
}
"#;
        run_with(source, StoreData::with_vfs(Vfs::tempdir().unwrap()))
            .await
            .expect("host guards");
    }

    #[tokio::test]
    async fn lines_iterator_closes_handle_on_loop_exit() {
        let source = r#"
            import { writeText, lines } from "submilli:fs";
            function main(): void {
                writeText("/lines.txt", "a\nb\nc\n");
                let count: number = 0;
                for (const line of lines("/lines.txt")) {
                    count = count + 1;
                }
                assert(count === 3, "iterated all 3 lines");
            }
        "#;
        run_and_assert_handles_empty(source).await;
    }

    #[tokio::test]
    async fn lines_iterator_closes_handle_on_break() {
        let source = r#"
            import { writeText, lines } from "submilli:fs";
            function main(): void {
                writeText("/lines.txt", "a\nb\nc\nd\ne\n");
                let count: number = 0;
                for (const line of lines("/lines.txt")) {
                    count = count + 1;
                    if (count === 2) {
                        break;
                    }
                }
                assert(count === 2, "broke after 2 lines");
            }
        "#;
        run_and_assert_handles_empty(source).await;
    }

    #[tokio::test]
    async fn bytes_iterator_closes_handle_on_break() {
        let source = r#"
            import { write, bytes } from "submilli:fs";
            function main(): void {
                const payload: Uint8Array = new Uint8Array([0, 1, 2, 3, 4, 5, 6, 7]);
                write("/blob.bin", payload);
                let chunks: number = 0;
                for (const chunk of bytes("/blob.bin", 2)) {
                    chunks = chunks + 1;
                    if (chunks === 2) {
                        break;
                    }
                }
                assert(chunks === 2, "broke after 2 chunks");
            }
        "#;
        run_and_assert_handles_empty(source).await;
    }

    #[tokio::test]
    async fn list_iterator_closes_handle_on_break() {
        let source = r#"
            import { mkdir, writeText, list, DirEntry } from "submilli:fs";
            function main(): void {
                mkdir("/d", false);
                writeText("/d/a.txt", "a");
                writeText("/d/b.txt", "b");
                writeText("/d/c.txt", "c");
                let count: number = 0;
                for (const entry of list("/d", false)) {
                    count = count + 1;
                    if (count === 1) {
                        break;
                    }
                }
                assert(count === 1, "broke after first entry");
            }
        "#;
        run_and_assert_handles_empty(source).await;
    }

    #[tokio::test]
    async fn lines_iterator_closes_handle_on_throw() {
        let source = r#"
            import { writeText, lines } from "submilli:fs";
            function main(): void {
                writeText("/lines.txt", "alpha\nbeta\ngamma\n");
                let caught: boolean = false;
                try {
                    for (const line of lines("/lines.txt")) {
                        if (line === "beta") {
                            throw new Error("unwind from loop");
                        }
                    }
                } catch (e: Error) {
                    caught = true;
                }
                assert(caught, "outer catch saw the throw");
            }
        "#;
        run_and_assert_handles_empty(source).await;
    }

    /// The for-of `finally` closes every iterator, and eager `close()` refunds the
    /// reader's charged bytes immediately. So after a clean run the store's
    /// host-attached counter must be back to zero — the externref-era equivalent of
    /// "the handle map is empty".
    async fn run_and_assert_handles_empty(source: &str) {
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran without trap");
        let live = store.data().tenant_limits.host_attached_bytes();
        assert_eq!(
            live, 0,
            "host-attached bytes should be refunded after for-of finally — got {live}",
        );
    }

    struct RecordingCheck {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }
    impl SecurityCheck for RecordingCheck {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            self.seen
                .lock()
                .unwrap()
                .push((caller.to_string(), capability.to_string()));
            CheckOutcome::Allow
        }
    }

    struct ReadContextCheck {
        seen: std::sync::Mutex<Vec<serde_json::Value>>,
    }
    impl SecurityCheck for ReadContextCheck {
        fn check(
            &self,
            _caller: &str,
            capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            if capability == "fs.read" {
                self.seen.lock().unwrap().push(context.clone());
            }
            CheckOutcome::Allow
        }
    }

    #[tokio::test]
    async fn read_bytes_capability_omits_offset() {
        let source = r#"
            import { writeText, readBytes } from "submilli:fs";
            function main(): void {
                writeText("/x.txt", "abcdef");
                const bytes = readBytes("/x.txt", 2, 3);
                assert(bytes.length === 3, "read requested range");
            }
        "#;
        let recording = Arc::new(ReadContextCheck {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        run_with(source, data).await.expect("main ran");

        let seen = recording.seen.lock().unwrap();
        let context = seen
            .iter()
            .find(|context| context["op"] == "readBytes")
            .expect("readBytes capability context");
        assert_eq!(context["path"], "/x.txt");
        assert_eq!(context["length"], 3);
        assert!(
            context.get("offset").is_none(),
            "read offset is transport mechanics, not capability context: {context}"
        );
    }

    #[tokio::test]
    async fn a_script_is_attributed_to_main() {
        let source = r#"
            import { writeText, readText } from "submilli:fs";
            function main(): void {
                writeText("/x.txt", "hi");
                const _back = readText("/x.txt");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran");

        let seen = recording.seen.lock().unwrap().clone();
        assert!(
            !seen.is_empty(),
            "RecordingCheck should have captured at least one fs.* call"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "main",
                "expected caller=main for capability {capability}; got {caller}"
            );
        }
    }

    /// Simulates a library-above-main call to verify entry-below-top caller semantics.
    #[tokio::test]
    async fn a_package_is_attributed_to_the_package() {
        let source = r#"
            import { writeText, readText } from "submilli:fs";
            function main(): void {
                writeText("/x.txt", "hi");
                const _back = readText("/x.txt");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        // Compiled under the package name so the identity rides the wasm frame the runtime
        // reads, rather than being declared out-of-band.
        let compiled = crate::compile::compile_script_owned_by(
            "submilli:url",
            source,
            "test.subm",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran");

        let seen = recording.seen.lock().unwrap().clone();
        assert!(
            !seen.is_empty(),
            "RecordingCheck should have captured calls"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "submilli:url",
                "expected caller=submilli:url for capability {capability}; got {caller}"
            );
        }
    }

    #[tokio::test]
    async fn deny_policy_throws_catchable_permission_denied() {
        let source = r#"
            import { writeText } from "submilli:fs";
            function main(): string {
                try {
                    writeText("/x.txt", "hi");
                } catch (e: PermissionDeniedError) {
                    return e.capability + "|" + e.caller + "|" + e.reason;
                }
                return "not denied";
            }
        "#;
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = Arc::new(DenyAllFs);
        let value = run_with(source, data)
            .await
            .expect("denial is caught in user code — main must not trap");
        assert_eq!(
            value.as_deref(),
            Some("fs.write|main|denied fs.write in test")
        );
    }

    /// Run `source` with the supplied `StoreData`, returning the dispatch result
    /// so callers can assert success (asserts held) or a trap.
    async fn run_with(source: &str, data: StoreData) -> wasmtime::Result<Option<String>> {
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst).await
    }

    #[tokio::test]
    async fn none_mode_disables_fs_calls() {
        let source = r#"
            import { writeText } from "submilli:fs";
            function main(): void {
                writeText("/x.txt", "hi");
            }
        "#;
        let data = StoreData::with_vfs(Vfs::none());
        let err = run_with(source, data)
            .await
            .expect_err("none mode must trap fs calls");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("filesystem is disabled"),
            "expected disabled-fs trap; got: {msg}"
        );
    }

    #[tokio::test]
    async fn info_reports_mode_and_limits() {
        let source = r#"
            import { info, Info } from "submilli:fs";
            function main(): void {
                const fs = info();
                assert(fs.mode === "per_session", "mode surfaced");
                assert(fs.sizeLimit === 1048576, "size limit surfaced");
                assert(fs.pathLimit === 42, "path limit surfaced");
            }
        "#;
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.vfs_info = VfsInfo {
            mode: VfsMode::PerSession,
            size_limit: Some(1024 * 1024),
            path_limit: Some(42),
        };
        run_with(source, data).await.expect("info asserts hold");
    }
}
