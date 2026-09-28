//! Source validation must run during check, before lowering to Wasm.
use interpreter::compile::{compile_package_checked, compile_script_checked, typecheck_checked};
use interpreter::{Diagnostic, FileId, ModulePath, PackageSourceModule, Severity, Type, ValueKind};

fn parameters(arity: usize) -> String {
    (0..arity)
        .map(|i| format!("a{i}: number"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn assert_limit(diagnostics: &[Diagnostic], source: &str, file: FileId) {
    let limits: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("256 parameter slots"))
        .collect();
    assert!(!limits.is_empty(), "{diagnostics:?}");
    for diagnostic in limits {
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("maximum is 255"));
        assert!(!diagnostic.span.text(source, file).unwrap().is_empty());
        assert!(
            diagnostic
                .help
                .iter()
                .any(|h| h.contains("object") && h.contains("rest"))
        );
    }
}

#[test]
fn check_enforces_boundary_for_all_source_signature_forms() {
    for arity in [255, 256] {
        let p = parameters(arity);
        let last = arity - 1;
        let cases = [
            format!("function f({p}): number {{ return a{last}; }}"),
            format!("function f<T>({p}): number {{ return a{last}; }}"),
            format!("interface I {{ f({p}): number; }}"),
            format!("interface I {{ ({p}): number; }}"),
            format!("type F = ({p}) => number;"),
            format!("type F = {{ nested: Array<({p}) => void> }};"),
            format!("class C {{ f({p}): number {{ return a{last}; }} }}"),
            format!("class C {{ static f({p}): number {{ return a{last}; }} }}"),
            format!("function f(a: {{ callback: ({p}) => number }}): void {{}}"),
            format!("function f(): void {{ const g = ({p}): number => a{last}; }}"),
            format!(
                "function f(): void {{ const g = function({p}): number {{ return a{last}; }}; }}"
            ),
            format!("function f(): void {{ function g({p}): number {{ return a{last}; }} }}"),
            format!(
                "function f({}, ...rest: number[]): void {{}}",
                parameters(arity - 1)
            ),
        ];
        for (index, declaration) in cases.iter().enumerate() {
            let source = format!("{declaration}\nfunction main(): number {{ return 42; }}");
            if arity == 255 {
                typecheck_checked(&source, FileId(7))
                    .unwrap_or_else(|e| panic!("case {index}: {e:?}"));
                continue;
            }
            let error = typecheck_checked(&source, FileId(7)).unwrap_err();
            assert!(error.fatal.is_none(), "case {index}: {error:?}");
            assert_limit(&error.diagnostics, &source, FileId(7));
            typecheck_checked("function main(): number { return 42; }", FileId(7)).unwrap();
        }
    }
}

#[test]
fn repeated_annotation_resolution_reports_one_limit() {
    let source = format!(
        "type F = ({}) => number; function main(): number {{ return 42; }}",
        parameters(256)
    );
    let error = typecheck_checked(&source, FileId(0)).unwrap_err();
    assert_eq!(
        error
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("parameter slots"))
            .count(),
        1,
        "{error:?}"
    );
}

#[test]
fn constructors_keep_their_own_abi() {
    let source = format!(
        "class C {{ constructor({}) {{}} }} function main(): number {{ return 42; }}",
        parameters(256)
    );
    typecheck_checked(&source, FileId(0)).unwrap();
    compile_script_checked(&source, "constructor.ts", FileId(0), &[], &[]).unwrap();
}

fn imported_declaration() -> interpreter::PackageDeclaration {
    compile_package_checked("aritydep", ModulePath::from("lib"), &[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export function bad(a: number): number { return a; } export function healthy(): number { return 42; }",
    }], &[]).unwrap().declaration
}

#[test]
fn imported_signatures_are_checked_on_use_and_unused_surface_is_allowed() {
    let mut declaration = imported_declaration();
    let ValueKind::Function { params, .. } = &mut declaration.values.get_mut("bad").unwrap().kind
    else {
        panic!("function")
    };
    *params = (0..256)
        .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    for body in [
        format!("return bad({args});"),
        "const f = bad; return 0;".into(),
    ] {
        let source =
            format!("import {{ bad }} from \"aritydep\"; function main(): number {{ {body} }}");
        let error =
            compile_script_checked(&source, "use.ts", FileId(7), &[&declaration], &[]).unwrap_err();
        assert!(error.fatal.is_none(), "{error:?}");
        assert_limit(&error.diagnostics, &source, FileId(7));
    }
    for source in [
        "function main(): number { return 42; }",
        "import { healthy } from \"aritydep\"; function main(): number { return healthy(); }",
        "import { bad } from \"aritydep\"; function main(): number { return 42; }",
    ] {
        compile_script_checked(source, "healthy.ts", FileId(7), &[&declaration], &[]).unwrap();
    }
}

fn oversized_type() -> Type {
    Type::Function {
        params: vec![Type::Number; 256],
        ret: Box::new(Type::Number),
        has_rest: false,
        predicate: None,
    }
}

#[test]
fn nested_imported_returns_and_generic_instantiations_are_checked() {
    let mut declaration = imported_declaration();
    let ValueKind::Function { params, ret, .. } =
        &mut declaration.values.get_mut("bad").unwrap().kind
    else {
        panic!("function")
    };
    params.clear();
    *ret = Type::Array(Box::new(oversized_type()));
    let source = "import { bad } from \"aritydep\"; function main(): number { const callbacks = bad(); return 42; }";
    let error =
        compile_script_checked(source, "nested.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, source, FileId(7));

    let ValueKind::Function { generics, ret, .. } =
        &mut declaration.values.get_mut("bad").unwrap().kind
    else {
        panic!("function")
    };
    generics.push("T".into());
    *ret = Type::TypeVar("T".into());
    let source = format!(
        "import {{ bad }} from \"aritydep\"; function main(): number {{ const callback = bad<({}) => number>(); return 42; }}",
        parameters(256)
    );
    let error =
        compile_script_checked(&source, "generic.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, &source, FileId(7));
}

#[test]
fn inherited_dependency_methods_require_supported_payload_signatures() {
    let mut declaration = compile_package_checked(
        "aritydep",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export class Base { f(): number { return 42; } }",
        }],
        &[],
    )
    .unwrap()
    .declaration;
    let interpreter::TypeKind::Class { methods, .. } =
        &mut declaration.types.get_mut("Base").unwrap().kind
    else {
        panic!("class")
    };
    methods.get_mut("f").unwrap().params = (0..256)
        .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
        .collect();
    for source in [
        "import { Base } from \"aritydep\"; class Child extends Base {} function main(): number { return 42; }",
        "import { Base } from \"aritydep\"; function main(): number { const b = new Base(); return 42; }",
    ] {
        let error = compile_script_checked(source, "class.ts", FileId(7), &[&declaration], &[])
            .unwrap_err();
        assert!(error.fatal.is_none(), "{error:?}");
        assert_limit(&error.diagnostics, source, FileId(7));
        assert!(error.diagnostics.iter().any(|d| {
            d.help
                .iter()
                .any(|h| h.contains("aritydep") && h.contains("f"))
        }));
    }
    compile_script_checked(
        "function main(): number { return 42; }",
        "healthy.ts",
        FileId(7),
        &[&declaration],
        &[],
    )
    .unwrap();
}

#[test]
fn imported_interface_methods_are_validated_when_dispatched() {
    let mut declaration = compile_package_checked("aritydep", ModulePath::from("lib"), &[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export interface I { f(): number; } export function get(): I { return { f: (): number => 42 }; }",
    }], &[]).unwrap().declaration;
    let interpreter::TypeKind::Interface { methods, .. } =
        &mut declaration.types.get_mut("I").unwrap().kind
    else {
        panic!("interface")
    };
    methods.get_mut("f").unwrap().params = (0..256)
        .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "import {{ get }} from \"aritydep\"; function main(): number {{ return get().f({args}); }}"
    );
    let error =
        compile_script_checked(&source, "method.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, &source, FileId(7));
    let source =
        "import { get } from \"aritydep\"; function main(): number { const i = get(); return 42; }";
    let error = compile_script_checked(source, "reachable.ts", FileId(7), &[&declaration], &[])
        .unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, source, FileId(7));
    compile_script_checked(
        "import { get } from \"aritydep\"; function main(): number { return 42; }",
        "unused.ts",
        FileId(7),
        &[&declaration],
        &[],
    )
    .unwrap();
}

#[test]
fn imported_constructors_keep_their_own_abi() {
    let params = parameters(256);
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    for generic in [false, true] {
        let (declaration_args, use_args) = if generic {
            ("<T>", "<number>")
        } else {
            ("", "")
        };
        let source = format!(
            "export class C{declaration_args} {{ last: number; constructor({params}) {{ this.last = a255; }} }}"
        );
        let package = compile_package_checked(
            "ctor",
            ModulePath::from("lib"),
            &[PackageSourceModule {
                path: ModulePath::from("lib"),
                source: &source,
            }],
            &[],
        )
        .unwrap();
        let source = format!(
            "import {{ C }} from \"ctor\"; function main(): number {{ return new C{use_args}({args}).last; }}"
        );
        compile_script_checked(
            &source,
            "constructor.ts",
            FileId(7),
            &[&package.declaration],
            &[],
        )
        .unwrap();
    }
}

#[test]
fn static_only_class_usage_checks_required_callable_payloads() {
    let mut declaration = compile_package_checked("aritydep", ModulePath::from("lib"), &[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export class C { static f: (a: number) => number = (a: number): number => a; static healthy(): number { return 42; } }",
    }], &[]).unwrap().declaration;
    let interpreter::TypeKind::Class { static_fields, .. } =
        &mut declaration.types.get_mut("C").unwrap().kind
    else {
        panic!("class")
    };
    static_fields.get_mut("f").unwrap().ty = oversized_type();
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    for call in [format!("C.f({args})"), "C.healthy()".into()] {
        let source = format!(
            "import {{ C }} from \"aritydep\"; function main(): number {{ return {call}; }}"
        );
        let error = compile_script_checked(&source, "static.ts", FileId(7), &[&declaration], &[])
            .unwrap_err();
        assert!(error.fatal.is_none(), "{error:?}");
        assert_limit(&error.diagnostics, &source, FileId(7));
    }
}

#[test]
fn static_calls_validate_unmentioned_instance_method_payloads() {
    let mut declaration = compile_package_checked("aritydep", ModulePath::from("lib"), &[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export class C { f(): number { return 42; } static healthy(): number { return 42; } }",
    }], &[]).unwrap().declaration;
    let interpreter::TypeKind::Class { methods, .. } =
        &mut declaration.types.get_mut("C").unwrap().kind
    else {
        panic!("class")
    };
    methods.get_mut("f").unwrap().params = (0..256)
        .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let source = "import { C } from \"aritydep\"; function main(): number { return C.healthy(); }";
    let error =
        compile_script_checked(source, "static.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, source, FileId(7));
}

#[test]
fn raw_host_functions_only_need_the_limit_when_adapted_to_closures() {
    let mut declaration = imported_declaration();
    declaration.runtime_functions.clear();
    let ValueKind::Function { params, .. } = &mut declaration.values.get_mut("bad").unwrap().kind
    else {
        panic!("function")
    };
    *params = (0..256)
        .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "import * as api from \"aritydep\"; function main(): number {{ return api.bad({args}); }}"
    );
    compile_script_checked(&source, "host.ts", FileId(7), &[&declaration], &[]).unwrap();
    let source =
        "import { bad } from \"aritydep\"; function main(): number { const f = bad; return 42; }";
    let error =
        compile_script_checked(source, "adapter.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(error.fatal.is_none(), "{error:?}");
    assert_limit(&error.diagnostics, source, FileId(7));
}

#[test]
fn inherited_statics_validate_the_declaring_class_only() {
    let declaration = compile_package_checked(
        "aritydep",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export class Base { baseMethod(): number { return 42; } static healthy(): number { return 42; } static identity<T>(value: T): T { return value; } static callback: () => number = (): number => 42; } export class Derived extends Base { derivedMethod(): number { return 42; } }",
        }],
        &[],
    ).unwrap().declaration;
    for (class, method) in [("Derived", "derivedMethod"), ("Base", "baseMethod")] {
        let mut declaration = declaration.clone();
        let interpreter::TypeKind::Class { methods, .. } =
            &mut declaration.types.get_mut(class).unwrap().kind
        else {
            panic!("class")
        };
        methods.get_mut(method).unwrap().params = (0..256)
            .map(|i| interpreter::Param::new(format!("a{i}"), Type::Number))
            .collect();
        for body in [
            "return Derived.healthy();",
            "const f = Derived.healthy; return f();",
            "return Derived.identity<number>(42);",
            "return Derived.callback();",
            "const f = Derived.callback; return f();",
        ] {
            let source = format!(
                "import {{ Derived }} from \"aritydep\"; function main(): number {{ {body} }}"
            );
            let result = compile_script_checked(
                &source,
                "inherited-static.ts",
                FileId(7),
                &[&declaration],
                &[],
            );
            if class == "Derived" {
                result.unwrap_or_else(|error| {
                    panic!("unused derived payload rejected for {body}: {error:?}")
                });
            } else {
                let error = result.unwrap_err();
                assert!(error.fatal.is_none(), "{body}: {error:?}");
                assert_limit(&error.diagnostics, &source, FileId(7));
                assert!(
                    error
                        .diagnostics
                        .iter()
                        .any(|d| d.help.iter().any(|h| h.contains("Base.baseMethod"))),
                    "{body}: {error:?}"
                );
            }
        }
    }
}
