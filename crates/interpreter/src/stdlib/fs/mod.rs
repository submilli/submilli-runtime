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

use std::collections::HashSet;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use wasmtime::{
    Caller, ExternRef, FieldType, Finality, Func, FuncType, HeapType, Linker, Mutability, RefType,
    Rooted, StorageType, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::fs::{
    ContainError, ContentPath, FileIdentity, LinkPath, MAX_REMOVE_ENTRIES, guest_normalize,
    resolve_link,
};
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::{
    range_error, read_string_arg, read_uint8_array_arg, register_host_fn,
    write_submilli_string_struct, write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::prelude::iterator::{
    as_struct, build_closable_iterator, iter_done, iter_yield, next_closure_type, void_closure_type,
};
use crate::runtime::{DiskQuota, Holder, OpenFileGuard, QuotaCharge, StoreData, regular_files};
use crate::stdlib::abi::{
    self, backing_struct, externref_field, f64_field, install_field_getters, string_field,
};
use crate::stdlib::shared::{
    DEFAULT_CWD, atomic_write, check_security, contain_trap, quota_refusal, refuse_volume_root,
    require_writable, resolve_content_or_trap, resolve_link_or_trap, write_target_trap,
};
use handles::{
    ChargedByteReader, ChargedDirIter, ChargedFileWriter, ChargedLineReader, ContainedWalk,
    TempFile, WriteError, kind_of,
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
const INFO_ACCESS: usize = 3;
const INFO_VOLUME: usize = 4;
const INFO_MOUNTS: usize = 5;

// `$MountInfoBacking` field indices.
const MOUNT_PATH: usize = 1;
const MOUNT_MODE: usize = 2;
const MOUNT_VOLUME: usize = 3;
const MOUNT_ACCESS: usize = 4;
const MOUNT_SIZE_LIMIT: usize = 5;

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
            string_field(&intr),     // mode
            f64_field(),             // sizeLimit
            string_field(&intr),     // access
            string_field(&intr),     // volume
            abi::array_field(&intr), // mounts
            wasmtime::FieldType::new(Mutability::Const, StorageType::ValType(ValType::I64)), // nominal marker
        ],
    )
}

fn mount_info_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr), // path
            string_field(&intr), // mode
            string_field(&intr), // volume
            string_field(&intr), // access
            f64_field(),         // sizeLimit
            wasmtime::FieldType::new(Mutability::Const, StorageType::ValType(ValType::I32)), // nominal marker
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
            check_security(&*caller, "fs.stat", serde_json::json!({ "path": &path }))?;
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
            check_security(&*caller, "fs.stat", serde_json::json!({ "path": &path }))?;
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
            check_security(&*caller, "fs.stat", serde_json::json!({ "path": &path }))?;
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
            check_security(&*caller, "fs.stat", serde_json::json!({ "path": &path }))?;
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
                        "path": &path,
                        "length": bytes.len(),
                    }),
                )?;
                let resolved = resolve_content_or_trap(caller.data(), &path, &ctx)?;
                require_writable(&*caller, resolved.placement(), "fs.write", &path)?;
                refuse_volume_root(&resolved, &ctx, &path)?;
                let quota = resolved.placement().quota().cloned();
                if atomic {
                    atomic_write(&resolved, &bytes, None, &path, &ctx, quota)
                } else {
                    append_bytes(&resolved, &bytes, &path, &ctx, quota)
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
                serde_json::json!({ "path": &path, "recursive": recursive }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.mkdir")?;
            if !(recursive && resolved.is_existing_dir()) {
                require_writable(&*caller, resolved.placement(), "fs.mkdir", &path)?;
            }
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
                serde_json::json!({ "path": &path, "recursive": recursive }),
            )?;
            let resolved = resolve_link_or_trap(caller.data(), &path, "fs.remove")?;
            require_writable(&*caller, resolved.placement(), "fs.remove", &path)?;
            // `remove_dir_all(".")` drains the root and only then fails on the self-unlink,
            // so without this the guest destroys the operator's volume and is told the call
            // failed. Refuse before touching anything. A mount point is the root of its
            // volume and is refused the same way.
            if resolved.is_root() {
                wasmtime::bail!(
                    "fs.remove {}: cannot remove the VFS root or a mount point itself; \
                     remove its entries instead",
                    path
                );
            }
            let meta = resolved
                .symlink_metadata()
                .map_err(|e| contain_trap("fs.remove", &path, &e))?;
            // `is_dir` on link metadata is false for a symlink to a directory, so an
            // escaping link is unlinked rather than followed and recursively deleted.
            remove_releasing(
                &resolved,
                meta.is_dir(),
                recursive,
                resolved.placement().quota(),
            )
            .map_err(|e| contain_trap("fs.remove", &path, &e))
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
                serde_json::json!({ "from": &from, "to": &to }),
            )?;
            let from_resolved = resolve_link_or_trap(caller.data(), &from, "fs.move")?;
            let to_resolved = resolve_link_or_trap(caller.data(), &to, "fs.move")?;
            require_writable(&*caller, from_resolved.placement(), "fs.move", &from)?;
            require_writable(&*caller, to_resolved.placement(), "fs.move", &to)?;
            let pair = format!("{from} -> {to}");
            if from_resolved.is_root() || to_resolved.is_root() {
                wasmtime::bail!(
                    "fs.move {pair}: the VFS root and mount points cannot be moved or replaced"
                );
            }
            if !from_resolved
                .placement()
                .same_volume(to_resolved.placement())
            {
                return move_across(&from_resolved, &to_resolved, &pair);
            }
            // A file moved over another frees the one it replaced; a move onto the same
            // file, under any spelling, frees nothing.
            let moved = from_resolved.regular_file().map(|(file, _)| file);
            let replaced = to_resolved
                .regular_file()
                .filter(|(file, _)| Some(*file) != moved);
            from_resolved
                .rename_to(&to_resolved)
                .map_err(|e| contain_trap("fs.move", &pair, &e))?;
            if let (Some(quota), Some((file, bytes))) = (to_resolved.placement().quota(), replaced)
            {
                quota.release_file(file, bytes);
            }
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
                    "from": &from,
                    "to": &to,
                    "recursive": recursive,
                }),
            )?;
            let from_resolved = resolve_link_or_trap(caller.data(), &from, "fs.copy")?;
            let pair = format!("{from} -> {to}");
            let mount = from_resolved
                .contains_mount_point()
                .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
            if let Some(mount) = mount {
                wasmtime::bail!(
                    "fs.copy {pair}: {from} contains the mount point {mount}; copy the \
                     mount's contents separately"
                );
            }
            // The content path routes to the same volume as the link path, and resolves
            // even when the destination's parent does not exist yet.
            let to_volume = resolve_content_or_trap(caller.data(), &to, "fs.copy")?;
            require_writable(&*caller, to_volume.placement(), "fs.copy", &to)?;
            // A recursive copy used to reach `create_dir_all(to)`, which built the whole
            // destination chain. Resolving the destination now opens its parent, so that
            // chain has to exist first or a working call starts failing.
            if recursive
                && from_resolved
                    .symlink_metadata()
                    .is_ok_and(|meta| meta.is_dir())
            {
                to_volume
                    .create_dir_all()
                    .map_err(|e| write_target_trap("fs.copy", &to, &e))?;
            }
            let to_resolved = resolve_link_or_trap(caller.data(), &to, "fs.copy")?;
            let quota = to_resolved.placement().quota().cloned();
            copy_recursive(
                &from_resolved,
                &to_resolved,
                &CopyRun {
                    recursive,
                    links: LinkCopies::BestEffort,
                    op: "fs.copy",
                    pair: &pair,
                    quota: quota.as_ref(),
                },
            )
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
            check_security(&*caller, "fs.write", serde_json::json!({ "path": &path }))?;
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
            check_security(&*caller, "fs.read", serde_json::json!({ "path": &path }))?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.lines")?;
            let file = resolved
                .open()
                .map_err(|e| contain_trap("fs.lines", &path, &e))?;
            let held = hold_for_reading(resolved.placement().quota(), &file);
            let reader = ChargedLineReader::new(
                BufReader::new(file.into_std()),
                &caller.data().tenant_limits,
                held,
            )
            .map_err(|e| wasmtime::Error::new(e).context(format!("fs.lines {path}")))?;
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
                serde_json::json!({ "path": &path, "chunkSize": chunk_size }),
            )?;
            let resolved = resolve_content_or_trap(caller.data(), &path, "fs.bytes")?;
            let file = resolved
                .open()
                .map_err(|e| contain_trap("fs.bytes", &path, &e))?;
            let held = hold_for_reading(resolved.placement().quota(), &file);
            let reader = ChargedByteReader::new(
                file.into_std(),
                chunk_size as usize,
                &caller.data().tenant_limits,
                held,
            )
            .map_err(|e| wasmtime::Error::new(e).context(format!("fs.bytes {path}")))?;
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
                serde_json::json!({ "path": &path, "recursive": recursive }),
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
            let mounts = if recursive {
                resolved.mounts_below()
            } else {
                Vec::new()
            };
            let walk = ContainedWalk::new(base, list_prefix(&path)?, recursive, mounts)
                .map_err(|e| contain_trap("fs.list", &path, &ContainError::from(e)))?;
            let dir = ChargedDirIter::new(walk, &caller.data().tenant_limits)
                .map_err(|e| wasmtime::Error::new(e).context(format!("fs.list {path}")))?;
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
        crate::runtime::VfsMode::Named => "named",
    };
    let vfs = &caller.data().vfs;
    let root_limit = vfs.quota().map(|quota| quota.limit());
    let access = vfs.access().as_str();
    let volume = vfs.volume().unwrap_or_default().to_string();
    // Cloned out of the store, which the string writes below borrow mutably.
    let mounts = vfs.mounts().to_vec();
    let mount_ty = mount_info_backing_struct(caller.engine())?;
    let mut built = Vec::with_capacity(mounts.len());
    for mount in &mounts {
        built.push(build_mount_info(caller, &mount_ty, mount)?);
    }
    let mounts = abi::new_array(caller, &built)?;
    // A named root's limit is the volume's, which the server shares in rather
    // than writes into the info.
    let size_limit = size_limit_number(info.size_limit.or(root_limit));
    let mode = write_submilli_string_struct(caller, mode)?.to_anyref();
    let access = write_submilli_string_struct(caller, access)?.to_anyref();
    let volume = write_submilli_string_struct(caller, &volume)?.to_anyref();
    let ty = info_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(mode)),
            Val::F64(size_limit.to_bits()),
            Val::AnyRef(Some(access)),
            Val::AnyRef(Some(volume)),
            mounts,
            Val::I64(0),
        ],
    )
}

/// One `info().mounts` entry as a `$MountInfoBacking`, fields in `MOUNT_*` order.
fn build_mount_info(
    caller: &mut Caller<'_, StoreData>,
    ty: &StructType,
    mount: &crate::runtime::vfs::Mount,
) -> wasmtime::Result<Val> {
    let path = write_submilli_string_struct(caller, mount.guest_path())?.to_anyref();
    let mode = write_submilli_string_struct(caller, "named")?.to_anyref();
    let volume = write_submilli_string_struct(caller, mount.volume())?.to_anyref();
    let access = write_submilli_string_struct(caller, mount.access().as_str())?.to_anyref();
    let size_limit = size_limit_number(mount.quota().map(|quota| quota.limit()));
    abi::new_backing(
        caller,
        ty.clone(),
        &[
            Val::AnyRef(Some(path)),
            Val::AnyRef(Some(mode)),
            Val::AnyRef(Some(volume)),
            Val::AnyRef(Some(access)),
            Val::F64(size_limit.to_bits()),
            Val::I32(0),
        ],
    )
}

/// `-1` stands for "no limit": the language has no `undefined`, so the field
/// is always present.
fn size_limit_number(limit: Option<u64>) -> f64 {
    limit.map_or(-1.0, |v| v as f64)
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
    check_security(&*caller, "fs.read", serde_json::json!({ "path": path }))?;
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
    require_writable(&*caller, resolved.placement(), "fs.write", path)?;
    refuse_volume_root(&resolved, "fs.writer", path)?;
    let tmp = resolved.temp_sibling();
    let file = tmp
        .create_new()
        .map_err(|e| write_target_trap("fs.writer", path, &e))?;
    let temp = TempFile::created(tmp, &file)
        .map_err(|e| wasmtime::Error::msg(format!("fs.writer {path}: {e}")))?;
    let quota = resolved.placement().quota().cloned();
    let writer = ChargedFileWriter::new(
        file.into_std(),
        temp,
        resolved,
        &caller.data().tenant_limits,
        quota,
    )
    .map_err(|e| wasmtime::Error::new(e).context(format!("fs.writer {path}")))?;
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
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));
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
            ("access", INFO_ACCESS, string()),
            ("volume", INFO_VOLUME, string()),
            ("mounts", INFO_MOUNTS, array),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "MountInfo",
        engine,
        &receiver,
        &[
            ("path", MOUNT_PATH, string()),
            ("mode", MOUNT_MODE, string()),
            ("volume", MOUNT_VOLUME, string()),
            ("access", MOUNT_ACCESS, string()),
            ("sizeLimit", MOUNT_SIZE_LIMIT, ValType::F64),
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
                .map_err(|e| write_error("fs.writer.close", e))
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
                .map_err(|e| write_error("fs.writer.writeLine", e))
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
                .map_err(|e| write_error("fs.writer.writeBytes", e))
        },
    )?;

    Ok(())
}

/// A size-limit refusal is a `RangeError` the program can catch, like every other
/// write that would pass it.
fn write_error(op: &str, err: WriteError) -> wasmtime::Error {
    match err {
        WriteError::Full(exceeded) => range_error(format!("{op}: {exceeded}")),
        WriteError::Io(e) => wasmtime::Error::msg(format!("{op}: {e}")),
        WriteError::Contain(e) => wasmtime::Error::msg(format!("{op}: {e}")),
    }
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

fn append_bytes(
    final_path: &ContentPath,
    bytes: &[u8],
    guest_path: &str,
    op: &str,
    quota: Option<Arc<DiskQuota>>,
) -> wasmtime::Result<()> {
    let appended = bytes.len() as u64;
    let before = final_path.file_len();
    let mut disk_charge = QuotaCharge::new(quota, None);
    disk_charge
        .reserve(appended)
        .map_err(|exceeded| quota_refusal(op, guest_path, exceeded))?;
    let result = final_path
        .append()
        .map_err(|e| write_target_trap(op, guest_path, &e))
        .and_then(|mut f| {
            f.write_all(bytes)
                .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: {e}")))
        });
    if result.is_err() {
        // A failed append may still have landed part of its bytes; keep those counted.
        let landed = final_path.file_len().saturating_sub(before).min(appended);
        disk_charge.unreserve(appended.saturating_sub(landed));
        disk_charge.keep();
        return result;
    }
    disk_charge.commit();
    result
}

/// The regular files a removal frees: the file itself, or every file under a
/// directory removed recursively. A tree that can't be walked frees nothing on the
/// count, which errs toward refusing a later write rather than allowing one past
/// the limit. The walk stops where the removal's own scan would refuse, so a tree
/// too large to remove isn't walked in full first.
fn files_freed_by_remove(
    target: &LinkPath,
    is_dir: bool,
    recursive: bool,
) -> Vec<(FileIdentity, u64)> {
    if !is_dir {
        return target.regular_file().into_iter().collect();
    }
    if !recursive {
        return Vec::new();
    }
    target
        .open_dir()
        .ok()
        .and_then(|dir| regular_files(&dir, MAX_REMOVE_ENTRIES).ok())
        .unwrap_or_default()
}

/// The regular files still under `target` after a removal stopped partway, or
/// `None` when that can't be told.
fn files_left_after(
    target: &LinkPath,
    is_dir: bool,
    recursive: bool,
) -> Option<HashSet<FileIdentity>> {
    if !is_dir {
        return Some(
            target
                .regular_file()
                .map(|(file, _)| file)
                .into_iter()
                .collect(),
        );
    }
    if !recursive {
        // A directory removed on its own frees no regular file.
        return Some(HashSet::new());
    }
    let dir = match target.open_dir() {
        Ok(dir) => dir,
        Err(ContainError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(HashSet::new());
        }
        Err(_) => return None,
    };
    let files = regular_files(&dir, MAX_REMOVE_ENTRIES).ok()?;
    Some(files.into_iter().map(|(file, _)| file).collect())
}

/// Register a file a reader opens, so a removal of its name keeps its bytes
/// counted until the reader closes.
fn hold_for_reading(
    quota: Option<&Arc<DiskQuota>>,
    file: &cap_std::fs::File,
) -> Option<OpenFileGuard> {
    let quota = quota?;
    let metadata = file.metadata().ok()?;
    Some(quota.hold(FileIdentity::of(&metadata).ok()?, Holder::Reader))
}

/// A move between two volumes, which no rename can make: copy into a temporary
/// sibling of the destination (charged to the destination's limit), rename it into
/// place, then remove the source (released from the source's limit).
///
/// Until the rename, a failure removes the temporary copy and leaves the source
/// untouched. Once the destination is in place it is kept; if the source then
/// cannot be removed, the call fails saying both copies may exist. A link the copy
/// cannot reproduce fails the move, since the source is removed after.
fn move_across(from: &LinkPath, to: &LinkPath, pair: &str) -> wasmtime::Result<()> {
    let meta = from
        .symlink_metadata()
        .map_err(|e| contain_trap("fs.move", pair, &e))?;
    let is_dir = meta.is_dir();
    // The source is removed last, so a tree too deep or too large to remove, or
    // one holding a mount point, is refused now, before a copy of it lands in the
    // destination. The destination gets the checks its rename will run, so a
    // move that must fail does no copying first.
    from.check_removable()
        .map_err(|e| contain_trap("fs.move", pair, &e))?;
    to.check_removable()
        .map_err(|e| contain_trap("fs.move", pair, &e))?;
    refuse_unfitting_destination(to, is_dir, pair)?;
    let to_quota = to.placement().quota().cloned();
    let staged = to.temp_sibling();
    let run = CopyRun {
        recursive: true,
        links: LinkCopies::Required,
        op: "fs.move",
        pair,
        quota: to_quota.as_ref(),
    };
    let copied = copy_recursive(from, &staged, &run).and_then(|()| {
        let replaced = to.regular_file();
        staged
            .rename_to(to)
            .map_err(|e| contain_trap("fs.move", pair, &e))?;
        if let (Some(quota), Some((file, bytes))) = (to_quota.as_ref(), replaced) {
            quota.release_file(file, bytes);
        }
        Ok(())
    });
    if let Err(err) = copied {
        // Best effort: what cannot be removed stays charged, so the count errs
        // toward refusing a later write.
        if let Ok(meta) = staged.symlink_metadata() {
            let _ = remove_releasing(&staged, meta.is_dir(), true, to_quota.as_ref());
        }
        return Err(err);
    }
    remove_releasing(from, is_dir, true, from.placement().quota()).map_err(|e| {
        wasmtime::Error::msg(format!(
            "fs.move {pair}: moved to the destination but could not remove the source, \
             so both may now exist: {e}"
        ))
    })
}

/// Refuse a destination a rename of the source could not replace — a directory
/// in place of a file, a file in place of a directory, or a directory that isn't
/// empty — as a rename within one volume would, before any copying.
fn refuse_unfitting_destination(
    to: &LinkPath,
    moving_dir: bool,
    pair: &str,
) -> wasmtime::Result<()> {
    let existing = match to.symlink_metadata() {
        Ok(existing) => existing,
        Err(ContainError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(err) => return Err(contain_trap("fs.move", pair, &err)),
    };
    let replacing_dir = existing.is_dir();
    if replacing_dir != moving_dir {
        let (what, onto) = if moving_dir {
            ("a directory", "something that is not a directory")
        } else {
            ("a file", "a directory")
        };
        wasmtime::bail!("fs.move {pair}: cannot move {what} onto {onto}");
    }
    if !replacing_dir {
        return Ok(());
    }
    let has_entries = to
        .entries()
        .map_err(|e| contain_trap("fs.move", pair, &e))?
        .next()
        .is_some();
    if has_entries {
        wasmtime::bail!("fs.move {pair}: the destination directory is not empty");
    }
    Ok(())
}

/// Remove `target` and release from `quota` every file that is gone afterwards:
/// all of them on success, and only those actually removed when the removal stops
/// partway, so the count neither keeps a file that is gone nor frees one that
/// is still there.
fn remove_releasing(
    target: &LinkPath,
    is_dir: bool,
    recursive: bool,
    quota: Option<&Arc<DiskQuota>>,
) -> Result<(), ContainError> {
    let freed = match quota {
        Some(_) => files_freed_by_remove(target, is_dir, recursive),
        None => Vec::new(),
    };
    let removed = match (is_dir, recursive) {
        (true, true) => target.remove_dir_all(),
        (true, false) => target.remove_dir(),
        (false, _) => target.remove_file(),
    };
    let Some(quota) = quota else {
        return removed;
    };
    let left: HashSet<FileIdentity> = match removed {
        Ok(()) => HashSet::new(),
        Err(_) => match files_left_after(target, is_dir, recursive) {
            Some(left) => left,
            // What is left can't be told, so nothing is released: a count kept
            // too high refuses a later write rather than allowing one past it.
            None => return removed,
        },
    };
    for (file, bytes) in freed {
        if !left.contains(&file) {
            quota.release_file(file, bytes);
        }
    }
    removed
}

/// How one copy walk treats its tree.
#[derive(Clone, Copy)]
struct CopyRun<'a> {
    recursive: bool,
    links: LinkCopies,
    /// `fs.copy` or `fs.move`, for diagnostics.
    op: &'a str,
    /// `from -> to` as the program wrote them, for diagnostics.
    pair: &'a str,
    /// The destination volume's limit.
    quota: Option<&'a Arc<DiskQuota>>,
}

/// What a copy does with a link inside the tree that the platform will not
/// reproduce, such as an absolute one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkCopies {
    /// Skip it: a copy leaves the source in place, so nothing is lost.
    BestEffort,
    /// Fail: a move removes the source afterwards, which would lose the link.
    Required,
}

/// Preserves symlinks as links rather than dereferencing — prevents exfiltrating targets
/// outside the VFS. `cap_std::fs::Dir::copy` follows the source link and would write a
/// regular file, so the walk stays manual.
fn copy_recursive(from: &LinkPath, to: &LinkPath, run: &CopyRun<'_>) -> wasmtime::Result<()> {
    let CopyRun {
        recursive,
        links,
        op,
        pair,
        quota,
    } = *run;
    let meta = from
        .symlink_metadata()
        .map_err(|e| contain_trap(op, pair, &e))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        copy_link(from, to).map_err(|e| contain_trap(op, pair, &e))?;
    } else if ft.is_dir() {
        if !recursive {
            wasmtime::bail!("{op} {pair}: source is a directory (use {{ recursive: true }})");
        }
        to.create_dir_all()
            .map_err(|e| contain_trap(op, pair, &e))?;
        let from_dir = Arc::new(from.open_dir().map_err(|e| contain_trap(op, pair, &e))?);
        let to_dir = Arc::new(to.open_dir().map_err(|e| contain_trap(op, pair, &e))?);
        let entries = from_dir
            .entries()
            .map_err(|e| contain_trap(op, pair, &ContainError::from(e)))?;
        let nested = CopyRun {
            recursive: true,
            ..*run
        };
        for entry in entries {
            let entry = entry.map_err(|e| contain_trap(op, pair, &ContainError::from(e)))?;
            let name = entry.file_name();
            let child_from = from.child(Arc::clone(&from_dir), name.clone());
            let child_to = to.child(Arc::clone(&to_dir), name);
            if entry.file_type().is_ok_and(|ft| ft.is_symlink()) && links == LinkCopies::BestEffort
            {
                // A `.venv/bin/python -> /usr/bin/python3` is one the platform may refuse to
                // reproduce, and that must not fail a whole checkout copy. Scoped to the link
                // reproduction itself: recursing and swallowing every error would report
                // success for a tree that silently lost entries to a full disk or a
                // read-only destination.
                let _ = copy_link(&child_from, &child_to);
                continue;
            }
            copy_recursive(&child_from, &child_to, &nested)?;
        }
    } else {
        // Each file is reserved as the walk reaches it, so a copy that would pass the
        // size limit stops at the file that would, with what came before in place.
        // No program code runs during the copy, and it truncates and rewrites the
        // destination itself, so it draws on the destination's old contents.
        let mut disk_charge = QuotaCharge::in_place(quota.cloned(), to.copied_onto_file());
        disk_charge
            .reserve(meta.len())
            .map_err(|exceeded| quota_refusal(op, pair, exceeded))?;
        if let Err(e) = from.copy_to(to) {
            // The copy may have truncated the destination and landed part of it.
            disk_charge.settle_at(to.copied_onto_file().map_or(0, |(_, bytes)| bytes));
            return Err(contain_trap(op, pair, &e));
        }
        disk_charge.commit();
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
import { info, stat, peek, list, writer, writeText, Info, MountInfo, Stat, Peek, DirEntry, FileWriter } from "submilli:fs";

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
class MountField extends Parent { value: MountInfo | null = null; }
function main(): void {
 writeText("/input.txt", "text");
 const i = new InfoField(); assert(i.value.mode.length > 0, "Info");
 const s = new StatField(); s.reset(stat("/input.txt")); assert(s.value!.size === 4, "Stat");
 i.reset(s.value); rejects(() => { const v = i.value; });
 s.reset(info()); rejects(() => { const v = s.value; });
 const p = new PeekField(); p.reset(peek("/input.txt")); assert(p.value!.size === 4, "Peek");
 const e = new EntryField();
 for (const entry of list("/", false)) { if (entry.name === "input.txt") e.reset(entry); }
 assert(e.value!.name === "input.txt", "DirEntry");
 p.reset(e.value); rejects(() => { const v = p.value; });
 e.reset(peek("/input.txt")); rejects(() => { const v = e.value; });
 const w = new WriterField(); w.reset(writer("/output.txt")); w.value!.close();
 w.reset(info()); rejects(() => { const v = w.value; });
 const m = new MountField(); m.reset(info().mounts[0]); assert(m.value!.path === "/m", "MountInfo");
 m.reset(info()); rejects(() => { const v = m.value; });
 i.reset(info().mounts[0]); rejects(() => { const v = i.value; });
}
"#;
        let volume = tempfile::tempdir().unwrap();
        let vfs = mount_volume(
            Vfs::tempdir().unwrap(),
            "/m",
            volume.path(),
            "m",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        run_with(source, StoreData::with_vfs(vfs))
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
            .find(|context| context.get("length").is_some())
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
            }
        "#;
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.vfs_info = VfsInfo {
            mode: VfsMode::PerSession,
            size_limit: Some(1024 * 1024),
        };
        run_with(source, data).await.expect("info asserts hold");
    }

    fn limited(limit: u64) -> StoreData {
        let vfs = Vfs::tempdir().expect("tempdir").with_size_limit(limit);
        let mut data = StoreData::with_vfs(vfs);
        data.vfs_info.size_limit = Some(limit);
        data
    }

    #[tokio::test]
    async fn size_limit_refuses_growth_but_allows_rewrites_within_it() {
        let source = r#"
            import { writeText, readText, append, info } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                assert(info().sizeLimit === 100, "limit surfaced");
                writeText("/a.txt", "x".repeat(60));
                assert(refused(() => writeText("/b.txt", "y".repeat(60))), "a second file past the limit");
                writeText("/a.txt", "z".repeat(90));
                assert(readText("/a.txt") === "z".repeat(90), "a rewrite that ends within the limit");
                assert(refused(() => append("/a.txt", new Uint8Array(20))), "an append past the limit");
                writeText("/a.txt", "");
                writeText("/b.txt", "y".repeat(100));
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("limit asserts hold");
    }

    #[tokio::test]
    async fn size_limit_stops_a_writer_and_frees_its_temp_file() {
        let source = r#"
            import { writer, writeText, exists } from "submilli:fs";
            function main(): void {
                const w = writer("/log.txt");
                w.writeLine("x".repeat(40));
                let refused = false;
                try { w.writeLine("y".repeat(80)); } catch (e) { refused = e instanceof RangeError; }
                assert(refused, "the writer stops at the limit");
                w.close();
                assert(exists("/log.txt"), "what was written before the limit is kept");
                writeText("/other.txt", "z".repeat(59));
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("writer asserts hold");
    }

    #[tokio::test]
    async fn size_limit_stops_a_copy_at_the_file_that_would_pass_it() {
        let source = r#"
            import { writeText, copy, exists } from "submilli:fs";
            function main(): void {
                writeText("/a.txt", "x".repeat(40));
                copy("/a.txt", "/b.txt", false);
                let refused = false;
                try { copy("/a.txt", "/c.txt", false); } catch (e) { refused = e instanceof RangeError; }
                assert(refused, "a third copy passes the limit");
                assert(!exists("/c.txt"), "the refused copy leaves nothing");
                copy("/b.txt", "/a.txt", false);
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("copy asserts hold");
    }

    #[tokio::test]
    async fn size_limit_counts_what_remove_and_move_free() {
        let source = r#"
            import { writeText, remove, move } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/a.txt", "x".repeat(60));
                remove("/a.txt", false);
                writeText("/b.txt", "y".repeat(60));
                writeText("/c.txt", "z".repeat(30));
                move("/c.txt", "/b.txt");
                writeText("/d.txt", "w".repeat(70));
                move("/d.txt", "/d.txt");
                assert(refused(() => writeText("/e.txt", "v".repeat(1))), "a move onto itself frees nothing");
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("remove and move free space");
    }

    #[tokio::test]
    async fn size_limit_counts_a_writer_whose_target_shrinks_mid_write() {
        let source = r#"
            import { writer, writeText } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/a.txt", "x".repeat(90));
                const w = writer("/a.txt");
                writeText("/a.txt", "");
                w.writeBytes(new Uint8Array(90));
                w.close();
                assert(refused(() => writeText("/b.txt", "y".repeat(90))), "the writer's 90 bytes still count");
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("stale allowance is not honoured");
    }

    #[tokio::test]
    async fn size_limit_counts_every_writer_on_one_path() {
        let source = r#"
            import { writer, writeText } from "submilli:fs";
            function main(): void {
                writeText("/a.txt", "x".repeat(60));
                const first = writer("/a.txt");
                first.writeBytes(new Uint8Array(30));
                const second = writer("/a.txt");
                let refused = false;
                try { second.writeBytes(new Uint8Array(30)); } catch (e) { refused = e instanceof RangeError; }
                assert(refused, "two open writers' bytes both count");
                first.close();
                second.close();
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("each writer is counted");
    }

    #[tokio::test]
    async fn a_directory_over_its_limit_can_be_cleaned_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("big.txt"), vec![b'x'; 200]).expect("seed");
        let vfs = Vfs::external_with_mode(dir.path().to_path_buf(), VfsMode::PerSession)
            .expect("external")
            .with_size_limit(100);
        let source = r#"
            import { writeText, remove } from "submilli:fs";
            function main(): void {
                writeText("/big.txt", "");
                writeText("/big.txt", "x".repeat(10));
                remove("/big.txt", false);
                writeText("/after.txt", "y".repeat(100));
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("truncating and removing recover the limit");
    }

    #[tokio::test]
    async fn a_writer_temp_file_moved_away_stays_counted() {
        let source = r#"
            import { writer, list, move, writeText } from "submilli:fs";
            function main(): void {
                const w = writer("/w.bin");
                w.writeBytes(new Uint8Array(60));
                let temp = "";
                for (const e of list("/", false)) { if (e.name.startsWith("w.bin.")) { temp = e.path; } }
                move(temp, "/kept.bin");
                let failed = false;
                try { w.close(); } catch (e) { failed = true; }
                assert(failed, "close notices its temp file was moved");
                let refused = false;
                try { writeText("/x.txt", "y".repeat(50)); } catch (e) { refused = e instanceof RangeError; }
                assert(refused, "the moved bytes still count");
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("moved temp stays counted");
    }

    // Relies on a removed file's link count reaching 0 while the writer holds it,
    // which Windows may report differently; there the count only over-counts.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_writer_leaves_a_file_the_program_put_at_its_temp_name() {
        let source = r#"
            import { writer, list, remove, writeText, exists } from "submilli:fs";
            function main(): void {
                const w = writer("/w.bin");
                w.writeBytes(new Uint8Array(40000));
                let temp = "";
                for (const e of list("/", false)) { if (e.name.startsWith("w.bin.")) { temp = e.path; } }
                remove(temp, false);
                writeText(temp, "");
                let failed = false;
                try { w.close(); } catch (e) { failed = true; }
                assert(failed, "close notices its temp file was replaced");
                assert(exists(temp), "the program's own file is left alone");
                writeText("/x.bin", "y".repeat(100000));
            }
        "#;
        run_with(source, limited(100000))
            .await
            .expect("a replaced temp file is neither committed nor double-freed");
    }

    #[tokio::test]
    async fn an_unmeasured_directory_opens_as_full() {
        let vfs = Vfs::tempdir()
            .expect("tempdir")
            .with_measured_limit(100, None);
        let source = r#"
            import { writeText, remove } from "submilli:fs";
            function main(): string {
                writeText("/empty.txt", "");
                remove("/empty.txt", false);
                try { writeText("/a.txt", "x"); return "allowed"; }
                catch (e) { return (e instanceof RangeError ? "true " : "false ") + (e as Error).message; }
            }
        "#;
        let result = run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("runs")
            .expect("a result");
        assert!(result.starts_with("true "), "{result}");
        assert!(result.contains("couldn't be measured"), "{result}");
    }

    #[tokio::test]
    async fn size_limit_counts_what_a_recursive_remove_frees() {
        let source = r#"
            import { mkdir, writeText, remove } from "submilli:fs";
            function main(): void {
                mkdir("/d/e", true);
                writeText("/d/e/a.txt", "x".repeat(60));
                writeText("/d/b.txt", "y".repeat(30));
                remove("/d", true);
                writeText("/c.txt", "z".repeat(100));
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("a recursive remove frees its whole tree");
    }

    #[tokio::test]
    async fn a_removed_file_a_reader_holds_counts_until_the_reader_closes() {
        let source = r#"
            import { writeText, bytes, remove } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/f.bin", "x".repeat(90));
                for (const chunk of bytes("/f.bin", 1)) {
                    remove("/f.bin", false);
                    assert(refused(() => writeText("/g.txt", "y".repeat(20))), "the open file's bytes still count");
                    break;
                }
                writeText("/g.txt", "y".repeat(100));
            }
        "#;
        run_with(source, limited(100))
            .await
            .expect("a closed reader frees what its removed file held");
    }

    #[tokio::test]
    async fn a_write_over_a_writers_temp_file_is_counted_in_full() {
        let source = r#"
            import { writer, list, writeText } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                const w = writer("/a.bin");
                w.writeBytes(new Uint8Array(40000));
                let temp = "";
                for (const e of list("/", false)) { if (e.name.startsWith("a.bin.")) { temp = e.path; } }
                writeText(temp, "x".repeat(40000));
                let failed = false;
                try { w.close(); } catch (e) { failed = true; }
                assert(failed, "close notices its temp file was replaced");
                assert(refused(() => writeText("/b.txt", "y".repeat(60001))), "the 40000 at the temp name count");
                writeText("/b.txt", "y".repeat(60000));
            }
        "#;
        run_with(source, limited(100000))
            .await
            .expect("the replaced temp file's bytes are counted exactly once");
    }

    #[tokio::test]
    async fn rewriting_a_file_a_reader_holds_counts_both_copies() {
        let source = r#"
            import { writeText, bytes } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/f.bin", "x".repeat(40000));
                for (const chunk of bytes("/f.bin", 1)) {
                    writeText("/f.bin", "z".repeat(40000));
                    assert(refused(() => writeText("/g.txt", "y".repeat(20001))), "the old copy is still on disk");
                    break;
                }
                writeText("/g.txt", "y".repeat(60000));
            }
        "#;
        run_with(source, limited(100000))
            .await
            .expect("the old copy is freed when the reader closes");
    }

    #[tokio::test]
    async fn copying_over_a_file_a_reader_holds_counts_only_the_growth() {
        let source = r#"
            import { writeText, copy, bytes } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/f.bin", "x".repeat(40000));
                writeText("/src.bin", "z".repeat(40000));
                for (const chunk of bytes("/f.bin", 1)) {
                    copy("/src.bin", "/f.bin", false);
                    writeText("/g.txt", "y".repeat(20000));
                    assert(refused(() => writeText("/h.txt", "w")), "the limit is reached");
                    break;
                }
            }
        "#;
        run_with(source, limited(100000))
            .await
            .expect("a copy rewrites the held file in place");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn copying_onto_a_link_frees_the_file_it_rewrites() {
        let vfs = Vfs::tempdir().expect("tempdir");
        std::fs::write(vfs.root().join("t.bin"), vec![b't'; 50000]).expect("write");
        std::os::unix::fs::symlink("t.bin", vfs.root().join("l")).expect("symlink");
        let vfs = vfs.with_size_limit(100000);
        let mut data = StoreData::with_vfs(vfs);
        data.vfs_info.size_limit = Some(100000);
        let source = r#"
            import { writeText, copy } from "submilli:fs";
            function main(): void {
                writeText("/s.bin", "0123456789");
                copy("/s.bin", "/l", false);
                writeText("/g.txt", "y".repeat(99980));
            }
        "#;
        run_with(source, data)
            .await
            .expect("the link's target shrank to the copied 10 bytes");
    }

    #[tokio::test]
    async fn copying_over_a_writers_temp_file_leaves_it_for_the_writer_to_settle() {
        let source = r#"
            import { writeText, writer, list, copy, mkdir } from "submilli:fs";
            function refused(write: () => void): boolean {
                try { write(); return false; } catch (e) { return e instanceof RangeError; }
            }
            function main(): void {
                writeText("/empty", "");
                writeText("/keep.bin", "k".repeat(20000));
                const w = writer("/a.bin");
                w.writeBytes(new Uint8Array(40000));
                let temp = "";
                for (const e of list("/", false)) {
                    if (e.name.startsWith("a.bin.")) { temp = e.path; }
                }
                copy("/empty", temp, false);
                mkdir("/a.bin", false);
                writeText("/a.bin/x", "1");
                let closed = true;
                try { w.close(); } catch (e) { closed = false; }
                assert(!closed, "close fails on the directory");
                assert(refused(() => writeText("/big.bin", "y".repeat(80000))), "20001 bytes remain on disk");
                writeText("/big.bin", "y".repeat(79999));
            }
        "#;
        run_with(source, limited(100000))
            .await
            .expect("the writer's temp file is freed once, when the writer discards it");
    }

    fn mount_volume(
        vfs: Vfs,
        guest: &str,
        host: &std::path::Path,
        volume: &str,
        access: crate::runtime::vfs::Access,
        quota: Option<Arc<crate::runtime::DiskQuota>>,
    ) -> Vfs {
        vfs.with_mount(crate::runtime::vfs::MountSpec {
            guest_path: guest.to_string(),
            host: host.to_path_buf(),
            volume: volume.to_string(),
            access,
            quota,
        })
        .expect("mount")
    }

    #[tokio::test]
    async fn mount_routes_its_paths_to_the_volume() {
        let volume = tempfile::tempdir().expect("volume");
        std::fs::write(volume.path().join("kept.txt"), "kept").expect("seed");
        let session = tempfile::tempdir().expect("session");
        let vfs = mount_volume(
            Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
                .expect("root"),
            "/data/memory",
            volume.path(),
            "project-memory",
            crate::runtime::vfs::Access::ReadWrite,
            Some(Arc::new(crate::runtime::DiskQuota::new(1000, 4))),
        );
        let root = vfs.root().to_path_buf();
        let source = r#"
            import { info, readText, writeText, list, stat, exists } from "submilli:fs";
            function main(): void {
                assert(readText("/data/memory/kept.txt") === "kept", "reads the volume");
                writeText("/data/memory/new.txt", "fresh");
                assert(readText("/data/memory/../memory/new.txt") === "fresh", ".. resolves before routing");
                assert(!exists("/data/memoryx/new.txt"), "a sibling prefix is not the mount");
                assert(stat("/data/memory")!.kind === "directory", "the mount point is a directory");
                const names: string[] = [];
                for (const entry of list("/data", false)) { names.push(entry.name); }
                assert(names.length === 1 && names[0] === "memory", "the parent lists the mount point");
                const paths: string[] = [];
                for (const entry of list("/", true)) { paths.push(entry.path); }
                assert(paths.includes("/data/memory/kept.txt"), "a recursive walk descends into the volume");
                assert(paths.includes("/data/memory/new.txt"), "and sees new files");
                const fs = info();
                assert(fs.access === "read_write" && fs.volume === "", "root info");
                assert(fs.mounts.length === 1, "one mount");
                const mount = fs.mounts[0];
                assert(mount.path === "/data/memory" && mount.mode === "named", "mount path and mode");
                assert(mount.volume === "project-memory" && mount.access === "read_write", "mount volume and access");
                assert(mount.sizeLimit === 1000, "mount size limit");
            }
        "#;
        let data = StoreData::with_vfs(vfs);
        let quota = data.vfs.mounts()[0].quota().cloned().expect("quota");
        run_with(source, data).await.expect("mount asserts hold");
        assert_eq!(
            std::fs::read_to_string(volume.path().join("new.txt")).expect("in the volume"),
            "fresh"
        );
        let placeholder = std::fs::read_dir(root.join("data/memory")).expect("placeholder");
        assert_eq!(placeholder.count(), 0, "the root's mount point stays empty");
        assert_eq!(quota.used(), 9, "the write is charged to the volume");
    }

    #[tokio::test]
    async fn mount_read_only_refuses_every_write() {
        let volume = tempfile::tempdir().expect("volume");
        std::fs::create_dir(volume.path().join("dir")).expect("seed dir");
        std::fs::write(volume.path().join("dir/a.txt"), "a").expect("seed");
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/handbook",
            volume.path(),
            "company-handbook",
            crate::runtime::vfs::Access::ReadOnly,
            None,
        );
        let source = r#"
            import { info, readText, writeText, append, writer, mkdir, remove, move, copy, list } from "submilli:fs";
            function denied(op: () => void): string {
                try { op(); } catch (e: PermissionDeniedError) { return e.capability; }
                return "allowed";
            }
            function main(): void {
                writeText("/local.txt", "l");
                assert(readText("/handbook/dir/a.txt") === "a", "reads work");
                let count = 0;
                for (const entry of list("/handbook", true)) { count++; }
                assert(count === 2, "listing works");
                assert(denied(() => writeText("/handbook/b.txt", "b")) === "fs.write", "write");
                assert(denied(() => append("/handbook/dir/a.txt", new Uint8Array(1))) === "fs.write", "append");
                assert(denied(() => writer("/handbook/c.txt")) === "fs.write", "writer");
                assert(denied(() => mkdir("/handbook/new", false)) === "fs.mkdir", "mkdir");
                mkdir("/handbook/dir", true);
                assert(denied(() => remove("/handbook/dir/a.txt", false)) === "fs.remove", "remove");
                assert(denied(() => move("/handbook/dir/a.txt", "/moved.txt")) === "fs.move", "move out");
                assert(denied(() => move("/local.txt", "/handbook/l.txt")) === "fs.move", "move in");
                assert(denied(() => copy("/local.txt", "/handbook/l.txt", false)) === "fs.copy", "copy in");
                copy("/handbook/dir/a.txt", "/copied.txt", false);
                assert(readText("/copied.txt") === "a", "copy out works");
                assert(info().mounts[0].access === "read_only", "info reports read-only");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("read-only asserts hold");
        assert!(!volume.path().join("b.txt").exists());
        assert!(volume.path().join("dir/a.txt").exists());
    }

    #[tokio::test]
    async fn mount_policy_denial_comes_before_the_read_only_refusal() {
        let volume = tempfile::tempdir().expect("volume");
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/ro",
            volume.path(),
            "ro",
            crate::runtime::vfs::Access::ReadOnly,
            None,
        );
        let source = r#"
            import { writeText } from "submilli:fs";
            function main(): string {
                try { writeText("/ro/x.txt", "hi"); } catch (e: PermissionDeniedError) { return e.reason; }
                return "not denied";
            }
        "#;
        let mut data = StoreData::with_vfs(vfs);
        data.security_check = Arc::new(DenyAllFs);
        let value = run_with(source, data).await.expect("caught");
        assert_eq!(value.as_deref(), Some("denied fs.write in test"));
    }

    #[tokio::test]
    async fn mount_read_only_root_refuses_writes_and_needs_existing_mount_points() {
        let root = tempfile::tempdir().expect("root");
        let volume = tempfile::tempdir().expect("volume");
        let read_only = || {
            Vfs::external_with_mode(root.path().to_path_buf(), VfsMode::Named)
                .expect("root")
                .with_access(crate::runtime::vfs::Access::ReadOnly)
        };
        let missing = read_only().with_mount(crate::runtime::vfs::MountSpec {
            guest_path: "/rw".to_string(),
            host: volume.path().to_path_buf(),
            volume: "rw".to_string(),
            access: crate::runtime::vfs::Access::ReadWrite,
            quota: None,
        });
        assert!(
            matches!(
                missing,
                Err(crate::runtime::vfs::MountError::MountPointUnavailable { .. })
            ),
            "a read-only root is never written, so the mount point must exist"
        );
        assert!(!root.path().join("rw").exists());
        std::fs::create_dir(root.path().join("rw")).expect("mount point");
        let vfs = mount_volume(
            read_only(),
            "/rw",
            volume.path(),
            "rw",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        let source = r#"
            import { writeText, info } from "submilli:fs";
            function main(): void {
                let denied = false;
                try { writeText("/x.txt", "x"); } catch (e: PermissionDeniedError) { denied = true; }
                assert(denied, "the root is read-only");
                writeText("/rw/x.txt", "x");
                assert(info().access === "read_only", "root access surfaced");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("read-only root asserts hold");
        assert!(volume.path().join("x.txt").exists());
    }

    #[tokio::test]
    async fn mount_points_cannot_be_removed_moved_or_copied_through() {
        let volume = tempfile::tempdir().expect("volume");
        std::fs::write(volume.path().join("v.txt"), "v").expect("seed");
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/a/m",
            volume.path(),
            "m",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        let source = r#"
            import { remove, move, copy, writeText, mkdir, exists } from "submilli:fs";
            function fails(op: () => void): boolean {
                try { op(); return false; } catch (e) { return true; }
            }
            function main(): void {
                writeText("/a/sibling.txt", "s");
                mkdir("/a", true);
                assert(fails(() => remove("/a/m", true)), "remove the mount point");
                assert(fails(() => remove("/a", true)), "remove an ancestor");
                assert(fails(() => move("/a/m", "/b")), "move the mount point");
                assert(fails(() => move("/a", "/b")), "move an ancestor");
                assert(fails(() => move("/a/sibling.txt", "/a/m")), "replace the mount point");
                assert(fails(() => copy("/a", "/b", true)), "copy an ancestor");
                assert(fails(() => copy("/", "/a/m/all", true)), "copy the root into a mount");
                assert(exists("/a/m/v.txt"), "the volume is intact");
                remove("/a/sibling.txt", false);
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("mount-point asserts hold");
        assert!(volume.path().join("v.txt").exists());
    }

    #[tokio::test]
    async fn mount_shared_volume_quota_spans_every_vfs_that_mounts_it() {
        let volume = tempfile::tempdir().expect("volume");
        let quota = Arc::new(crate::runtime::DiskQuota::new(100, 0));
        let mount = |vfs: Vfs, guest: &str| {
            mount_volume(
                vfs,
                guest,
                volume.path(),
                "shared",
                crate::runtime::vfs::Access::ReadWrite,
                Some(Arc::clone(&quota)),
            )
        };
        let first = mount(Vfs::tempdir().expect("tempdir"), "/one");
        let second = mount(Vfs::tempdir().expect("tempdir"), "/two");
        let write = r#"
            import { writeText } from "submilli:fs";
            function main(): void { writeText("/one/a.txt", "x".repeat(60)); }
        "#;
        run_with(write, StoreData::with_vfs(first))
            .await
            .expect("first write fits");
        let refuse = r#"
            import { writeText, readText } from "submilli:fs";
            function main(): void {
                assert(readText("/two/a.txt")!.length === 60, "the other alias sees it");
                let refused = false;
                try { writeText("/two/b.txt", "y".repeat(60)); } catch (e) { refused = e instanceof RangeError; }
                assert(refused, "the shared limit refuses the second write");
            }
        "#;
        run_with(refuse, StoreData::with_vfs(second))
            .await
            .expect("second alias shares the limit");
        assert_eq!(quota.used(), 60);
    }

    #[tokio::test]
    async fn mount_move_between_volumes_copies_then_removes_and_moves_the_charge() {
        let volume = tempfile::tempdir().expect("volume");
        let mount_quota = Arc::new(crate::runtime::DiskQuota::new(1000, 0));
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir").with_size_limit(1000),
            "/memory",
            volume.path(),
            "memory",
            crate::runtime::vfs::Access::ReadWrite,
            Some(Arc::clone(&mount_quota)),
        );
        let root_quota = vfs.quota().cloned().expect("root quota");
        let source = r#"
            import { writeText, readText, move, mkdir, exists } from "submilli:fs";
            function main(): void {
                writeText("/a.txt", "x".repeat(10));
                mkdir("/tree/sub", true);
                writeText("/tree/sub/b.txt", "y".repeat(20));
                move("/a.txt", "/memory/a.txt");
                move("/tree", "/memory/tree");
                assert(!exists("/a.txt") && !exists("/tree"), "sources removed");
                assert(readText("/memory/a.txt")!.length === 10, "file moved");
                assert(readText("/memory/tree/sub/b.txt")!.length === 20, "tree moved");
                move("/memory/a.txt", "/back.txt");
                assert(readText("/back.txt")!.length === 10, "moved back");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("cross-volume move asserts hold");
        assert_eq!(root_quota.used(), 10, "the root holds only back.txt");
        assert_eq!(mount_quota.used(), 20, "the volume holds the tree");
        let leftovers: Vec<_> = std::fs::read_dir(volume.path())
            .expect("volume")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(
            leftovers,
            vec![std::ffi::OsString::from("tree")],
            "no staging left"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mount_link_to_an_ancestor_cannot_reach_a_mount_point() {
        let session = tempfile::tempdir().expect("session");
        let volume = tempfile::tempdir().expect("volume");
        std::fs::write(volume.path().join("v.txt"), "v").expect("seed");
        let vfs = mount_volume(
            Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
                .expect("root"),
            "/a/m",
            volume.path(),
            "m",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        std::os::unix::fs::symlink("a", session.path().join("lnk")).expect("link");
        std::fs::create_dir(session.path().join("r")).expect("dir");
        std::os::unix::fs::symlink("..", session.path().join("r/up")).expect("link");
        std::os::unix::fs::symlink("a/m/x", session.path().join("dangling")).expect("link");
        std::os::unix::fs::symlink(".", session.path().join("here")).expect("link");
        let source = r#"
            import { remove, move, copy, writeText, readText, mkdir, append } from "submilli:fs";
            function refused(op: () => void): boolean {
                try { op(); return false; } catch (e) { return String(e).includes("mount point"); }
            }
            function main(): void {
                assert(refused(() => remove("/lnk/m", false)), "remove the placeholder through a link");
                assert(refused(() => remove("/lnk/m", true)), "recursive remove through a link");
                assert(refused(() => writeText("/lnk/m", "x")), "replace the placeholder through a link");
                assert(refused(() => writeText("/lnk/m/x.txt", "x")), "write inside the placeholder");
                assert(refused(() => move("/r/up/a", "/z")), "move an ancestor through a link");
                assert(refused(() => remove("/r/up/a", true)), "remove an ancestor through a link");
                assert(refused(() => copy("/r/up/a", "/copy", true)), "copy an ancestor through a link");
                copy("/lnk", "/linkcopy", false);
                assert(refused(() => mkdir("/lnk/m/new", false)), "mkdir inside the placeholder");
                assert(refused(() => append("/dangling", new Uint8Array(1))), "append through a dangling link");
                writeText("/plain.txt", "p");
                assert(refused(() => copy("/plain.txt", "/dangling", false)), "copy through a dangling link");
                writeText("/here", "replaced");
                assert(readText("/here") === "replaced", "a write replaces a link to the root rather than refusing");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("link asserts hold");
        assert!(
            session.path().join("a/m").is_dir(),
            "the placeholder survives"
        );
        assert_eq!(
            std::fs::read_dir(session.path().join("a/m"))
                .expect("placeholder")
                .count(),
            0,
            "nothing landed in the placeholder"
        );
        assert!(volume.path().join("v.txt").exists());
        assert!(!session.path().join("copy").exists());
        assert!(
            std::fs::symlink_metadata(session.path().join("linkcopy"))
                .expect("copied")
                .file_type()
                .is_symlink(),
            "copying the link copies the link itself, never the tree behind it"
        );
    }

    #[tokio::test]
    async fn mount_move_too_deep_to_remove_is_refused_before_copying() {
        let volume = tempfile::tempdir().expect("volume");
        let quota = Arc::new(crate::runtime::DiskQuota::new(1 << 20, 0));
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/memory",
            volume.path(),
            "memory",
            crate::runtime::vfs::Access::ReadWrite,
            Some(Arc::clone(&quota)),
        );
        let source = r#"
            import { mkdir, writeText, move, exists } from "submilli:fs";
            function main(): void {
                const deep = "/deep" + "/d".repeat(70);
                mkdir(deep, true);
                writeText(deep + "/f.txt", "x");
                let failed = false;
                try { move("/deep", "/memory/deep"); } catch (e) { failed = true; }
                assert(failed, "the move is refused");
                assert(exists(deep + "/f.txt"), "the source is untouched");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("deep move asserts hold");
        assert_eq!(
            std::fs::read_dir(volume.path()).expect("volume").count(),
            0,
            "no staging copy is left behind"
        );
        assert_eq!(quota.used(), 0, "nothing stays charged");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn mount_case_variant_directory_cannot_stand_in_as_a_mount_point() {
        use crate::runtime::vfs::{Access, MountError, MountSpec};
        let session = tempfile::tempdir().expect("session");
        let volume = tempfile::tempdir().expect("volume");
        std::fs::create_dir(session.path().join("Memory")).expect("variant");
        let result = Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
            .expect("root")
            .with_mount(MountSpec {
                guest_path: "/memory".to_string(),
                host: volume.path().to_path_buf(),
                volume: "memory".to_string(),
                access: Access::ReadWrite,
                quota: None,
            });
        assert!(
            matches!(result, Err(MountError::MountPointUnavailable { .. })),
            "a case variant is refused"
        );
    }

    #[tokio::test]
    async fn mount_move_between_volumes_onto_existing_entries() {
        let volume = tempfile::tempdir().expect("volume");
        let quota = Arc::new(crate::runtime::DiskQuota::new(1000, 0));
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/memory",
            volume.path(),
            "memory",
            crate::runtime::vfs::Access::ReadWrite,
            Some(Arc::clone(&quota)),
        );
        let source = r#"
            import { writeText, readText, move, mkdir, exists } from "submilli:fs";
            function failure(op: () => void): string {
                try { op(); return "ok"; } catch (e) { return String(e); }
            }
            function main(): void {
                writeText("/memory/old.txt", "o".repeat(30));
                writeText("/new.txt", "n".repeat(10));
                move("/new.txt", "/memory/old.txt");
                assert(readText("/memory/old.txt")!.length === 10, "the file is replaced");
                mkdir("/dir", false);
                writeText("/file.txt", "f");
                assert(failure(() => move("/file.txt", "/memory")).includes("mount point"), "onto the mount point");
                mkdir("/memory/full", false);
                writeText("/memory/full/x.txt", "x");
                assert(failure(() => move("/dir", "/memory/full")).includes("not empty"), "onto a non-empty directory");
                assert(failure(() => move("/file.txt", "/memory/full")).includes("onto a directory"), "a file onto a directory");
                assert(exists("/dir") && exists("/file.txt"), "the sources are untouched");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("existing-destination asserts hold");
        assert_eq!(
            quota.used(),
            11,
            "the replaced file is released; only new files count"
        );
        let staged: Vec<_> = std::fs::read_dir(volume.path())
            .expect("volume")
            .map(|entry| entry.expect("entry").file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(staged.is_empty(), "{staged:?}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn mount_non_ascii_alias_of_a_mount_point_is_refused() {
        let session = tempfile::tempdir().expect("session");
        let volume = tempfile::tempdir().expect("volume");
        let vfs = mount_volume(
            Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
                .expect("root"),
            "/skills",
            volume.path(),
            "skills",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        // U+017F (long s) folds to `s` on a case-insensitive APFS volume.
        let source = r#"
            import { writeText, remove, move } from "submilli:fs";
            function refused(op: () => void): boolean {
                try { op(); return false; } catch (e) { return String(e).includes("mount point"); }
            }
            function main(): void {
                assert(refused(() => writeText("/ſkills/x.txt", "x")), "write through the alias");
                assert(refused(() => remove("/ſkills", true)), "remove the mount point by its alias");
                assert(refused(() => move("/ſkills", "/elsewhere")), "move the mount point by its alias");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("alias asserts hold");
        assert_eq!(
            std::fs::read_dir(session.path().join("skills"))
                .expect("placeholder")
                .count(),
            0
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn mount_nested_non_ascii_aliases_of_a_mount_path_are_refused() {
        let session = tempfile::tempdir().expect("session");
        let volume = tempfile::tempdir().expect("volume");
        let vfs = mount_volume(
            Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
                .expect("root"),
            "/sets/skills",
            volume.path(),
            "skills",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        // U+017F (long s) folds to `s` on a case-insensitive APFS volume, in the
        // parent as well as in the mount point.
        let source = r#"
            import { writeText, remove, move } from "submilli:fs";
            function refused(op: () => void): boolean {
                try { op(); return false; } catch (e) { return String(e).includes("mount point"); }
            }
            function main(): void {
                assert(refused(() => writeText("/\u017Fets/\u017Fkills/x.txt", "x")), "both components aliased");
                assert(refused(() => writeText("/Sets/\u017Fkills/x.txt", "x")), "case and Unicode aliases");
                assert(refused(() => remove("/\u017Fets", true)), "remove an aliased ancestor");
                assert(refused(() => move("/\u017Fets", "/elsewhere")), "move an aliased ancestor");
                writeText("/\u017Fets/beside.txt", "beside");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("nested alias asserts hold");
        assert_eq!(
            std::fs::read_dir(session.path().join("sets/skills"))
                .expect("placeholder")
                .count(),
            0
        );
        assert!(
            session.path().join("sets/beside.txt").exists(),
            "siblings stay writable"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mount_root_link_loops_and_absolute_links_fail_cleanly() {
        let session = tempfile::tempdir().expect("session");
        let volume = tempfile::tempdir().expect("volume");
        let vfs = mount_volume(
            Vfs::external_with_mode(session.path().to_path_buf(), VfsMode::PerSession)
                .expect("root"),
            "/memory",
            volume.path(),
            "memory",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        std::os::unix::fs::symlink("loop", session.path().join("loop")).expect("link");
        std::os::unix::fs::symlink("/etc", session.path().join("abs")).expect("link");
        let source = r#"
            import { writeText, append } from "submilli:fs";
            function message(op: () => void): string {
                try { op(); return "ok"; } catch (e) { return String(e); }
            }
            function main(): void {
                assert(message(() => append("/loop/x", new Uint8Array(1))) !== "ok", "a loop fails");
                assert(message(() => append("/abs/x", new Uint8Array(1))).includes("escapes"), "an absolute link escapes");
                writeText("/fine.txt", "fine");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("loop asserts hold");
    }

    #[tokio::test]
    async fn mount_move_onto_a_mount_ancestor_is_refused_before_copying() {
        let volume = tempfile::tempdir().expect("volume");
        let other = tempfile::tempdir().expect("other");
        let quota = Arc::new(crate::runtime::DiskQuota::new(1 << 20, 0));
        let root = tempfile::tempdir().expect("root");
        let vfs = mount_volume(
            mount_volume(
                Vfs::external_with_mode(root.path().to_path_buf(), VfsMode::PerSession)
                    .expect("root")
                    .with_size_limit(1 << 20),
                "/memory",
                volume.path(),
                "memory",
                crate::runtime::vfs::Access::ReadWrite,
                None,
            ),
            "/sub/inner",
            other.path(),
            "inner",
            crate::runtime::vfs::Access::ReadWrite,
            Some(Arc::clone(&quota)),
        );
        let source = r#"
            import { writeText, move, exists } from "submilli:fs";
            function main(): void {
                writeText("/memory/tree.txt", "t");
                let failed = false;
                try { move("/memory/tree.txt", "/sub"); } catch (e) { failed = String(e).includes("mount point"); }
                assert(failed, "the move onto a mount's ancestor is refused");
                assert(exists("/memory/tree.txt"), "the source is untouched");
            }
        "#;
        run_with(source, StoreData::with_vfs(vfs))
            .await
            .expect("ancestor move asserts hold");
        assert_eq!(quota.used(), 0);
        let staged: Vec<_> = std::fs::read_dir(root.path())
            .expect("root")
            .map(|entry| entry.expect("entry").file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            staged.is_empty(),
            "nothing was staged in the root: {staged:?}"
        );
    }

    /// Mounting reads an identity for every directory on the mount path and fails
    /// if it can't, so this passing means the host supplies them.
    #[test]
    fn mount_reads_an_identity_for_each_directory_on_its_path() {
        let volume = tempfile::tempdir().expect("volume");
        let vfs = mount_volume(
            Vfs::tempdir().expect("tempdir"),
            "/data/memory",
            volume.path(),
            "memory",
            crate::runtime::vfs::Access::ReadWrite,
            None,
        );
        let names: Vec<_> = vfs.mounts()[0]
            .placeholders()
            .iter()
            .map(|placeholder| placeholder.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["data", "memory"],
            "every directory on the mount path got a placeholder"
        );
    }

    #[test]
    fn mount_paths_are_plain_ascii() {
        use crate::runtime::vfs::{Access, MountError, MountSpec};
        let volume = tempfile::tempdir().expect("volume");
        for bad in ["/mémoire", "/a b", "/a\\b", "/memory.", "/..."] {
            let result = Vfs::tempdir().expect("tempdir").with_mount(MountSpec {
                guest_path: bad.to_string(),
                host: volume.path().to_path_buf(),
                volume: "v".to_string(),
                access: Access::ReadWrite,
                quota: None,
            });
            assert!(matches!(result, Err(MountError::BadPath(_))), "{bad}");
        }
    }

    #[test]
    fn mounts_reject_ambiguous_tables() {
        use crate::runtime::vfs::{Access, MountError, MountSpec};
        let volume = tempfile::tempdir().expect("volume");
        let spec = |guest: &str, name: &str| MountSpec {
            guest_path: guest.to_string(),
            host: volume.path().to_path_buf(),
            volume: name.to_string(),
            access: Access::ReadWrite,
            quota: None,
        };
        assert!(matches!(
            Vfs::none().with_mount(spec("/a", "a")),
            Err(MountError::RootDisabled)
        ));
        let base = || Vfs::tempdir().expect("tempdir");
        for bad in ["a", "/a/", "/a//b", "/a/./b", "/a/../b", ""] {
            assert!(
                matches!(
                    base().with_mount(spec(bad, "a")),
                    Err(MountError::BadPath(_))
                ),
                "{bad:?} is not a normalized absolute path"
            );
        }
        assert!(matches!(
            base().with_mount(spec("/", "a")),
            Err(MountError::AtRoot)
        ));
        assert!(matches!(
            base().with_mount(spec("/x/.git", "a")),
            Err(MountError::ProtectedPath(_))
        ));
        let one = base().with_mount(spec("/a", "a")).expect("first");
        assert!(matches!(
            one.clone().with_mount(spec("/a/b", "b")),
            Err(MountError::Nested { .. })
        ));
        assert!(matches!(
            one.clone().with_mount(spec("/c", "a")),
            Err(MountError::DuplicateVolume { .. })
        ));
        let root = one.root().to_path_buf();
        std::fs::write(root.join("file"), "f").expect("file");
        assert!(matches!(
            one.clone().with_mount(spec("/file/x", "c")),
            Err(MountError::MountPointUnavailable { .. })
        ));
        let mut many = base();
        for index in 0..crate::runtime::vfs::MAX_MOUNTS {
            many = many
                .with_mount(spec(&format!("/m{index}"), &format!("v{index}")))
                .expect("within the cap");
        }
        assert!(matches!(
            many.with_mount(spec("/extra", "extra")),
            Err(MountError::TooMany)
        ));
    }
}
