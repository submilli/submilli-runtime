//! Tests for WasmGC cross-module type canonicalization.

use std::borrow::Cow;

use interpreter::RuntimeConfig;
use wasm_encoder::{
    BlockType, CodeSection, CompositeInnerType, CompositeType, ElementSection, Elements,
    ExportKind, ExportSection, FieldType, Function, FunctionSection, HeapType, ImportSection,
    Instruction, Module, RefType, StorageType, StructType, SubType, TypeSection, ValType,
};
use wasmtime::{Instance, Linker, Module as WtModule};

const PRODUCER_MODULE_NAME: &str = "a";

fn link_and_run(a_bytes: &[u8], b_bytes: &[u8]) -> wasmtime::Result<i32> {
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine()?;
    let mut store = cfg.store(&engine, ())?;
    let a_module = WtModule::new(&engine, a_bytes)?;
    let b_module = WtModule::new(&engine, b_bytes)?;
    let mut linker = Linker::<()>::new(&engine);
    let a_instance = Instance::new(&mut store, &a_module, &[])?;
    linker.instance(&mut store, PRODUCER_MODULE_NAME, a_instance)?;
    let b_instance = linker.instantiate(&mut store, &b_module)?;
    let main = b_instance.get_typed_func::<(), i32>(&mut store, "main")?;
    main.call(&mut store, ())
}

fn link_and_run_expecting_error(a_bytes: &[u8], b_bytes: &[u8]) -> String {
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine builds");
    let mut store = cfg.store(&engine, ()).expect("store builds");
    let a_module = WtModule::new(&engine, a_bytes).expect("producer A compiles");
    let b_module = match WtModule::new(&engine, b_bytes) {
        Ok(m) => m,
        Err(e) => return format!("(B failed to compile) {e:#}"),
    };
    let mut linker = Linker::<()>::new(&engine);
    let a_instance = match Instance::new(&mut store, &a_module, &[]) {
        Ok(i) => i,
        Err(e) => return format!("(A failed to instantiate) {e:#}"),
    };
    linker
        .instance(&mut store, PRODUCER_MODULE_NAME, a_instance)
        .expect("A registers");
    match linker.instantiate(&mut store, &b_module) {
        Ok(_) => panic!("expected linking to fail, but it succeeded"),
        Err(e) => format!("{e:#}"),
    }
}

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
}

fn ref_null_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(idx),
    })
}

struct ModuleLayout {
    types: TypeSection,
    imports: ImportSection,
    functions: FunctionSection,
    code: CodeSection,
    exports: ExportSection,
    declared_funcs: Vec<u32>,
}

impl ModuleLayout {
    fn new() -> Self {
        Self {
            types: TypeSection::new(),
            imports: ImportSection::new(),
            functions: FunctionSection::new(),
            code: CodeSection::new(),
            exports: ExportSection::new(),
            declared_funcs: Vec::new(),
        }
    }

    fn finish(self) -> Vec<u8> {
        let mut module = Module::new();
        module.section(&self.types);
        module.section(&self.imports);
        module.section(&self.functions);
        module.section(&self.exports);
        // Element section (if any) must come after exports and before code.
        if !self.declared_funcs.is_empty() {
            let mut elements = ElementSection::new();
            elements.declared(Elements::Functions(Cow::Owned(self.declared_funcs)));
            module.section(&elements);
        }
        module.section(&self.code);
        module.finish()
    }
}

fn substruct(fields: Vec<FieldType>, is_final: bool) -> SubType {
    SubType {
        is_final,
        supertype_idx: None,
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

fn end(f: &mut Function) {
    f.instruction(&Instruction::End);
}

fn build_pair_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();

    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);

    m.functions.function(make_sig_idx);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(13));
    make.instruction(&Instruction::I32Const(29));
    make.instruction(&Instruction::StructNew(pair_idx));
    end(&mut make);
    m.code.function(&make);

    m.exports.export("make", ExportKind::Func, 0);
    m.finish()
}

fn build_pair_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();

    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "make",
        wasm_encoder::EntityType::Function(make_sig_idx),
    );

    m.functions.function(main_sig_idx);
    let mut main = Function::new([(1, ref_to(pair_idx))]);
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::LocalSet(0));
    main.instruction(&Instruction::LocalGet(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx,
        field_index: 0,
    });
    main.instruction(&Instruction::LocalGet(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx,
        field_index: 1,
    });
    main.instruction(&Instruction::I32Add);
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn flat_struct_returned_across_boundary() {
    let result = link_and_run(&build_pair_producer(), &build_pair_consumer())
        .expect("flat struct must round-trip across modules");
    assert_eq!(result, 13 + 29);
}

fn build_read_first_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let read_sig_idx = 1u32;
    m.types.ty().function([ref_to(pair_idx)], [ValType::I32]);

    m.functions.function(read_sig_idx);
    let mut read = Function::new(std::iter::empty());
    read.instruction(&Instruction::LocalGet(0));
    read.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx,
        field_index: 0,
    });
    end(&mut read);
    m.code.function(&read);

    m.exports.export("read_first", ExportKind::Func, 0);
    m.finish()
}

fn build_read_first_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let read_sig_idx = 1u32;
    m.types.ty().function([ref_to(pair_idx)], [ValType::I32]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "read_first",
        wasm_encoder::EntityType::Function(read_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::I32Const(101));
    main.instruction(&Instruction::I32Const(202));
    main.instruction(&Instruction::StructNew(pair_idx));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn flat_struct_passed_across_boundary() {
    let result = link_and_run(&build_read_first_producer(), &build_read_first_consumer())
        .expect("flat struct must be accepted as a parameter across modules");
    assert_eq!(result, 101);
}

fn build_mutable_pair_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: true,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);

    m.functions.function(make_sig_idx);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(7));
    make.instruction(&Instruction::I32Const(35));
    make.instruction(&Instruction::StructNew(pair_idx));
    end(&mut make);
    m.code.function(&make);

    m.exports.export("make", ExportKind::Func, 0);
    m.finish()
}

fn build_mutable_pair_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: true,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "make",
        wasm_encoder::EntityType::Function(make_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx,
        field_index: 1,
    });
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn struct_with_mutable_field_canonicalizes() {
    let result = link_and_run(
        &build_mutable_pair_producer(),
        &build_mutable_pair_consumer(),
    )
    .expect("mutable-field struct must canonicalize across modules");
    assert_eq!(result, 35);
}

fn build_nested_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let inner_idx = 0u32;
    let outer_idx = 1u32;
    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    }]);
    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ref_to(inner_idx)),
        mutable: false,
    }]);
    let make_sig_idx = 2u32;
    m.types.ty().function([], [ref_to(outer_idx)]);

    m.functions.function(make_sig_idx);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(444));
    make.instruction(&Instruction::StructNew(inner_idx));
    make.instruction(&Instruction::StructNew(outer_idx));
    end(&mut make);
    m.code.function(&make);

    m.exports.export("make", ExportKind::Func, 0);
    m.finish()
}

fn build_nested_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let inner_idx = 0u32;
    let outer_idx = 1u32;
    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    }]);
    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ref_to(inner_idx)),
        mutable: false,
    }]);
    let make_sig_idx = 2u32;
    m.types.ty().function([], [ref_to(outer_idx)]);
    let main_sig_idx = 3u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "make",
        wasm_encoder::EntityType::Function(make_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: outer_idx,
        field_index: 0,
    });
    main.instruction(&Instruction::StructGet {
        struct_type_index: inner_idx,
        field_index: 0,
    });
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn nested_struct_canonicalizes() {
    let result = link_and_run(&build_nested_producer(), &build_nested_consumer())
        .expect("nested struct must canonicalize across modules");
    assert_eq!(result, 444);
}

fn build_linked_list_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let node_idx = 0u32;
    m.types.ty().rec(vec![SubType {
        is_final: false,
        supertype_idx: None,
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_null_to(node_idx)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }]);
    let sum_sig_idx = 1u32;
    m.types
        .ty()
        .function([ref_null_to(node_idx)], [ValType::I32]);

    m.functions.function(sum_sig_idx);
    let mut sum = Function::new([(1, ref_null_to(node_idx)), (1, ValType::I32)]);
    sum.instruction(&Instruction::LocalGet(0));
    sum.instruction(&Instruction::LocalSet(1));
    sum.instruction(&Instruction::Block(BlockType::Empty));
    sum.instruction(&Instruction::Loop(BlockType::Empty));
    sum.instruction(&Instruction::LocalGet(1));
    sum.instruction(&Instruction::BrOnNull(1));
    sum.instruction(&Instruction::StructGet {
        struct_type_index: node_idx,
        field_index: 0,
    });
    sum.instruction(&Instruction::LocalGet(2));
    sum.instruction(&Instruction::I32Add);
    sum.instruction(&Instruction::LocalSet(2));
    sum.instruction(&Instruction::LocalGet(1));
    sum.instruction(&Instruction::RefAsNonNull);
    sum.instruction(&Instruction::StructGet {
        struct_type_index: node_idx,
        field_index: 1,
    });
    sum.instruction(&Instruction::LocalSet(1));
    sum.instruction(&Instruction::Br(0));
    sum.instruction(&Instruction::End);
    sum.instruction(&Instruction::End);
    sum.instruction(&Instruction::LocalGet(2));
    end(&mut sum);
    m.code.function(&sum);

    m.exports.export("sum", ExportKind::Func, 0);
    m.finish()
}

fn build_linked_list_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let node_idx = 0u32;
    m.types.ty().rec(vec![SubType {
        is_final: false,
        supertype_idx: None,
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: vec![
                    FieldType {
                        element_type: StorageType::Val(ValType::I32),
                        mutable: false,
                    },
                    FieldType {
                        element_type: StorageType::Val(ref_null_to(node_idx)),
                        mutable: false,
                    },
                ]
                .into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }]);
    let sum_sig_idx = 1u32;
    m.types
        .ty()
        .function([ref_null_to(node_idx)], [ValType::I32]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "sum",
        wasm_encoder::EntityType::Function(sum_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new([(1, ref_null_to(node_idx))]);
    main.instruction(&Instruction::I32Const(30));
    main.instruction(&Instruction::RefNull(HeapType::Concrete(node_idx)));
    main.instruction(&Instruction::StructNew(node_idx));
    main.instruction(&Instruction::LocalSet(0));
    main.instruction(&Instruction::I32Const(20));
    main.instruction(&Instruction::LocalGet(0));
    main.instruction(&Instruction::StructNew(node_idx));
    main.instruction(&Instruction::LocalSet(0));
    main.instruction(&Instruction::I32Const(10));
    main.instruction(&Instruction::LocalGet(0));
    main.instruction(&Instruction::StructNew(node_idx));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn recursive_rec_group_canonicalizes() {
    let result = link_and_run(&build_linked_list_producer(), &build_linked_list_consumer())
        .expect("recursive rec-group struct must canonicalize across modules");
    assert_eq!(result, 10 + 20 + 30);
}

fn build_mutual_rec_group<F>(extra_setup: F) -> ModuleLayout
where
    F: FnOnce(&mut ModuleLayout, u32, u32),
{
    let mut m = ModuleLayout::new();
    let x_idx = 0u32;
    let y_idx = 1u32;
    m.types.ty().rec(vec![
        SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        FieldType {
                            element_type: StorageType::Val(ValType::I32),
                            mutable: false,
                        },
                        FieldType {
                            element_type: StorageType::Val(ref_null_to(y_idx)),
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
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        FieldType {
                            element_type: StorageType::Val(ValType::I32),
                            mutable: false,
                        },
                        FieldType {
                            element_type: StorageType::Val(ref_null_to(x_idx)),
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
    ]);
    extra_setup(&mut m, x_idx, y_idx);
    m
}

#[test]
fn mutual_rec_group_same_order_canonicalizes() {
    let mut a = build_mutual_rec_group(|_, _, _| {});
    let x_idx = 0u32;
    let peek_sig_idx = 2u32;
    a.types.ty().function([ref_to(x_idx)], [ValType::I32]);
    a.functions.function(peek_sig_idx);
    let mut peek = Function::new(std::iter::empty());
    peek.instruction(&Instruction::LocalGet(0));
    peek.instruction(&Instruction::StructGet {
        struct_type_index: x_idx,
        field_index: 0,
    });
    end(&mut peek);
    a.code.function(&peek);
    a.exports.export("peek_x_value", ExportKind::Func, 0);

    let mut b = build_mutual_rec_group(|_, _, _| {});
    let x_idx = 0u32;
    let y_idx = 1u32;
    let peek_sig_idx = 2u32;
    b.types.ty().function([ref_to(x_idx)], [ValType::I32]);
    let main_sig_idx = 3u32;
    b.types.ty().function([], [ValType::I32]);
    b.imports.import(
        PRODUCER_MODULE_NAME,
        "peek_x_value",
        wasm_encoder::EntityType::Function(peek_sig_idx),
    );
    b.functions.function(main_sig_idx);
    let mut main = Function::new([(1, ref_null_to(y_idx))]);
    main.instruction(&Instruction::I32Const(99));
    main.instruction(&Instruction::RefNull(HeapType::Concrete(x_idx)));
    main.instruction(&Instruction::StructNew(y_idx));
    main.instruction(&Instruction::LocalSet(0));
    main.instruction(&Instruction::I32Const(77));
    main.instruction(&Instruction::LocalGet(0));
    main.instruction(&Instruction::StructNew(x_idx));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    b.code.function(&main);
    b.exports.export("main", ExportKind::Func, 1);

    let result = link_and_run(&a.finish(), &b.finish())
        .expect("mutually recursive rec group with matching order must canonicalize");
    assert_eq!(result, 77);
}

fn build_mutual_rec_group_reversed<F>(extra_setup: F) -> ModuleLayout
where
    F: FnOnce(&mut ModuleLayout, u32, u32),
{
    let mut m = ModuleLayout::new();
    let y_idx = 0u32;
    let x_idx = 1u32;
    m.types.ty().rec(vec![
        SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        FieldType {
                            element_type: StorageType::Val(ValType::I32),
                            mutable: false,
                        },
                        FieldType {
                            element_type: StorageType::Val(ref_null_to(x_idx)),
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
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        FieldType {
                            element_type: StorageType::Val(ValType::I32),
                            mutable: false,
                        },
                        FieldType {
                            element_type: StorageType::Val(ref_null_to(y_idx)),
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
    ]);
    extra_setup(&mut m, x_idx, y_idx);
    m
}

#[test]
fn negative_rec_group_order_mismatch() {
    let mut a = build_mutual_rec_group(|_, _, _| {});
    let x_idx = 0u32;
    let peek_sig_idx = 2u32;
    a.types.ty().function([ref_to(x_idx)], [ValType::I32]);
    a.functions.function(peek_sig_idx);
    let mut peek = Function::new(std::iter::empty());
    peek.instruction(&Instruction::LocalGet(0));
    peek.instruction(&Instruction::StructGet {
        struct_type_index: x_idx,
        field_index: 0,
    });
    end(&mut peek);
    a.code.function(&peek);
    a.exports.export("peek_x_value", ExportKind::Func, 0);

    let mut b = build_mutual_rec_group_reversed(|_, _, _| {});
    let x_idx_b = 1u32;
    let peek_sig_idx_b = 2u32;
    b.types.ty().function([ref_to(x_idx_b)], [ValType::I32]);
    let main_sig_idx = 3u32;
    b.types.ty().function([], [ValType::I32]);
    b.imports.import(
        PRODUCER_MODULE_NAME,
        "peek_x_value",
        wasm_encoder::EntityType::Function(peek_sig_idx_b),
    );
    b.functions.function(main_sig_idx);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::I32Const(0));
    main.instruction(&Instruction::RefNull(HeapType::Abstract {
        shared: false,
        ty: wasm_encoder::AbstractHeapType::None,
    }));
    main.instruction(&Instruction::StructNew(x_idx_b));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    b.code.function(&main);
    b.exports.export("main", ExportKind::Func, 1);

    let err = link_and_run_expecting_error(&a.finish(), &b.finish());
    eprintln!("[negative: rec group order mismatch (A=[X,Y] vs B=[Y,X])]\n{err}\n");
    assert!(!err.is_empty(), "expected an error, got empty string");
}

fn build_get_cb_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let cb_idx = 0u32;
    m.types.ty().function([ValType::I32], [ValType::I32]);
    let get_cb_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(cb_idx)]);

    m.functions.function(cb_idx);
    let mut double_fn = Function::new(std::iter::empty());
    double_fn.instruction(&Instruction::LocalGet(0));
    double_fn.instruction(&Instruction::I32Const(2));
    double_fn.instruction(&Instruction::I32Mul);
    end(&mut double_fn);
    m.code.function(&double_fn);

    m.functions.function(get_cb_sig_idx);
    let mut get_cb = Function::new(std::iter::empty());
    get_cb.instruction(&Instruction::RefFunc(0));
    end(&mut get_cb);
    m.code.function(&get_cb);

    // function 0 needs to be in the declared elements set so RefFunc works
    m.declared_funcs.push(0);

    m.exports.export("get_cb", ExportKind::Func, 1);
    m.finish()
}

fn build_get_cb_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let cb_idx = 0u32;
    m.types.ty().function([ValType::I32], [ValType::I32]);
    let get_cb_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(cb_idx)]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "get_cb",
        wasm_encoder::EntityType::Function(get_cb_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::I32Const(21));
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::CallRef(cb_idx));
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn function_ref_returned_by_producer() {
    let result = link_and_run(&build_get_cb_producer(), &build_get_cb_consumer())
        .expect("function-ref must canonicalize and be callable across modules");
    assert_eq!(result, 42);
}

fn build_apply_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let cb_idx = 0u32;
    m.types.ty().function([ValType::I32], [ValType::I32]);
    let apply_sig_idx = 1u32;
    m.types
        .ty()
        .function([ref_to(cb_idx), ValType::I32], [ValType::I32]);

    m.functions.function(apply_sig_idx);
    let mut apply = Function::new(std::iter::empty());
    apply.instruction(&Instruction::LocalGet(1));
    apply.instruction(&Instruction::LocalGet(0));
    apply.instruction(&Instruction::CallRef(cb_idx));
    end(&mut apply);
    m.code.function(&apply);

    m.exports.export("apply", ExportKind::Func, 0);
    m.finish()
}

fn build_apply_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let cb_idx = 0u32;
    m.types.ty().function([ValType::I32], [ValType::I32]);
    let apply_sig_idx = 1u32;
    m.types
        .ty()
        .function([ref_to(cb_idx), ValType::I32], [ValType::I32]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "apply",
        wasm_encoder::EntityType::Function(apply_sig_idx),
    );
    m.functions.function(cb_idx);
    let mut triple = Function::new(std::iter::empty());
    triple.instruction(&Instruction::LocalGet(0));
    triple.instruction(&Instruction::I32Const(3));
    triple.instruction(&Instruction::I32Mul);
    end(&mut triple);
    m.code.function(&triple);

    m.functions.function(main_sig_idx);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::RefFunc(1));
    main.instruction(&Instruction::I32Const(14));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    m.code.function(&main);

    m.declared_funcs.push(1);
    m.exports.export("main", ExportKind::Func, 2);
    m.finish()
}

#[test]
fn function_ref_produced_by_consumer() {
    let result = link_and_run(&build_apply_producer(), &build_apply_consumer())
        .expect("consumer-produced function ref must be callable in producer");
    assert_eq!(result, 42);
}

fn build_closure_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let env_idx = 0u32;
    let cb_idx = 1u32;
    let closure_idx = 2u32;
    let apply_sig_idx = 3u32;

    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    }]);
    m.types
        .ty()
        .function([ref_to(env_idx), ValType::I32], [ValType::I32]);
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ref_to(env_idx)),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ref_to(cb_idx)),
            mutable: false,
        },
    ]);
    m.types
        .ty()
        .function([ref_to(closure_idx), ValType::I32], [ValType::I32]);

    m.functions.function(apply_sig_idx);
    let mut apply = Function::new(std::iter::empty());
    apply.instruction(&Instruction::LocalGet(0));
    apply.instruction(&Instruction::StructGet {
        struct_type_index: closure_idx,
        field_index: 0,
    });
    apply.instruction(&Instruction::LocalGet(1));
    apply.instruction(&Instruction::LocalGet(0));
    apply.instruction(&Instruction::StructGet {
        struct_type_index: closure_idx,
        field_index: 1,
    });
    apply.instruction(&Instruction::CallRef(cb_idx));
    end(&mut apply);
    m.code.function(&apply);

    m.exports.export("apply", ExportKind::Func, 0);
    m.finish()
}

fn build_closure_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let env_idx = 0u32;
    let cb_idx = 1u32;
    let closure_idx = 2u32;
    let apply_sig_idx = 3u32;
    let main_sig_idx = 4u32;

    m.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    }]);
    m.types
        .ty()
        .function([ref_to(env_idx), ValType::I32], [ValType::I32]);
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ref_to(env_idx)),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ref_to(cb_idx)),
            mutable: false,
        },
    ]);
    m.types
        .ty()
        .function([ref_to(closure_idx), ValType::I32], [ValType::I32]);
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "apply",
        wasm_encoder::EntityType::Function(apply_sig_idx),
    );

    m.functions.function(cb_idx);
    let mut cb_body = Function::new(std::iter::empty());
    cb_body.instruction(&Instruction::LocalGet(0));
    cb_body.instruction(&Instruction::StructGet {
        struct_type_index: env_idx,
        field_index: 0,
    });
    cb_body.instruction(&Instruction::LocalGet(1));
    cb_body.instruction(&Instruction::I32Add);
    end(&mut cb_body);
    m.code.function(&cb_body);

    m.functions.function(main_sig_idx);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::I32Const(100));
    main.instruction(&Instruction::StructNew(env_idx));
    main.instruction(&Instruction::RefFunc(1));
    main.instruction(&Instruction::StructNew(closure_idx));
    main.instruction(&Instruction::I32Const(23));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    m.code.function(&main);

    m.declared_funcs.push(1);
    m.exports.export("main", ExportKind::Func, 2);
    m.finish()
}

#[test]
fn closure_struct_of_env_and_fn_ref() {
    let result = link_and_run(&build_closure_producer(), &build_closure_consumer())
        .expect("closure-shaped struct must canonicalize across modules");
    assert_eq!(result, 100 + 23);
}

fn build_vtable_producer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let add_idx = 0u32;
    let mul_idx = 1u32;
    let vt_idx = 2u32;
    let dispatch_sig_idx = 3u32;

    m.types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    m.types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ref_to(add_idx)),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ref_to(mul_idx)),
            mutable: false,
        },
    ]);
    m.types
        .ty()
        .function([ref_to(vt_idx), ValType::I32, ValType::I32], [ValType::I32]);

    m.functions.function(dispatch_sig_idx);
    let mut dispatch = Function::new(std::iter::empty());
    dispatch.instruction(&Instruction::LocalGet(1));
    dispatch.instruction(&Instruction::LocalGet(2));
    dispatch.instruction(&Instruction::LocalGet(0));
    dispatch.instruction(&Instruction::StructGet {
        struct_type_index: vt_idx,
        field_index: 0,
    });
    dispatch.instruction(&Instruction::CallRef(add_idx));
    end(&mut dispatch);
    m.code.function(&dispatch);

    m.exports.export("dispatch_add", ExportKind::Func, 0);
    m.finish()
}

fn build_vtable_consumer() -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let add_idx = 0u32;
    let mul_idx = 1u32;
    let vt_idx = 2u32;
    let dispatch_sig_idx = 3u32;
    let main_sig_idx = 4u32;

    m.types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    m.types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ref_to(add_idx)),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ref_to(mul_idx)),
            mutable: false,
        },
    ]);
    m.types
        .ty()
        .function([ref_to(vt_idx), ValType::I32, ValType::I32], [ValType::I32]);
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "dispatch_add",
        wasm_encoder::EntityType::Function(dispatch_sig_idx),
    );

    m.functions.function(add_idx);
    let mut my_add = Function::new(std::iter::empty());
    my_add.instruction(&Instruction::LocalGet(0));
    my_add.instruction(&Instruction::LocalGet(1));
    my_add.instruction(&Instruction::I32Add);
    end(&mut my_add);
    m.code.function(&my_add);

    m.functions.function(mul_idx);
    let mut my_mul = Function::new(std::iter::empty());
    my_mul.instruction(&Instruction::LocalGet(0));
    my_mul.instruction(&Instruction::LocalGet(1));
    my_mul.instruction(&Instruction::I32Mul);
    end(&mut my_mul);
    m.code.function(&my_mul);

    m.functions.function(main_sig_idx);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::RefFunc(1));
    main.instruction(&Instruction::RefFunc(2));
    main.instruction(&Instruction::StructNew(vt_idx));
    main.instruction(&Instruction::I32Const(19));
    main.instruction(&Instruction::I32Const(23));
    main.instruction(&Instruction::Call(0));
    end(&mut main);
    m.code.function(&main);

    m.declared_funcs.push(1);
    m.declared_funcs.push(2);
    m.exports.export("main", ExportKind::Func, 3);
    m.finish()
}

#[test]
fn vtable_struct_of_multiple_fn_refs() {
    let result = link_and_run(&build_vtable_producer(), &build_vtable_consumer())
        .expect("vtable struct must canonicalize across modules");
    assert_eq!(result, 19 + 23);
}

fn build_pair_producer_with_field_mut(mutable_first: bool) -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: mutable_first,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);

    m.functions.function(make_sig_idx);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(1));
    make.instruction(&Instruction::I32Const(2));
    make.instruction(&Instruction::StructNew(pair_idx));
    end(&mut make);
    m.code.function(&make);

    m.exports.export("make", ExportKind::Func, 0);
    m.finish()
}

#[test]
fn negative_mismatched_mutability() {
    let a = build_pair_producer_with_field_mut(false);
    let b = build_consumer_calling_make_returning_pair_mut(true);
    let err = link_and_run_expecting_error(&a, &b);
    eprintln!("[negative: mismatched mutability]\n{err}\n");
    assert!(
        err.to_lowercase().contains("incompatible")
            || err.to_lowercase().contains("mismatch")
            || err.to_lowercase().contains("type"),
        "expected a type-mismatch error, got: {err}"
    );
}

fn build_consumer_calling_make_returning_pair_mut(mutable_first: bool) -> Vec<u8> {
    let mut m = ModuleLayout::new();
    let pair_idx = 0u32;
    m.types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: mutable_first,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ]);
    let make_sig_idx = 1u32;
    m.types.ty().function([], [ref_to(pair_idx)]);
    let main_sig_idx = 2u32;
    m.types.ty().function([], [ValType::I32]);

    m.imports.import(
        PRODUCER_MODULE_NAME,
        "make",
        wasm_encoder::EntityType::Function(make_sig_idx),
    );
    m.functions.function(main_sig_idx);

    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx,
        field_index: 0,
    });
    end(&mut main);
    m.code.function(&main);

    m.exports.export("main", ExportKind::Func, 1);
    m.finish()
}

#[test]
fn negative_mismatched_field_count() {
    let mut a_layout = ModuleLayout::new();
    let pair_idx_a = 0u32;
    a_layout.types.ty().struct_([FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: false,
    }]);
    let make_sig_a = 1u32;
    a_layout.types.ty().function([], [ref_to(pair_idx_a)]);
    a_layout.functions.function(make_sig_a);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(1));
    make.instruction(&Instruction::StructNew(pair_idx_a));
    end(&mut make);
    a_layout.code.function(&make);
    a_layout.exports.export("make", ExportKind::Func, 0);

    let b = build_consumer_calling_make_returning_pair_mut(false);

    let err = link_and_run_expecting_error(&a_layout.finish(), &b);
    eprintln!("[negative: mismatched field count]\n{err}\n");
    assert!(!err.is_empty(), "expected an error, got empty string");
}

#[test]
fn negative_mismatched_finality() {
    let mut a = ModuleLayout::new();
    let pair_idx_a = 0u32;
    a.types.ty().subtype(&substruct(
        vec![
            FieldType {
                element_type: StorageType::Val(ValType::I32),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::I32),
                mutable: false,
            },
        ],
        false,
    ));
    let make_sig_a = 1u32;
    a.types.ty().function([], [ref_to(pair_idx_a)]);
    a.functions.function(make_sig_a);
    let mut make = Function::new(std::iter::empty());
    make.instruction(&Instruction::I32Const(1));
    make.instruction(&Instruction::I32Const(2));
    make.instruction(&Instruction::StructNew(pair_idx_a));
    end(&mut make);
    a.code.function(&make);
    a.exports.export("make", ExportKind::Func, 0);

    let mut b = ModuleLayout::new();
    let pair_idx_b = 0u32;
    b.types.ty().subtype(&substruct(
        vec![
            FieldType {
                element_type: StorageType::Val(ValType::I32),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::I32),
                mutable: false,
            },
        ],
        true,
    ));
    let make_sig_b = 1u32;
    b.types.ty().function([], [ref_to(pair_idx_b)]);
    let main_sig_b = 2u32;
    b.types.ty().function([], [ValType::I32]);
    b.imports.import(
        PRODUCER_MODULE_NAME,
        "make",
        wasm_encoder::EntityType::Function(make_sig_b),
    );
    b.functions.function(main_sig_b);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::StructGet {
        struct_type_index: pair_idx_b,
        field_index: 0,
    });
    end(&mut main);
    b.code.function(&main);
    b.exports.export("main", ExportKind::Func, 1);

    let err = link_and_run_expecting_error(&a.finish(), &b.finish());
    eprintln!("[negative: mismatched finality (non-final vs final)]\n{err}\n");
    assert!(!err.is_empty(), "expected an error, got empty string");
}

#[test]
fn negative_mismatched_function_ref_signature() {
    let a = build_get_cb_producer();

    let mut b = ModuleLayout::new();
    let cb_idx = 0u32;
    b.types.ty().function([ValType::I64], [ValType::I32]);
    let get_cb_sig_idx = 1u32;
    b.types.ty().function([], [ref_to(cb_idx)]);
    let main_sig_idx = 2u32;
    b.types.ty().function([], [ValType::I32]);
    b.imports.import(
        PRODUCER_MODULE_NAME,
        "get_cb",
        wasm_encoder::EntityType::Function(get_cb_sig_idx),
    );
    b.functions.function(main_sig_idx);
    let mut main = Function::new(std::iter::empty());
    main.instruction(&Instruction::I64Const(21));
    main.instruction(&Instruction::Call(0));
    main.instruction(&Instruction::CallRef(cb_idx));
    end(&mut main);
    b.code.function(&main);
    b.exports.export("main", ExportKind::Func, 1);

    let err = link_and_run_expecting_error(&a, &b.finish());
    eprintln!("[negative: mismatched function-ref signature]\n{err}\n");
    assert!(!err.is_empty(), "expected an error, got empty string");
}
