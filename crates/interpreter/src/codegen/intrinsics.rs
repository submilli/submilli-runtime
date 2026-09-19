//! Canonical intrinsic Wasm types every Submilli module hard-codes locally.
//! Both the prelude and consumer codegen call [`declare_intrinsic_types`] to guarantee
//! identical bytes; WasmGC structural canonicalization then unifies cross-module references.

use wasm_encoder::{
    CompositeInnerType, CompositeType, FieldType, FuncType, HeapType, RefType, StorageType,
    StructType, SubType, TypeSection, ValType,
};
#[derive(Clone, Copy, Debug)]
pub struct IntrinsicTypeIndices {
    pub raw_string: u32,
    pub vtable: u32,
    pub object: u32,
    pub string: u32,
    pub boxed_number: u32,
    pub boxed_boolean: u32,
    pub field_names: u32,
    pub object_shape: u32,
    pub object_fields: u32,
    pub to_string_fn: u32,
    pub to_json_fn: u32,
    pub equals_fn: u32,
    pub hash_fn: u32,
    pub field_getter: u32,
    pub field_setter: u32,
    pub raw_array: u32,
    pub array: u32,
    pub raw_uint8_array: u32,
    pub uint8_array: u32,
    pub closure: u32,
    pub class_vtable: u32,
    pub error_vtable: u32,
    pub error: u32,
    pub raw_bigint: u32,
    pub bigint: u32,
    pub regex_capture_array: u32,
    pub regex_match: u32,
    pub regex: u32,
    pub regex_match_box: u32,
    pub temporal_instant: u32,
    pub temporal_duration: u32,
    pub temporal_zdt: u32,
    pub raw_index_array: u32,
    pub map: u32,
    pub set: u32,
    pub url: u32,
    pub fs_stat: u32,
    pub fs_peek: u32,
    pub fs_dir_entry: u32,
    pub fs_info: u32,
    pub fs_file_writer: u32,
    pub http_response: u32,
    pub http_download_result: u32,
    pub session_entry: u32,
    pub session_page: u32,

    pub temporal_plain_date: u32,
    pub temporal_plain_time: u32,
    pub temporal_plain_date_time: u32,
    pub temporal_plain_year_month: u32,
    pub temporal_plain_month_day: u32,
}

/// Number of types [`declare_intrinsic_types`] emits — the first free type index
/// in every module.
pub const INTRINSIC_TYPE_COUNT: u32 = 50;

pub fn declare_intrinsic_types(types: &mut TypeSection) -> IntrinsicTypeIndices {
    let raw_string = 0u32;
    let vtable = 1u32;
    let object = 2u32;
    let string = 3u32;
    let boxed_number = 4u32;
    let boxed_boolean = 5u32;
    let field_names = 6u32;
    let object_fields = 7u32;
    let object_shape = 8u32;
    let to_string_fn = 9u32;
    let to_json_fn = 10u32;
    let equals_fn = 11u32;
    let hash_fn = 12u32;
    let field_getter = 13u32;
    let field_setter = 14u32;
    let raw_array = 15u32;
    let array = 16u32;
    let raw_uint8_array = 17u32;
    let uint8_array = 18u32;
    let closure = 19u32;
    // `$ClassVTable` — nominal-identity base for class vtables (SUB-631). A
    // self-referential singleton rec group `(sub $VTable (4 funcrefs, parent
    // (ref null $ClassVTable)))`. Only class vtables (and `$Error_vtable`)
    // subtype it; every other vtable stays a plain `$VTable` subtype, so a
    // `ref.cast (ref $ClassVTable)` classifies "is a class instance" and the
    // parent field carries the `extends` chain for `instanceof`'s ref.eq walk.
    let class_vtable = 20u32;
    // `$Error` class pair, one rec group `(rec $Error_vtable $Error)`. Shaped
    // byte-for-byte like the user-class emitter's output for
    // `class Error { message: string; name: string }` (see
    // `classes.rs::{vtable_subtype, struct_subtype}`) so a user
    // `class MyError extends Error` reconstructs the parent via the ordinary
    // class path and canonicalization unifies it with this pair. Fields live
    // in the object-fields payload: slot 0 = message, slot 1 = name.
    let error_vtable = 21u32;
    let error = 22u32;
    // bigint runtime types. Standalone (like `$Array` /
    // `$Uint8Array`) — they reference rec-group members (`$Object`,
    // `$VTable`) by index, no cyclic dep with this type's fields.
    let raw_bigint = 23u32;
    let bigint = 24u32;
    // regex runtime types. Two standalone (`$RegExpCaptureArray`,
    // `$RegExpMatch`) outside the `$Object` rec group so the host
    // can allocate them via `wasmtime::StructType::with_finality_and_supertype`
    // (they are not part of the recursive `$Object`/`$VTable` group). `$regex`
    // is a standard `$Object` subtype declared standalone.
    let regex_capture_array = 25u32;
    let regex_match = 26u32;
    let regex = 27u32;
    let regex_match_box = 28u32;
    // Temporal carrier types. All three standalone $Object
    // subtypes — the runtime doesn't need to construct them via
    // wasmtime's host-side `StructType::with_finality_and_supertype`
    // (host returns the carrier fields, the prelude wrapper assembles
    // the struct in Wasm), so the rec-group rules aren't a concern.
    let temporal_instant = 29u32;
    let temporal_duration = 30u32;
    let temporal_zdt = 31u32;
    let raw_index_array = 32u32;
    let map = 33u32;
    let set = 34u32;
    let url = 35u32;
    let fs_stat = 41u32;
    let fs_peek = 42u32;
    let fs_dir_entry = 43u32;
    let fs_info = 44u32;
    let fs_file_writer = 45u32;
    let http_response = 46u32;
    let http_download_result = 47u32;
    let session_entry = 48u32;
    let session_page = 49u32;

    let temporal_plain_date = 36u32;
    let temporal_plain_time = 37u32;
    let temporal_plain_date_time = 38u32;
    let temporal_plain_year_month = 39u32;
    let temporal_plain_month_day = 40u32;

    // $rawString standalone — matches the runtime host's standalone
    // ArrayType registration so cross-module canonicalization aligns.
    types.ty().array(&StorageType::I16, true);

    // $VTable / $Object / $string / boxed primitives / four method
    // sigs — closed reference cycle (the methods reference $string
    // and $Object), emitted as one rec group.
    types.ty().rec(vec![
        // $VTable — references the four method-signature types below
        // by their forthcoming indices (forward refs in a rec group).
        substruct(
            vec![
                fieldtype_ref(to_string_fn),
                fieldtype_ref(to_json_fn),
                fieldtype_ref(equals_fn),
                fieldtype_ref(hash_fn),
            ],
            None,
        ),
        substruct(vec![fieldtype_ref(vtable)], None),
        SubType {
            is_final: false,
            supertype_idx: Some(object),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        fieldtype_ref(vtable),
                        FieldType {
                            element_type: StorageType::Val(ref_to(raw_string)),
                            mutable: false,
                        },
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        SubType {
            is_final: false,
            supertype_idx: Some(object),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        fieldtype_ref(vtable),
                        FieldType {
                            element_type: StorageType::Val(ValType::F64),
                            mutable: false,
                        },
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        SubType {
            is_final: false,
            supertype_idx: Some(object),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        fieldtype_ref(vtable),
                        FieldType {
                            element_type: StorageType::Val(ValType::I32),
                            mutable: false,
                        },
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        // $field_names — `(array (ref $string))`. Per-shape canonical field-name list.
        SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Array(wasm_encoder::ArrayType(FieldType {
                    element_type: StorageType::Val(ref_to(string)),
                    mutable: false,
                })),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Array(wasm_encoder::ArrayType(FieldType {
                    element_type: StorageType::Val(ref_null(object)),
                    mutable: true,
                })),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        SubType {
            is_final: false,
            supertype_idx: Some(object),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        fieldtype_ref(vtable),
                        FieldType {
                            element_type: StorageType::Val(ref_to(field_names)),
                            mutable: false,
                        },
                        FieldType {
                            element_type: StorageType::Val(ref_to(object_fields)),
                            mutable: false,
                        },
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        },
        // $toStringFn/$toJsonFn return (ref $string) intra-group — forces $string into this rec group.
        subfunc(vec![ref_to(object)], vec![ref_to(string)]),
        subfunc(vec![ref_to(object)], vec![ref_to(string)]),
        subfunc(vec![ref_to(object), ref_to(object)], vec![ValType::I32]),
        subfunc(vec![ref_to(object)], vec![ValType::I32]),
        subfunc(
            vec![ref_to(object_shape), ref_to(string)],
            vec![ref_null(object)],
        ),
        subfunc(
            vec![ref_to(object_shape), ref_to(string), ref_null(object)],
            vec![],
        ),
    ]);

    // $rawArray standalone — slot nullable so array.new_default works; language never stores null.
    types.ty().array(
        &StorageType::Val(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(object),
        })),
        true,
    );

    // $Array field 1 is mutable — push/pop swap in a new $rawArray since WasmGC arrays aren't growable.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ref_to(raw_array)),
                        mutable: true,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $rawUint8Array standalone — packed `(array (mut i8))`. Standalone
    // (like $rawString / $rawArray) so it canonicalizes with the host
    // side's standalone `ArrayType::new` registration.
    types.ty().array(&StorageType::I8, true);

    // $Uint8Array field 1 is immutable — unlike $Array, no slot swap; mutation goes through array.set.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ref_to(raw_uint8_array)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $Closure — non-final base; a single ref.test (ref $Closure) classifies any closure regardless of signature.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![fieldtype_ref(vtable)].into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });
    // `(rec $ClassVTable)` — its own rec group (the self-referential parent
    // field needs one); see the index-table comment above.
    types.ty().rec(vec![substruct(
        vec![
            fieldtype_ref(to_string_fn),
            fieldtype_ref(to_json_fn),
            fieldtype_ref(equals_fn),
            fieldtype_ref(hash_fn),
            FieldType {
                element_type: StorageType::Val(ref_null(class_vtable)),
                mutable: false,
            },
        ],
        Some(vtable),
    )]);
    // `(rec $Error_vtable $Error)` — see the index-table comment above for the
    // shape constraint (must mirror `classes.rs::{vtable_subtype, struct_subtype}`).
    types.ty().rec(vec![
        substruct(
            vec![
                fieldtype_ref(to_string_fn),
                fieldtype_ref(to_json_fn),
                fieldtype_ref(equals_fn),
                fieldtype_ref(hash_fn),
                FieldType {
                    element_type: StorageType::Val(ref_null(class_vtable)),
                    mutable: false,
                },
            ],
            Some(class_vtable),
        ),
        substruct(
            vec![
                fieldtype_ref(error_vtable),
                fieldtype_ref(field_names),
                fieldtype_ref(object_fields),
            ],
            Some(object_shape),
        ),
    ]);

    // $rawBigInt standalone — little-endian i64 limb array; standalone so host ArrayType::new canonicalizes.
    types.ty().array(&StorageType::Val(ValType::I64), true);

    // $bigint: field 1 = sign (−1/0/1, zero has sign==0); field 2 = immutable limbs (arithmetic returns fresh bigint).
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(raw_bigint)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $RegExpCaptureArray standalone — nullable elements encode unmatched captures; standalone for host canonicalization.
    types.ty().array(
        &StorageType::Val(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(raw_string),
        })),
        true,
    );

    // $RegExpMatch — standalone final struct (NOT $Object subtype); host allocates
    // it via StructType::with_finality_and_supertype.
    // Fields: 0=match text, 1=index, 2=input, 3=numbered captures, 4=named k/v pairs, 5=next_last_index.
    types.ty().subtype(&SubType {
        is_final: true,
        supertype_idx: None,
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    FieldType {
                        element_type: StorageType::Val(ref_to(raw_string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(raw_string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(regex_capture_array)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(regex_capture_array)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $regex field 1 = externref(ChargedRegex); GC reclaim runs Drop, releasing charged bytes from TenantLimits.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ValType::Ref(RefType::EXTERNREF)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: true,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $RegExpMatchBox: fields 1=match, 2=index, 3=input, 4=numbered captures, 5=alternating name/value array.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ref_to(string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(string)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(regex_capture_array)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(regex_capture_array)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $temporal_instant: field 1 = signed seconds since epoch, field 2 = subsecond nanos (0..=999_999_999).
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $temporal_duration: years/months/weeks/days as i32; hours..nanoseconds as i64 (avoids overflow for ns).
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    }, // years
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    }, // months
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    }, // weeks
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    }, // days
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // hours
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // minutes
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // seconds
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // milliseconds
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // microseconds
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    }, // nanoseconds
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // $temporal_zdt: same (secs, nanos) as $temporal_instant plus IANA tz id in field 3.
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    FieldType {
                        element_type: StorageType::Val(ValType::I64),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_to(string)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // Generic collections use erased element slots but distinct backing
    // structs. Keeping their canonical types in every module lets runtime
    // interface guards distinguish Map and Set from unrelated host objects.
    types.ty().array(&StorageType::Val(ValType::I32), true);
    let bucket_field = FieldType {
        element_type: StorageType::Val(ref_to(raw_array)),
        mutable: true,
    };
    let order_field = FieldType {
        element_type: StorageType::Val(ref_to(raw_index_array)),
        mutable: true,
    };
    let mutable_i32 = FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: true,
    };
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    bucket_field,
                    bucket_field,
                    mutable_i32,
                    order_field,
                    mutable_i32,
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });
    types.ty().subtype(&SubType {
        is_final: false,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    bucket_field,
                    mutable_i32,
                    order_field,
                    mutable_i32,
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // `$UrlBacking` mirrors `stdlib::url::url_backing_struct`. Keeping the
    // exact final layout here lets a checked interface read distinguish URL
    // values from every other host-built `$Object` subtype before direct
    // property dispatch sees the receiver.
    types.ty().subtype(&SubType {
        is_final: true,
        supertype_idx: Some(object),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    fieldtype_ref(vtable),
                    fieldtype_ref(string),
                    fieldtype_ref(string),
                    FieldType {
                        element_type: StorageType::Val(ref_null(boxed_number)),
                        mutable: false,
                    },
                    fieldtype_ref(string),
                    FieldType {
                        element_type: StorageType::Val(ref_null(object)),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_null(string)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });

    // Temporal plain-value carriers are host-created standalone `$Object`
    // subtypes. Their immutable civil fields are all i32 values; declaring the
    // exact layouts here gives erased interface boundaries nominally precise
    // representation tests without exposing the backing structs to user code.
    let temporal_i32 = FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    };
    for civil_field_count in [3usize, 4, 7, 2] {
        let mut fields = Vec::with_capacity(civil_field_count + 1);
        fields.push(fieldtype_ref(vtable));
        fields.extend(std::iter::repeat_n(temporal_i32, civil_field_count));
        types.ty().subtype(&substruct(fields, Some(object)));
    }
    // PlainMonthDay would otherwise canonicalize with PlainYearMonth (both
    // carry two i32s). A hidden immutable marker makes the host backing
    // nominally distinct while leaving its public field indices unchanged.
    types.ty().subtype(&substruct(
        vec![
            fieldtype_ref(vtable),
            temporal_i32,
            temporal_i32,
            FieldType {
                element_type: StorageType::Val(ValType::I64),
                mutable: false,
            },
        ],
        Some(object),
    ));

    // Exact standalone carriers for direct host interfaces. The hidden i64
    // markers distinguish fs interfaces whose public payload layouts coincide.
    let host_f64 = FieldType {
        element_type: StorageType::Val(ValType::F64),
        mutable: false,
    };
    let host_i64 = FieldType {
        element_type: StorageType::Val(ValType::I64),
        mutable: false,
    };
    let host_externref = FieldType {
        element_type: StorageType::Val(ValType::EXTERNREF),
        mutable: false,
    };
    let host_object = FieldType {
        element_type: StorageType::Val(ref_null(object)),
        mutable: false,
    };
    let host_string = FieldType {
        element_type: StorageType::Val(ref_null(string)),
        mutable: false,
    };
    // fs_stat
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                fieldtype_ref(string),
                host_f64,
                host_f64,
            ],
            Some(object),
        )
    });
    // fs_peek
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                fieldtype_ref(string),
                fieldtype_ref(string),
                fieldtype_ref(string),
                host_f64,
            ],
            Some(object),
        )
    });
    // fs_dir_entry
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                fieldtype_ref(string),
                fieldtype_ref(string),
                fieldtype_ref(string),
                host_f64,
                host_i64,
            ],
            Some(object),
        )
    });
    // fs_info
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                fieldtype_ref(string),
                host_f64,
                host_f64,
                host_i64,
            ],
            Some(object),
        )
    });
    // fs_file_writer
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(vec![fieldtype_ref(vtable), host_externref], Some(object))
    });
    // http_response
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                fieldtype_ref(string),
                host_object,
                temporal_i32,
                host_f64,
                fieldtype_ref(string),
                fieldtype_ref(string),
            ],
            Some(object),
        )
    });
    // http_download_result
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![
                fieldtype_ref(vtable),
                host_f64,
                fieldtype_ref(string),
                host_f64,
                fieldtype_ref(string),
                fieldtype_ref(string),
                host_f64,
            ],
            Some(object),
        )
    });
    // session_entry
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![fieldtype_ref(vtable), fieldtype_ref(string), host_f64],
            Some(object),
        )
    });
    // session_page
    types.ty().subtype(&SubType {
        is_final: true,
        ..substruct(
            vec![fieldtype_ref(vtable), fieldtype_ref(array), host_string],
            Some(object),
        )
    });

    IntrinsicTypeIndices {
        raw_string,
        vtable,
        object,
        string,
        boxed_number,
        boxed_boolean,
        field_names,
        object_shape,
        object_fields,
        to_string_fn,
        to_json_fn,
        equals_fn,
        hash_fn,
        field_getter,
        field_setter,
        raw_array,
        array,
        raw_uint8_array,
        uint8_array,
        closure,
        class_vtable,
        error_vtable,
        error,
        raw_bigint,
        bigint,
        regex_capture_array,
        regex_match,
        regex,
        regex_match_box,
        temporal_instant,
        temporal_duration,
        temporal_zdt,
        raw_index_array,
        map,
        set,
        url,
        fs_stat,
        fs_peek,
        fs_dir_entry,
        fs_info,
        fs_file_writer,
        http_response,
        http_download_result,
        session_entry,
        session_page,
        temporal_plain_date,
        temporal_plain_time,
        temporal_plain_date_time,
        temporal_plain_year_month,
        temporal_plain_month_day,
    }
}

/// Child → parent for every intrinsic type [`declare_intrinsic_types`] gives a
/// supertype, so coercion can tell an implicit upcast from one needing a
/// `ref.cast` (`SymbolTable::ref_fits_slot`).
///
/// A second spelling of what the ordered index table above already encodes —
/// which is the price of leaving that table alone. The
/// `intrinsic_supertypes_match_declarations` test re-parses the emitted section
/// and holds the two together.
pub(crate) fn intrinsic_supertypes(
    indices: IntrinsicTypeIndices,
) -> impl IntoIterator<Item = (u32, u32)> {
    [
        (indices.string, indices.object),
        (indices.boxed_number, indices.object),
        (indices.boxed_boolean, indices.object),
        (indices.object_shape, indices.object),
        (indices.array, indices.object),
        (indices.uint8_array, indices.object),
        (indices.closure, indices.object),
        (indices.class_vtable, indices.vtable),
        (indices.error_vtable, indices.class_vtable),
        (indices.error, indices.object_shape),
        (indices.bigint, indices.object),
        (indices.regex, indices.object),
        (indices.regex_match_box, indices.object),
        (indices.temporal_instant, indices.object),
        (indices.temporal_duration, indices.object),
        (indices.temporal_zdt, indices.object),
        (indices.map, indices.object),
        (indices.set, indices.object),
        (indices.url, indices.object),
        (indices.fs_stat, indices.object),
        (indices.fs_peek, indices.object),
        (indices.fs_dir_entry, indices.object),
        (indices.fs_info, indices.object),
        (indices.fs_file_writer, indices.object),
        (indices.http_response, indices.object),
        (indices.http_download_result, indices.object),
        (indices.session_entry, indices.object),
        (indices.session_page, indices.object),
        (indices.temporal_plain_date, indices.object),
        (indices.temporal_plain_time, indices.object),
        (indices.temporal_plain_date_time, indices.object),
        (indices.temporal_plain_year_month, indices.object),
        (indices.temporal_plain_month_day, indices.object),
    ]
}

/// Append instructions building a fresh `(ref $string)` for a short
/// codegen-owned literal: string vtable + `array.new_fixed` of its UTF-16 units.
pub(crate) fn push_string_literal(
    f: &mut wasm_encoder::Function,
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
    text: &str,
) {
    use wasm_encoder::Instruction;
    f.instruction(&Instruction::GlobalGet(string_vtable_global_idx));
    let mut len = 0u32;
    for unit in text.encode_utf16() {
        f.instruction(&Instruction::I32Const(i32::from(unit)));
        len += 1;
    }
    f.instruction(&Instruction::ArrayNewFixed {
        array_type_index: intrinsics.raw_string,
        array_size: len,
    });
    f.instruction(&Instruction::StructNew(intrinsics.string));
}

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
}

fn ref_null(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(idx),
    })
}

fn fieldtype_ref(idx: u32) -> FieldType {
    FieldType {
        element_type: StorageType::Val(ref_to(idx)),
        mutable: false,
    }
}

fn substruct(fields: Vec<FieldType>, supertype: Option<u32>) -> SubType {
    SubType {
        is_final: false,
        supertype_idx: supertype,
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: fields.into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

fn subfunc(params: Vec<ValType>, results: Vec<ValType>) -> SubType {
    SubType {
        is_final: false,
        supertype_idx: None,
        composite_type: CompositeType {
            inner: CompositeInnerType::Func(FuncType::new(params, results)),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{declare_intrinsic_types, intrinsic_supertypes};
    use wasm_encoder::{Module, TypeSection};
    use wasmparser::{Parser, Payload, Validator};

    #[test]
    fn intrinsic_supertypes_match_declarations() {
        let mut module = Module::new();
        let mut types = TypeSection::new();
        let indices = declare_intrinsic_types(&mut types);
        module.section(&types);
        let bytes = module.finish();

        let mut declared: BTreeMap<u32, u32> = BTreeMap::new();
        let mut next_idx = 0u32;
        for payload in Parser::new(0).parse_all(&bytes) {
            let Payload::TypeSection(reader) = payload.expect("parse intrinsic module") else {
                continue;
            };
            for group in reader {
                for sub in group.expect("read rec group").into_types() {
                    if let Some(parent) = sub.supertype_idx {
                        let parent = parent
                            .as_module_index()
                            .expect("intrinsic supertypes are module-level indices");
                        declared.insert(next_idx, parent);
                    }
                    next_idx += 1;
                }
            }
        }

        let table: BTreeMap<u32, u32> = intrinsic_supertypes(indices).into_iter().collect();
        assert_eq!(
            table, declared,
            "intrinsic_supertypes is out of step with declare_intrinsic_types",
        );
    }

    #[test]
    fn intrinsic_types_self_contained_and_validate() {
        let mut module = Module::new();
        let mut types = TypeSection::new();
        let indices = declare_intrinsic_types(&mut types);
        module.section(&types);

        let bytes = module.finish();
        Validator::new()
            .validate_all(&bytes)
            .expect("intrinsic types validate as a stand-alone module");

        assert_eq!(indices.raw_string, 0);
        assert_eq!(indices.vtable, 1);
        assert_eq!(indices.object, 2);
        assert_eq!(indices.string, 3);
        assert_eq!(indices.boxed_number, 4);
        assert_eq!(indices.boxed_boolean, 5);
        assert_eq!(indices.field_names, 6);
        assert_eq!(indices.object_fields, 7);
        assert_eq!(indices.object_shape, 8);
        assert_eq!(indices.to_string_fn, 9);
        assert_eq!(indices.to_json_fn, 10);
        assert_eq!(indices.equals_fn, 11);
        assert_eq!(indices.hash_fn, 12);
        assert_eq!(indices.field_getter, 13);
        assert_eq!(indices.field_setter, 14);
        assert_eq!(indices.raw_array, 15);
        assert_eq!(indices.array, 16);
        assert_eq!(indices.raw_uint8_array, 17);
        assert_eq!(indices.uint8_array, 18);
        assert_eq!(indices.closure, 19);
        assert_eq!(indices.class_vtable, 20);
        assert_eq!(indices.error_vtable, 21);
        assert_eq!(indices.error, 22);
        assert_eq!(indices.raw_bigint, 23);
        assert_eq!(indices.bigint, 24);
        assert_eq!(indices.regex_capture_array, 25);
        assert_eq!(indices.regex_match, 26);
        assert_eq!(indices.regex, 27);
        assert_eq!(indices.regex_match_box, 28);
        assert_eq!(indices.temporal_instant, 29);
        assert_eq!(indices.temporal_duration, 30);
        assert_eq!(indices.temporal_zdt, 31);
        assert_eq!(indices.raw_index_array, 32);
        assert_eq!(indices.map, 33);
        assert_eq!(indices.set, 34);
        assert_eq!(indices.url, 35);
        assert_eq!(indices.temporal_plain_date, 36);
        assert_eq!(indices.temporal_plain_time, 37);
        assert_eq!(indices.temporal_plain_date_time, 38);
        assert_eq!(indices.temporal_plain_year_month, 39);
        assert_eq!(indices.temporal_plain_month_day, 40);
        assert_eq!(super::INTRINSIC_TYPE_COUNT, 50);
    }
}
