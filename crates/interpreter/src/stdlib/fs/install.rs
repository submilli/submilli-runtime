//! Explicit host registrations for filesystem operations and resource methods.
use super::handles::{BackendReader, ChargedDirIter};
use super::{
    ENTRY_KIND, ENTRY_NAME, ENTRY_PATH, ENTRY_SIZE, HostMutationBudget, INFO_ACCESS, INFO_MODE,
    INFO_MOUNTS, INFO_SIZE_LIMIT, INFO_VOLUME, MODULE_NAME, MOUNT_ACCESS, MOUNT_MODE, MOUNT_PATH,
    MOUNT_SIZE_LIMIT, MOUNT_VOLUME, PEEK_ENCODING, PEEK_LINE_ENDING, PEEK_PREVIEW, PEEK_SIZE,
    STAT_KIND, STAT_MODIFIED_AT, STAT_SIZE, append, append_text, backend_error, begin_work,
    build_info, charge_path_work, dir_entry_backing_struct, effects, f64_arg, i32_flag,
    info_backing_struct, list_next, make_handle_iterator, mount_info_backing_struct, open_writer,
    peek_backing_struct, peek_file, read_byte_range, read_whole_capped, reader_next,
    stat_backing_struct, stat_entry, summary, write, write_text, writer_payload,
};
use crate::runtime::StoreData;
use crate::runtime::call_log::{Payload, Side, record_payload};
use crate::runtime::effects as runtime_effects;
use crate::runtime::filesystem::FilesystemError as PortError;
use crate::runtime::fs::ContainError;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, abi_result, read_string_arg, read_uint8_array_arg, register_host_fn,
    register_host_fn_async, register_host_value_async, write_submilli_string_struct,
    write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::stdlib::abi::{HostView, install_field_getters};
use crate::stdlib::shared::{check_security_call, require_writable};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

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
        |caller, _params, results| {
            *abi_result(results, 0)? = Val::F64((caller.data().fs_max_read_size as f64).to_bits());
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("cwd"),
        FuncType::new(&engine, [], [string.clone()]),
        |caller, _params, results| {
            let cwd = caller.data().filesystem.cwd().to_owned();
            let value = write_submilli_string_struct(&mut *caller, &cwd)?;
            let result = results.first_mut().ok_or_else(|| {
                crate::runtime::host::fatal_host_error("fs.cwd: missing result slot")
            })?;
            *result = Val::AnyRef(Some(value.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        fs_fn("info"),
        FuncType::new(&engine, [], [nullable_object.clone()]),
        |caller, _params, results| {
            *abi_result(results, 0)? = build_info(caller)?;
            Ok(())
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("exists"),
        FuncType::new(&engine, [string.clone()], [ValType::I32]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.exists")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.stat",
                    serde_json::json!({ "path": &path }),
                )?;
                let filesystem = caller.data().filesystem.clone();
                // A path the sandbox cannot reach is reported absent, not trapped: that
                // preserves the documented broken-link behaviour and denies the guest an
                // existence oracle over host layout.
                // `exists` is declared to mirror `std::path::exists` — a total predicate that
                // never throws. Every way a path can be unreachable answers `false`: outside the
                // sandbox, missing, a file used as a directory component, a symlink cycle. A
                // guard clause written as `if (!exists(p))` is exactly what an LLM writes, and it
                // must not abort on the malformed-path case it exists to handle.
                let found = runtime_effects::execute(
                    caller,
                    "fs.exists",
                    path,
                    move |caller, effect_id, path| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&path])?;
                            let work = begin_work(caller)?;
                            let found = match filesystem
                                .metadata(&effect_id, Some(work.clone()), &path, true)
                                .await
                            {
                                Ok(_) => true,
                                Err(
                                    PortError::Containment(
                                        ContainError::Escape | ContainError::Io(_),
                                    )
                                    | PortError::Io(_),
                                ) => false,
                                Err(e) => return Err(backend_error("fs.exists", &path, e)),
                            };
                            work.settle(&mut *caller)?;
                            Ok(found)
                        })
                    },
                )
                .await?;
                record_payload(&*caller, ticket, Side::Response, || {
                    Payload::meta(serde_json::json!({ "exists": found }))
                });
                Ok(Val::I32(i32::from(found)))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("size"),
        FuncType::new(&engine, [string.clone()], [ValType::F64]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.size")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.stat",
                    serde_json::json!({ "path": &path }),
                )?;
                let size =
                    runtime_effects::execute(caller, "fs.size", path, |caller, effect_id, path| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&path])?;
                            let work = begin_work(caller)?;
                            let meta = caller
                                .data()
                                .filesystem
                                .metadata(&effect_id, Some(work.clone()), &path, true)
                                .await
                                .map_err(|e| backend_error("fs.size", &path, e))?;
                            if meta.is_dir() {
                                wasmtime::bail!("fs.size {}: path is a directory", path);
                            }
                            work.settle(&mut *caller)?;
                            Ok(meta.len())
                        })
                    })
                    .await?;
                record_payload(&*caller, ticket, Side::Response, || {
                    Payload::meta(serde_json::json!({ "size": size }))
                });
                Ok(Val::F64((size as f64).to_bits()))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("stat"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.stat")?;
                check_security_call(
                    &mut *caller,
                    "fs.stat",
                    serde_json::json!({ "path": &path }),
                )?;
                stat_entry(caller, &path).await
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("peek"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.peek")?;
                check_security_call(
                    &mut *caller,
                    "fs.stat",
                    serde_json::json!({ "path": &path }),
                )?;
                peek_file(caller, &path).await
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("read"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.read")?;
                let output = read_whole_capped(caller, &path, "read").await?;
                match &output {
                    Some(bytes) => {
                        let arr = write_submilli_uint8array_struct(caller, &bytes.0)?;
                        Ok(Val::AnyRef(Some(arr.to_anyref())))
                    }
                    None => crate::runtime::prelude::undefined::value(caller),
                }
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("readText"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.readText")?;
                let output = read_whole_capped(caller, &path, "readText").await?;
                match &output {
                    Some(bytes) => {
                        // Lossy UTF-8: invalid bytes become U+FFFD rather than trapping.
                        let mut text = String::from_utf8_lossy(&bytes.0).into_owned();
                        if text.starts_with('\u{FEFF}') {
                            text.remove(0);
                        }
                        let st = write_submilli_string_struct(caller, &text)?;
                        Ok(Val::AnyRef(Some(st.to_anyref())))
                    }
                    None => crate::runtime::prelude::undefined::value(caller),
                }
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("readBytes"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64, ValType::F64],
            [uint8.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.readBytes")?;
                let offset = f64_arg(abi_arg(params, 1)?, "fs.readBytes (offset)")? as i64;
                let length = f64_arg(abi_arg(params, 2)?, "fs.readBytes (length)")? as i64;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.read",
                    serde_json::json!({
                        "path": &path,
                        "length": length,
                    }),
                )?;
                let bytes = read_byte_range(caller, &path, offset, length).await?;
                record_payload(&*caller, ticket, Side::Response, || {
                    Payload::meta(serde_json::json!({ "offset": offset })).with_body(&bytes.0)
                });
                let arr = write_submilli_uint8array_struct(caller, &bytes.0)?;
                Ok(Val::AnyRef(Some(arr.to_anyref())))
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("write"),
        FuncType::new(&engine, [string.clone(), uint8.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(caller, abi_arg(params, 0)?, "fs.write")?;
                let content = read_uint8_array_arg(caller, abi_arg(params, 1)?, "fs.write")?;
                write(caller, path, content).await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("writeText"),
        FuncType::new(&engine, [string.clone(), string.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(caller, abi_arg(params, 0)?, "fs.writeText")?;
                let content = read_string_arg(caller, abi_arg(params, 1)?, "fs.writeText")?;
                write_text(caller, path, content).await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("append"),
        FuncType::new(&engine, [string.clone(), uint8.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(caller, abi_arg(params, 0)?, "fs.append")?;
                let content = read_uint8_array_arg(caller, abi_arg(params, 1)?, "fs.append")?;
                append(caller, path, content).await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("appendText"),
        FuncType::new(&engine, [string.clone(), string.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(caller, abi_arg(params, 0)?, "fs.appendText")?;
                let content = read_string_arg(caller, abi_arg(params, 1)?, "fs.appendText")?;
                append_text(caller, path, content).await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("mkdir"),
        FuncType::new(&engine, [string.clone(), ValType::I32], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.mkdir")?;
                let recursive = i32_flag(abi_arg(params, 1)?, "fs.mkdir (recursive)")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.mkdir",
                    serde_json::json!({ "path": &path, "recursive": recursive }),
                )?;
                let request = effects::PathMutation { path, recursive };
                runtime_effects::execute(
                    caller,
                    "fs.mkdir",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&request.path])?;
                            let effects::PathMutation { path, recursive } = request;
                            let filesystem = caller.data().filesystem.clone();
                            let work = begin_work(caller)?;
                            let placement = filesystem
                                .placement(&path)
                                .await
                                .map_err(|e| backend_error("fs.mkdir", &path, e))?;
                            if !(recursive
                                && filesystem
                                    .metadata(&effect_id, Some(work.clone()), &path, true)
                                    .await
                                    .is_ok_and(|m| m.is_dir()))
                            {
                                require_writable(&*caller, ticket, &placement, "fs.mkdir", &path)?;
                            }
                            let result = filesystem
                                .mkdir(&effect_id, Some(work.clone()), &path, recursive)
                                .await;
                            work.settle(&mut *caller)?;
                            result.map_err(|e| backend_error("fs.mkdir", &path, e))?;
                            Ok(())
                        })
                    },
                )
                .await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("remove"),
        FuncType::new(&engine, [string.clone(), ValType::I32], []),
        |caller, params, _results| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.remove")?;
                let recursive = i32_flag(abi_arg(params, 1)?, "fs.remove (recursive)")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.remove",
                    serde_json::json!({ "path": &path, "recursive": recursive }),
                )?;
                let request = effects::PathMutation { path, recursive };
                runtime_effects::execute(
                    caller,
                    "fs.remove",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&request.path])?;
                            let effects::PathMutation { path, recursive } = request;
                            let filesystem = caller.data().filesystem.clone();
                            let work = begin_work(caller)?;
                            let placement = filesystem
                                .placement(&path)
                                .await
                                .map_err(|e| backend_error("fs.remove", &path, e))?;
                            require_writable(&*caller, ticket, &placement, "fs.remove", &path)?;
                            let mut budget = HostMutationBudget::new(caller);
                            let result = filesystem
                                .remove(
                                    &effect_id,
                                    Some(work.clone()),
                                    &path,
                                    recursive,
                                    &mut budget,
                                )
                                .await;
                            drop(budget);
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.remove", &path, error))
                        })
                    },
                )
                .await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("move"),
        FuncType::new(&engine, [string.clone(), string.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let from = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.move (from)")?;
                let to = read_string_arg(&mut *caller, abi_arg(params, 1)?, "fs.move (to)")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.move",
                    serde_json::json!({ "from": &from, "to": &to }),
                )?;
                let request = effects::MoveEntry { from, to };
                runtime_effects::execute(
                    caller,
                    "fs.move",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&request.from, &request.to])?;
                            let effects::MoveEntry { from, to } = request;
                            let filesystem = caller.data().filesystem.clone();
                            let work = begin_work(caller)?;
                            let placement = filesystem
                                .placement(&from)
                                .await
                                .map_err(|e| backend_error("fs.move", &from, e))?;
                            require_writable(&*caller, ticket, &placement, "fs.move", &from)?;
                            let placement = filesystem
                                .placement(&to)
                                .await
                                .map_err(|e| backend_error("fs.move", &to, e))?;
                            require_writable(&*caller, ticket, &placement, "fs.move", &to)?;
                            let mut budget = HostMutationBudget::new(caller);
                            let result = filesystem
                                .move_entry(&effect_id, Some(work.clone()), &from, &to, &mut budget)
                                .await;
                            drop(budget);
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.move", &to, error))
                        })
                    },
                )
                .await
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        fs_fn("copy"),
        FuncType::new(&engine, [string.clone(), string.clone(), ValType::I32], []),
        |caller, params, _results| {
            Box::pin(async move {
                let from = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.copy (from)")?;
                let to = read_string_arg(&mut *caller, abi_arg(params, 1)?, "fs.copy (to)")?;
                let recursive = i32_flag(abi_arg(params, 2)?, "fs.copy (recursive)")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.copy",
                    serde_json::json!({
                        "from": &from,
                        "to": &to,
                        "recursive": recursive,
                    }),
                )?;
                let request = effects::CopyEntry {
                    from,
                    to,
                    recursive,
                };
                runtime_effects::execute(
                    caller,
                    "fs.copy",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            charge_path_work(caller, &[&request.from, &request.to])?;
                            let effects::CopyEntry {
                                from,
                                to,
                                recursive,
                            } = request;
                            let filesystem = caller.data().filesystem.clone();
                            let work = begin_work(caller)?;
                            let placement = filesystem
                                .placement(&to)
                                .await
                                .map_err(|e| backend_error("fs.copy", &to, e))?;
                            require_writable(&*caller, ticket, &placement, "fs.copy", &to)?;
                            let mut budget = HostMutationBudget::new(caller);
                            let result = filesystem
                                .copy(
                                    &effect_id,
                                    Some(work.clone()),
                                    &from,
                                    &to,
                                    recursive,
                                    &mut budget,
                                )
                                .await;
                            drop(budget);
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.copy", &to, error))
                        })
                    },
                )
                .await
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("writer"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.writer (path)")?;
                let ticket = check_security_call(
                    &mut *caller,
                    "fs.write",
                    serde_json::json!({ "path": &path }),
                )?;
                open_writer(caller, ticket, &path).await
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("lines"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.lines (path)")?;
                check_security_call(
                    &mut *caller,
                    "fs.read",
                    serde_json::json!({ "path": &path }),
                )?;
                let options = crate::runtime::filesystem::ReaderOptions::Lines {
                    maximum_bytes: caller.data().fs_max_read_size,
                };
                let reader = effects::open_reader(caller, path, options, "fs.lines").await?;
                let reader = BackendReader::new(reader, &caller.data().tenant_limits)
                    .map_err(wasmtime::Error::new)?;
                make_handle_iterator(caller, reader, |caller, params| {
                    Box::new(async move { reader_next(caller, params).await })
                })
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("bytes"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::F64],
            [nullable_object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.bytes (path)")?;
                let chunk_size = f64_arg(abi_arg(params, 1)?, "fs.bytes (chunkSize)")? as i64;
                if chunk_size <= 0 {
                    return Err(crate::runtime::host::range_error(format!(
                        "fs.bytes: chunkSize must be > 0, got {chunk_size}"
                    )));
                }
                check_security_call(
                    &mut *caller,
                    "fs.read",
                    serde_json::json!({ "path": &path, "chunkSize": chunk_size }),
                )?;
                let options = crate::runtime::filesystem::ReaderOptions::Bytes {
                    chunk_size: chunk_size as usize,
                };
                let reader = effects::open_reader(caller, path, options, "fs.bytes").await?;
                let reader = BackendReader::new(reader, &caller.data().tenant_limits)
                    .map_err(wasmtime::Error::new)?;
                make_handle_iterator(caller, reader, |caller, params| {
                    Box::new(async move { reader_next(caller, params).await })
                })
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("list"),
        FuncType::new(
            &engine,
            [string.clone(), ValType::I32],
            [nullable_object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.list (path)")?;
                let recursive = i32_flag(abi_arg(params, 1)?, "fs.list (recursive)")?;
                check_security_call(
                    &mut *caller,
                    "fs.list",
                    serde_json::json!({ "path": &path, "recursive": recursive }),
                )?;
                let walk = effects::open_directory(caller, path.clone(), recursive).await?;
                let dir = ChargedDirIter::new(walk, &caller.data().tenant_limits)
                    .map_err(|e| wasmtime::Error::new(e).context(format!("fs.list {path}")))?;
                make_handle_iterator(caller, dir, |caller, params| {
                    Box::new(async move { list_next(caller, params).await })
                })
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("summary"),
        FuncType::new(&engine, [string.clone()], [string.clone()]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.summary")?;
                let text = summary::summarize(caller, &path).await?;
                let value = write_submilli_string_struct(caller, &text)?;
                Ok(Val::AnyRef(Some(value.to_anyref())))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        fs_fn("isEmpty"),
        FuncType::new(&engine, [string.clone()], [ValType::I32]),
        |caller, params| {
            Box::pin(async move {
                let path = read_string_arg(&mut *caller, abi_arg(params, 0)?, "fs.isEmpty")?;
                let empty = summary::is_empty(caller, &path).await?;
                Ok(Val::I32(i32::from(empty)))
            })
        },
    )?;

    for view in HOST_VIEWS {
        install_field_getters(linker, &engine, &object, view)?;
    }
    install_file_writer_methods(linker, &engine, &intr, object)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Getters and FileWriter methods
// ---------------------------------------------------------------------------

const STAT_VIEW: HostView = HostView {
    module: MODULE_NAME,
    interface: "Stat",
    backing: stat_backing_struct,
    properties: &[
        ("kind", STAT_KIND),
        ("size", STAT_SIZE),
        ("modifiedAt", STAT_MODIFIED_AT),
    ],
};

const PEEK_VIEW: HostView = HostView {
    module: MODULE_NAME,
    interface: "Peek",
    backing: peek_backing_struct,
    properties: &[
        ("preview", PEEK_PREVIEW),
        ("encoding", PEEK_ENCODING),
        ("lineEnding", PEEK_LINE_ENDING),
        ("size", PEEK_SIZE),
    ],
};

const DIR_ENTRY_VIEW: HostView = HostView {
    module: MODULE_NAME,
    interface: "DirEntry",
    backing: dir_entry_backing_struct,
    properties: &[
        ("kind", ENTRY_KIND),
        ("name", ENTRY_NAME),
        ("path", ENTRY_PATH),
        ("size", ENTRY_SIZE),
    ],
};

const INFO_VIEW: HostView = HostView {
    module: MODULE_NAME,
    interface: "Info",
    backing: info_backing_struct,
    properties: &[
        ("mode", INFO_MODE),
        ("sizeLimit", INFO_SIZE_LIMIT),
        ("access", INFO_ACCESS),
        ("volume", INFO_VOLUME),
        ("mounts", INFO_MOUNTS),
    ],
};

const MOUNT_INFO_VIEW: HostView = HostView {
    module: MODULE_NAME,
    interface: "MountInfo",
    backing: mount_info_backing_struct,
    properties: &[
        ("path", MOUNT_PATH),
        ("mode", MOUNT_MODE),
        ("volume", MOUNT_VOLUME),
        ("access", MOUNT_ACCESS),
        ("sizeLimit", MOUNT_SIZE_LIMIT),
    ],
};

/// The host-backed interfaces this package's values are.
pub(crate) const HOST_VIEWS: &[&HostView] = &[
    &STAT_VIEW,
    &PEEK_VIEW,
    &DIR_ENTRY_VIEW,
    &INFO_VIEW,
    &MOUNT_INFO_VIEW,
];

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

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "close"),
        FuncType::new(engine, [receiver.clone()], []),
        |caller, params, _results| {
            Box::pin(async move {
                let receiver = *abi_arg(params, 0)?;
                let descriptor = writer_payload(caller, &receiver, "fs.writer.close")?.id();
                let live_receiver = receiver;
                runtime_effects::execute(
                    caller,
                    "fs.writer.close",
                    descriptor,
                    move |caller, effect_id, _| {
                        let receiver = live_receiver;
                        Box::pin(async move {
                            let work = begin_work(caller)?;
                            let filesystem = caller.data().filesystem.clone();
                            let result = writer_payload(caller, &receiver, "fs.writer.close")?
                                .close(filesystem.as_ref(), &effect_id, Some(work.clone()))
                                .await;
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.writer.close", "", error))
                        })
                    },
                )
                .await?;
                writer_payload(caller, &receiver, "fs.writer.close")?.release_buffer();
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "writeLine"),
        FuncType::new(engine, [receiver.clone(), string], []),
        |caller, params, _results| {
            Box::pin(async move {
                let line =
                    read_string_arg(&mut *caller, abi_arg(params, 1)?, "fs.writer.writeLine")?;
                let mut bytes = line.into_bytes();
                bytes.push(b'\n');
                let receiver = *abi_arg(params, 0)?;
                let descriptor = writer_payload(caller, &receiver, "fs.writer.writeLine")?.id();
                let request = effects::WriterChunk {
                    resource: descriptor,
                    bytes,
                };
                runtime_effects::execute(
                    caller,
                    "fs.writer.writeLine",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            fuel::charge(&mut *caller, fuel::IO, request.bytes.len() as u64)?;
                            let work = begin_work(caller)?;
                            let filesystem = caller.data().filesystem.clone();
                            let result = writer_payload(caller, &receiver, "fs.writer.writeLine")?
                                .write(
                                    filesystem.as_ref(),
                                    &effect_id,
                                    Some(work.clone()),
                                    request.bytes,
                                )
                                .await;
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.writer.writeLine", "", error))
                        })
                    },
                )
                .await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&writer_key, "writeBytes"),
        FuncType::new(engine, [receiver, uint8], []),
        |caller, params, _results| {
            Box::pin(async move {
                let bytes = read_uint8_array_arg(
                    &mut *caller,
                    abi_arg(params, 1)?,
                    "fs.writer.writeBytes",
                )?;
                let receiver = *abi_arg(params, 0)?;
                let descriptor = writer_payload(caller, &receiver, "fs.writer.writeBytes")?.id();
                let request = effects::WriterChunk {
                    resource: descriptor,
                    bytes,
                };
                runtime_effects::execute(
                    caller,
                    "fs.writer.writeBytes",
                    request,
                    move |caller, effect_id, request| {
                        Box::pin(async move {
                            fuel::charge(&mut *caller, fuel::IO, request.bytes.len() as u64)?;
                            let work = begin_work(caller)?;
                            let filesystem = caller.data().filesystem.clone();
                            let result = writer_payload(caller, &receiver, "fs.writer.writeBytes")?
                                .write(
                                    filesystem.as_ref(),
                                    &effect_id,
                                    Some(work.clone()),
                                    request.bytes,
                                )
                                .await;
                            work.settle(&mut *caller)?;
                            result.map_err(|error| backend_error("fs.writer.writeBytes", "", error))
                        })
                    },
                )
                .await?;
                Ok(())
            })
        },
    )?;

    Ok(())
}
