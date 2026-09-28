//! Compiler failures below exercise source limits and injected invalid codegen state.
use interpreter::codegen::{
    closures::{self, ClosureSig},
    symbol_table::SymbolTable,
};
use interpreter::compile::{compile_package_checked, compile_script_checked};
use interpreter::compiler_error::{CompilerFailure, CompilerStage};
use interpreter::{FileId, ModulePath, PackageSourceModule, Type};

fn signature(arity: usize, ret: Type) -> Type {
    Type::Function {
        params: vec![Type::Number; arity],
        ret: Box::new(ret),
        has_rest: false,
        predicate: None,
    }
}

fn assert_limit(error: CompilerFailure) {
    assert!(
        matches!(
            error,
            CompilerFailure::Limit {
                stage: CompilerStage::Codegen,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("256"), "{error}");
    assert!(error.to_string().contains("255"), "{error}");
}

fn assert_internal(error: CompilerFailure) {
    assert!(
        matches!(
            error,
            CompilerFailure::Internal {
                stage: CompilerStage::Codegen,
                span: None,
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn classification_and_symbol_lowering_preserve_failure_categories() {
    for ret in [Type::Number, Type::Void] {
        assert_limit(closures::classify(&signature(256, ret.clone())).unwrap_err());
        assert_eq!(
            closures::classify(&signature(255, ret.clone())).unwrap(),
            ClosureSig {
                arity: 255,
                is_void: ret.is_void()
            }
        );
    }
    assert_internal(closures::classify(&Type::Number).unwrap_err());
    let mut symbols = SymbolTable::default();
    let supported = signature(1, Type::Number);
    assert_internal(symbols.value_type(&supported).unwrap_err());
    assert_limit(
        symbols
            .value_type(&signature(256, Type::Number))
            .unwrap_err(),
    );
    assert_limit(
        symbols
            .slot_value_type(&signature(256, Type::Number))
            .unwrap_err(),
    );
    assert_limit(
        symbols
            .wasm_result(&signature(256, Type::Number))
            .unwrap_err(),
    );
    assert_limit(
        symbols
            .slot_wasm_result(&signature(256, Type::Number))
            .unwrap_err(),
    );
    assert_limit(
        symbols
            .host_value_type(&signature(256, Type::Number))
            .unwrap_err(),
    );
    assert_internal(symbols.value_type(&Type::String).unwrap_err());
    assert_internal(symbols.value_type(&Type::Void).unwrap_err());
    assert_internal(symbols.value_type(&Type::Union(vec![])).unwrap_err());
    symbols.record_closure_struct_type(
        ClosureSig {
            arity: 1,
            is_void: false,
        },
        7,
    );
    assert_eq!(
        symbols.value_type(&supported).unwrap(),
        wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(7)
        })
    );
    assert_eq!(
        symbols.value_type(&Type::Number).unwrap(),
        wasm_encoder::ValType::F64
    );
}

#[test]
fn closure_registration_requires_intrinsics_before_emitting_types() {
    let mut types = wasm_encoder::TypeSection::new();
    let mut symbols = SymbolTable::default();
    let mut next = 0;
    assert_internal(
        closures::emit_func_and_struct_types(
            [ClosureSig {
                arity: 1,
                is_void: false,
            }],
            &mut types,
            &mut symbols,
            &mut next,
        )
        .unwrap_err(),
    );
    assert_eq!(types.len(), 0);
    assert_eq!(next, 0);
}

#[test]
fn oversized_sources_return_no_artifact_and_allow_a_healthy_followup() {
    let params = (0..256)
        .map(|i| format!("a{i}: number"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let cases = [
        format!(
            "function main(): number {{ const f = ({params}): number => a255; return f({args}); }}"
        ),
        format!(
            "function f({params}): number {{ return a255; }} function main(): number {{ return f({args}); }}"
        ),
        format!(
            "function f({params}): number {{ return a255; }} function main(): number {{ const g = f; return g({args}); }}"
        ),
        format!(
            "class C {{ f({params}): number {{ return a255; }} }} function main(): number {{ return new C().f({args}); }}"
        ),
        format!(
            "function main(): number {{ const f = ({params}): void => {{}}; f({args}); return 0; }}"
        ),
    ];
    for (index, source) in cases.into_iter().enumerate() {
        let error = compile_script_checked(&source, "limit.ts", FileId(0), &[], &[]).unwrap_err();
        let fatal = error.fatal.expect("codegen limit remains typed");
        if index == 0 {
            let CompilerFailure::Limit {
                span: Some(span), ..
            } = &fatal
            else {
                panic!("local closure limit must retain its source span: {fatal:?}");
            };
            assert_eq!(span.file, FileId(0));
            assert!(span.start < span.end && span.end as usize <= source.len());
            assert!(source[span.start as usize..span.end as usize].contains("const f"));
        }
        assert_limit(fatal);
        compile_script_checked(
            "function main(): number { return 42; }",
            "healthy.ts",
            FileId(0),
            &[],
            &[],
        )
        .unwrap();
    }
    let source = format!(
        "export function make(): ({params}) => number {{ return ({params}): number => a255; }}"
    );
    let error = compile_package_checked(
        "oversized",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &source,
        }],
        &[],
    )
    .unwrap_err();
    assert!(error.fatal.is_some(), "{error:?}");
    assert_limit(error.fatal.expect("package codegen limit remains typed"));
    compile_package_checked(
        "healthy",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export function f(): number { return 42; }",
        }],
        &[],
    )
    .unwrap();
}

#[test]
fn closure_index_exhaustion_is_reported_before_mutation() {
    let mut next = u32::MAX - 2;
    let error = closures::allocate_methods(&mut next).unwrap_err();
    assert!(matches!(error, CompilerFailure::Limit { .. }));
    assert_eq!(next, u32::MAX - 2);
    let mut types = wasm_encoder::TypeSection::new();
    let mut symbols = SymbolTable::default();
    next = u32::MAX;
    let error = closures::emit_env_types(&[], &mut types, &mut symbols, &mut next).unwrap_err();
    assert!(matches!(error, CompilerFailure::Limit { .. }));
    assert_eq!(next, u32::MAX);
    assert_eq!(types.len(), 0);
}
