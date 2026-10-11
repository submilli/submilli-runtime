//! Smoke test for the `$Object` + 4-slot `$VTable` design: subtype upcasts,
//! `ref.cast` downcasts, and `call_ref` through a vtable.

use wasm_encoder::{
    CodeSection, CompositeInnerType, CompositeType, ConstExpr, ElementSection, Elements,
    ExportKind, ExportSection, FieldType, Function, FunctionSection, GlobalSection, GlobalType,
    HeapType, Instruction, Module, RefType, StorageType, StructType, SubType, TypeSection, ValType,
};
use wasmtime::{Linker, Module as WtModule};

use submilli_engine::RuntimeConfig;

// Outside the rec group:
const STRING_IDX: u32 = 0;
// Rec group, in declaration order:
const VTABLE_IDX: u32 = 1;
const OBJECT_IDX: u32 = 2;
const TO_STRING_FN_IDX: u32 = 3;
const TO_JSON_FN_IDX: u32 = 4;
const EQUALS_FN_IDX: u32 = 5;
const HASH_FN_IDX: u32 = 6;
// After the rec group:
const POINT_IDX: u32 = 7;
const MAIN_SIG_IDX: u32 = 8;

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
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
            inner: CompositeInnerType::Func(wasm_encoder::FuncType::new(params, results)),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

fn build_module() -> Vec<u8> {
    let mut module = Module::new();

    let mut types = TypeSection::new();

    types.ty().array(&StorageType::I16, true);

    // 1..=6: rec group — types cross-reference each other by absolute module index.
    types.ty().rec(vec![
        substruct(
            vec![
                FieldType {
                    element_type: StorageType::Val(ref_to(TO_STRING_FN_IDX)),
                    mutable: false,
                },
                FieldType {
                    element_type: StorageType::Val(ref_to(TO_JSON_FN_IDX)),
                    mutable: false,
                },
                FieldType {
                    element_type: StorageType::Val(ref_to(EQUALS_FN_IDX)),
                    mutable: false,
                },
                FieldType {
                    element_type: StorageType::Val(ref_to(HASH_FN_IDX)),
                    mutable: false,
                },
            ],
            None,
        ),
        // non-final so user types can sub it
        substruct(
            vec![FieldType {
                element_type: StorageType::Val(ref_to(VTABLE_IDX)),
                mutable: false,
            }],
            None,
        ),
        subfunc(vec![ref_to(OBJECT_IDX)], vec![ref_to(STRING_IDX)]),
        subfunc(vec![ref_to(OBJECT_IDX)], vec![ref_to(STRING_IDX)]),
        subfunc(
            vec![ref_to(OBJECT_IDX), ref_to(OBJECT_IDX)],
            vec![ValType::I32],
        ),
        subfunc(vec![ref_to(OBJECT_IDX)], vec![ValType::I32]),
    ]);

    types.ty().subtype(&substruct(
        vec![
            FieldType {
                element_type: StorageType::Val(ref_to(VTABLE_IDX)),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::F64),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::F64),
                mutable: false,
            },
        ],
        Some(OBJECT_IDX),
    ));

    types.ty().function([], [ValType::I32]);

    module.section(&types);

    // Functions 0..=4: toString, toJson, equals, hash, main.
    let mut functions = FunctionSection::new();
    functions.function(TO_STRING_FN_IDX);
    functions.function(TO_JSON_FN_IDX);
    functions.function(EQUALS_FN_IDX);
    functions.function(HASH_FN_IDX);
    functions.function(MAIN_SIG_IDX);
    module.section(&functions);

    let mut globals = GlobalSection::new();
    let vt_init = ConstExpr::extended([
        Instruction::RefFunc(0),
        Instruction::RefFunc(1),
        Instruction::RefFunc(2),
        Instruction::RefFunc(3),
        Instruction::StructNew(VTABLE_IDX),
    ]);
    globals.global(
        GlobalType {
            val_type: ref_to(VTABLE_IDX),
            mutable: false,
            shared: false,
        },
        &vt_init,
    );
    module.section(&globals);

    let mut exports = ExportSection::new();
    exports.export("main", ExportKind::Func, 4);
    module.section(&exports);

    // ref.func requires a declarative element segment.
    let mut elements = ElementSection::new();
    elements.declared(Elements::Functions(std::borrow::Cow::Owned(vec![
        0, 1, 2, 3,
    ])));
    module.section(&elements);

    let mut code = CodeSection::new();
    code.function(&point_to_string_body());
    code.function(&trivial_string_returning_body());
    code.function(&trivial_two_obj_returning_body(0));
    code.function(&trivial_one_obj_returning_body(0));
    code.function(&main_body());
    module.section(&code);

    module.finish()
}

fn point_to_string_body() -> Function {
    let mut f = Function::new([(1, ValType::I32)]);
    f.instruction(&Instruction::LocalGet(0));
    // ref.cast (ref $Point) — traps on mismatch (never fires here).
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(POINT_IDX)));
    f.instruction(&Instruction::StructGet {
        struct_type_index: POINT_IDX,
        field_index: 1,
    });
    f.instruction(&Instruction::I32TruncSatF64S);
    f.instruction(&Instruction::LocalSet(1));
    f.instruction(&Instruction::I32Const(0x41));
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::ArrayNew(STRING_IDX));
    f.instruction(&Instruction::End);
    f
}

/// Stub body returning a trivial empty string. Used for `point_toJson`
/// to keep the vtable populated; not exercised by the test.
fn trivial_string_returning_body() -> Function {
    let mut f = Function::new(std::iter::empty());
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::ArrayNew(STRING_IDX));
    f.instruction(&Instruction::End);
    f
}

/// Stub body for `point_equals` — two ref params, i32 result.
fn trivial_two_obj_returning_body(_which_param: u32) -> Function {
    let mut f = Function::new(std::iter::empty());
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::End);
    f
}

/// Stub body for `point_hash` — one ref param, i32 result.
fn trivial_one_obj_returning_body(_seed: u32) -> Function {
    let mut f = Function::new(std::iter::empty());
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::End);
    f
}

/// `main` — builds `$Point { vt, x: 5.0, y: 0.0 }`, treats it as
/// `(ref $Object)` *via subtyping alone*, walks the vtable, calls
/// `toString`, and returns the result string's length.
fn main_body() -> Function {
    // Local typed as $Object so storing $Point exercises the subtype-upcast path.
    let obj_local: u32 = 0;
    let mut f = Function::new([(1, ref_to(OBJECT_IDX))]);
    f.instruction(&Instruction::GlobalGet(0));
    f.instruction(&Instruction::F64Const(5.0f64.into()));
    f.instruction(&Instruction::F64Const(0.0f64.into()));
    f.instruction(&Instruction::StructNew(POINT_IDX));
    // Implicit upcast: $Point <: $Object.
    f.instruction(&Instruction::LocalSet(obj_local));

    f.instruction(&Instruction::LocalGet(obj_local));
    f.instruction(&Instruction::LocalGet(obj_local));
    f.instruction(&Instruction::StructGet {
        struct_type_index: OBJECT_IDX,
        field_index: 0,
    });
    f.instruction(&Instruction::StructGet {
        struct_type_index: VTABLE_IDX,
        field_index: 0,
    });
    f.instruction(&Instruction::CallRef(TO_STRING_FN_IDX));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::End);
    f
}

#[test]
fn point_dispatches_through_object_vtable() {
    let bytes = build_module();

    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let mut store = cfg.store(&engine, ()).expect("store");
    let module = WtModule::new(&engine, &bytes).expect("module compiles");
    let linker = Linker::<()>::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    let main = instance
        .get_typed_func::<(), i32>(&mut store, "main")
        .expect("main: () -> i32");
    let len = main.call(&mut store, ()).expect("main does not trap");
    // x = 5.0 → trunc_sat → 5 → array of length 5.
    assert_eq!(len, 5);
}
