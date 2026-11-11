//! Source validation must run during check, before lowering to Wasm.
use submilli_engine::compile::{
    compile_package_checked, compile_script_checked, typecheck_checked,
};
use submilli_engine::{
    Diagnostic, FileId, ModulePath, PackageSourceModule, Severity, Type, ValueKind,
};

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

fn limit_diagnostics(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.message.contains("parameter slots"))
        .collect()
}

#[test]
fn an_alias_is_reported_once() {
    let source = format!(
        "type F = ({}) => number; function main(): number {{ const f: F | null = null; return 42; }}",
        parameters(256)
    );
    let error = typecheck_checked(&source, FileId(0)).unwrap_err();
    assert_eq!(limit_diagnostics(&error.diagnostics).len(), 1, "{error:?}");
}

/// Each place that writes a function type of too many slots is reported, even
/// when another writes the same one.
#[test]
fn each_written_place_is_reported() {
    let p = parameters(256);
    let source = format!(
        "type F = ({p}) => void; type G = ({p}) => void; function main(): number {{ return 42; }}"
    );
    let error = typecheck_checked(&source, FileId(0)).unwrap_err();
    let spans: Vec<_> = limit_diagnostics(&error.diagnostics)
        .into_iter()
        .map(|d| d.span.text(&source, FileId(0)).unwrap())
        .collect();
    assert_eq!(spans, ["F", "G"], "{error:?}");
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

fn compile_package(source: &str) -> Result<(), submilli_engine::compiler_error::CompileError> {
    compile_package_checked(
        "aritydep",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source,
        }],
        &[],
    )
    .map(|_| ())
}

/// A package with a callable of too many slots fails its own compile, once,
/// at the declaration, whatever a script would use of it.
#[test]
fn packages_report_the_limit_where_declared() {
    for arity in [255, 256] {
        let p = parameters(arity);
        // A closure is reported over its parameter list.
        let closure = p.strip_suffix(": number").unwrap();
        let cases = [
            (
                "an interface's method",
                format!("export interface I {{ f({p}): number; }}"),
                "f",
            ),
            (
                "a member's function type",
                format!("export interface I {{ f: ({p}) => number; }}"),
                "f",
            ),
            (
                "a rest parameter's element",
                format!("export function g(...fs: Array<({p}) => void>): void {{}}"),
                "fs",
            ),
            (
                "a function type inside a parameter's type",
                format!("export function g(o?: {{ callback: ({p}) => void }}): void {{}}"),
                "o",
            ),
            (
                "a function",
                format!("export function g({p}): number {{ return 42; }}"),
                "g",
            ),
            (
                "a function and a value that holds it",
                format!("export function f({p}): void {{}} export const g = f;"),
                "f",
            ),
            (
                "a type predicate",
                format!("export function isF(x: unknown): x is ({p}) => void {{ return false; }}"),
                "isF",
            ),
            (
                "a function's bound",
                format!("export function f<T extends ({p}) => void>(x: T | null): void {{}}"),
                "f",
            ),
            (
                "a method's bound",
                format!("export interface I {{ m<T extends ({p}) => void>(x: T): void; }}"),
                "m",
            ),
            (
                "a class field",
                format!("export class C {{ f: (({p}) => void) | null = null; }}"),
                "f",
            ),
            (
                "a static field",
                format!("export class C {{ static f: (({p}) => void) | null = null; }}"),
                "C",
            ),
            (
                "a class's extends clause",
                format!(
                    "export class Base<T> {{ f: T | null = null; }} export class C extends Base<({p}) => void> {{}}"
                ),
                "C",
            ),
            (
                "an interface's extends clause",
                format!(
                    "export interface B<T> {{ f: T; }} export interface I extends B<({p}) => void> {{}}"
                ),
                "I",
            ),
            (
                "a static method",
                format!("export class C {{ static f({p}): number {{ return 1; }} }}"),
                "f",
            ),
            (
                "a class's bound",
                format!("export class C<T extends ({p}) => void> {{ v: T | null = null; }}"),
                "C",
            ),
            (
                "a getter",
                format!("export class C {{ get g(): (({p}) => void) | null {{ return null; }} }}"),
                "g",
            ),
            (
                "a closure's type predicate",
                format!("export const isF = (x: unknown): x is ({p}) => void => false;"),
                "x",
            ),
            (
                "an inherited extends clause",
                format!(
                    "export class A<T> {{ f: T | null = null; }} export class B<U> extends A<U> {{}} export class C extends B<({p}) => void> {{}}"
                ),
                "C",
            ),
            (
                "a global's closure",
                format!("export const g = ({p}): void => {{}};"),
                closure,
            ),
            (
                "an object literal's method",
                format!("export const o = {{ m({p}): number {{ return 1; }} }};"),
                closure,
            ),
        ];
        for (name, source, declared) in cases {
            let result = compile_package(&source);
            if arity == 255 {
                result.unwrap_or_else(|error| panic!("{name}: {error:?}"));
                continue;
            }
            let error = result.unwrap_err();
            assert!(error.fatal.is_none(), "{name}: {error:?}");
            assert_limit(&error.diagnostics, &source, FileId(0));
            let limits = limit_diagnostics(&error.diagnostics);
            assert_eq!(limits.len(), 1, "{name}: {limits:?}");
            assert_eq!(
                limits[0].span.text(&source, FileId(0)).unwrap(),
                declared,
                "{name}"
            );
        }
    }
}

/// Inference copies a declared function type to the values that hold it; the
/// rule reports it once, where it is written.
#[test]
fn bodies_report_each_declared_type_once() {
    let p = parameters(256);
    let closure = p.strip_suffix(": number").unwrap();
    let cases = [
        (
            "a local's type",
            format!("const fs: Array<({p}) => void> = [];"),
            "fs",
        ),
        (
            "a closure held by a local",
            format!("const g = ({p}): void => {{}};"),
            closure,
        ),
        (
            "a function held by a local",
            "const g = f; const h = [f];".into(),
            "f",
        ),
        (
            "a cast",
            format!("const u: unknown = 1; u as ({p}) => void;"),
            "u as",
        ),
        (
            "a type argument",
            format!("id<({p}) => void>(null!);"),
            "id<",
        ),
    ];
    for (name, body, declared) in cases {
        let source = format!(
            "function id<T>(value: T): T {{ return value; }} function f({p}): number {{ return 1; }} function main(): number {{ {body} return 42; }}"
        );
        let error = typecheck_checked(&source, FileId(0)).unwrap_err();
        assert!(error.fatal.is_none(), "{name}: {error:?}");
        // `f` is reported in every case; the case adds at most one more.
        let limits: Vec<_> = limit_diagnostics(&error.diagnostics)
            .into_iter()
            .map(|d| d.span.text(&source, FileId(0)).unwrap())
            .filter(|text| *text != "f")
            .collect();
        if declared == "f" {
            assert!(limits.is_empty(), "{name}: {limits:?}");
        } else {
            assert_eq!(limits.len(), 1, "{name}: {limits:?}");
            assert!(limits[0].starts_with(declared), "{name}: {limits:?}");
        }
    }
}

/// A dependency is checked when it is compiled. One built by hand reaches
/// codegen, whose own check still rejects the signatures it lowers.
#[test]
fn codegen_rejects_a_hand_built_dependency() {
    let mut declaration = compile_package_checked("aritydep", ModulePath::from("lib"), &[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export function bad(a: number): number { return a; } export function healthy(): number { return 42; }",
    }], &[]).unwrap().declaration;
    let ValueKind::Function { params, .. } = &mut declaration.values.get_mut("bad").unwrap().kind
    else {
        panic!("function")
    };
    *params = (0..256)
        .map(|i| submilli_engine::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let runtime_name = declaration.values.get("bad").unwrap().mangled_name.clone();
    declaration
        .runtime_functions
        .get_mut(&runtime_name)
        .unwrap()
        .params = vec![Type::Number; 256];
    let args = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "import {{ bad }} from \"aritydep\"; function main(): number {{ return bad({args}); }}"
    );
    let error =
        compile_script_checked(&source, "use.ts", FileId(7), &[&declaration], &[]).unwrap_err();
    assert!(
        matches!(&error.fatal, Some(submilli_engine::compiler_error::CompilerFailure::Limit { message, .. }) if message.contains("256 parameter slots")),
        "{error:?}"
    );
    compile_script_checked(
        "import { healthy } from \"aritydep\"; function main(): number { return healthy(); }",
        "healthy.ts",
        FileId(7),
        &[&declaration],
        &[],
    )
    .unwrap();
}

/// A hand-built dependency's interface may hold a method of too many slots.
/// Using the interface lowers every member's signature, so codegen rejects a
/// script that reads only another member. The compiler never builds such a
/// declaration: host packages, the only ones built by hand, dispatch their
/// interfaces statically or directly.
#[test]
fn codegen_rejects_a_hand_built_interface() {
    let wide = parameters(255);
    let source = format!(
        "export interface I {{ m(a: number): number; n({wide}): number; p: number; }}
         export function get(): I {{ return {{ m: (a: number): number => a, n: ({wide}): number => a254, p: 7 }}; }}"
    );
    let mut declaration = compile_package_checked(
        "aritydep",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &source,
        }],
        &[],
    )
    .unwrap()
    .declaration;
    let submilli_engine::TypeKind::Interface { methods, .. } =
        &mut declaration.types.get_mut("I").unwrap().kind
    else {
        panic!("interface")
    };
    methods.get_mut("m").unwrap().params = (0..256)
        .map(|i| submilli_engine::Param::new(format!("a{i}"), Type::Number))
        .collect();
    let args = (1..=255)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    for body in [
        "return get().p;".to_string(),
        format!("return get().n({args});"),
    ] {
        let source =
            format!("import {{ get }} from \"aritydep\"; function main(): number {{ {body} }}");
        let error =
            compile_script_checked(&source, "use.ts", FileId(7), &[&declaration], &[]).unwrap_err();
        assert!(
            matches!(&error.fatal, Some(submilli_engine::compiler_error::CompilerFailure::Limit { message, .. }) if message.contains("256 parameter slots")),
            "{body}: {error:?}"
        );
    }
}
