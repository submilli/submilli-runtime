//! Mapped, conditional and intersection types that stay generic: a front end
//! other than the built-in one writes them into the `TypedAst` and the
//! `PackageDeclaration`. The built-in typechecker never produces them, so each
//! program here is checked with placeholder forms that lower the same way,
//! which are then replaced:
//!
//! - `keyof X` becomes `{ [K in keyof X as K]?: X[K] }`;
//! - `Partial<X>` becomes `X extends (infer E)[] ? E : boolean`;
//! - `X[Y]` becomes `X & Y`;
//! - the aliases `Branded`, `Score`, `Big` and `Flag` of `string`, `number`,
//!   `bigint` and `boolean` become that primitive `& { brand: true }`, which
//!   is used as the primitive.
use std::collections::BTreeMap;

use submilli_engine::compiler_limits::COMPILER_STACK_BYTES;
use submilli_engine::runtime::{
    LinkedPackageModule, StoreData, Vfs, install_package_modules_async,
    install_runtime_host_functions, install_runtime_store_bound, install_tenant_limits,
};
use submilli_engine::type_size::map_children_infallible;
use submilli_engine::types::MappedModifier;
use submilli_engine::{
    FileId, ModulePath, ObjectField, PackageDeclaration, PackageSourceModule, RuntimeConfig, Type,
    TypeKind, TypedAst, TypedTypeDecl, ValueKind,
};
use wasmtime::{Linker, Module};

/// Each variant in a generic function's signature and in a generic alias.
const SCRIPT: &str = r#"
type Patch<T> = keyof T;
type Test<T> = Partial<T>;
type Both<T, K extends keyof T> = T[K];
type Branded = string;
type Score = number;
type Big = bigint;
type Flag = boolean;

function patch<T>(base: T, change: keyof T): T { return base; }
function patchAlias<T>(base: T, change: Patch<T>): T { return base; }
function test<T>(x: T, marker: Partial<T>): T { return x; }
function testAlias<T>(x: T, marker: Test<T>): T { return x; }
function both<T, K extends keyof T>(a: T, k: K): T[K] { return a[k]; }
function bothAlias<T, K extends keyof T>(a: T, k: K): Both<T, K> { return a[k]; }
function brand(s: string): Branded { return s; }
function size(b: Branded): number {
  const shout = `${b}!`;
  const parts: Branded[] = [b, brand(b + "?")];
  const ordered = b < "b" && +brand("1") === 1 && b[0] === "a";
  return ordered && b === "abcdefg" && shout.endsWith("!") && parts[1].length === 8 ? b.length : 0;
}

function score(n: number): Score { return n; }
function big(n: bigint): Big { return n; }
function flag(b: boolean): Flag { return b; }
function arithmetic(s: Score, x: Big): number {
  let t = s;
  t++;
  const numbers = s * 2 === 6 && s < 4 && -s === -3 && t === 4;
  const bigints = -x === -5n && x < 6n && x * 2n === 10n;
  const flags = !flag(false) && flag(true);
  return numbers && bigints && flags ? 1 : 0;
}

function main(): number {
  const p = patch({ x: 1 }, "x");
  const q = patchAlias({ x: 2 }, "x");
  const t = test({ x: 3 }, {});
  const u = testAlias({ x: 4 }, {});
  const v = both({ x: 5 }, "x");
  const w = bothAlias({ x: 6 }, "x");
  return p.x + q.x + t.x + u.x + v + w + size(brand("abcdefg")) + arithmetic(score(3), big(5n));
}
"#;

const PACKAGE: &str = r#"
export type Branded = string;
export function patch<T>(base: T, change: keyof T): T { return base; }
export function test<T>(x: T, marker: Partial<T>): T { return x; }
export function both<T, K extends keyof T>(a: T, k: K): T[K] { return a[k]; }
export function brand(s: string): Branded { return s; }
export function size(b: Branded): number { return b.length; }
"#;

const CONSUMER: &str = r#"
import { patch, test, both, brand, size } from "ops";

function main(): number {
  const p = patch({ x: 1 }, "x");
  const t = test({ x: 3 }, {});
  const v = both({ x: 5 }, "x");
  return p.x + t.x + v + size(brand("abcdefg"));
}
"#;

#[test]
fn typed_ast_with_type_operators_compiles_and_runs() {
    on_compiler_stack(|| {
        let (mut ast, diagnostics) =
            submilli_engine::compile::typecheck_to_typed_ast(SCRIPT, FileId(0));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let rewritten = rewrite_typed_ast(&mut ast);
        for spelled in [MAPPED, CONDITIONAL, INTERSECTION] {
            for (position, types) in [
                ("signature", &rewritten.signatures),
                ("alias", &rewritten.aliases),
            ] {
                assert!(
                    types.iter().any(|ty| ty == spelled),
                    "no {position} is {spelled}: {types:?}"
                );
            }
        }
        for primitive in ["string", "number", "bigint", "boolean"] {
            let spelled = format!("{primitive} & {{ brand: true }}");
            assert!(rewritten.aliases.contains(&spelled), "{spelled}");
        }
        let dependencies = runtime_dependencies(&[]);
        let rules =
            submilli_engine::typechecker::rules::check_script(&ast, &dependencies).expect("rules");
        assert!(rules.is_empty(), "{rules:?}");
        let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0))
            .expect("desugar");
        let generated = submilli_engine::codegen::codegen_with_type_info(
            SCRIPT,
            "script.ts",
            FileId(0),
            &ast,
            &dependencies,
        )
        .expect("codegen");
        let result =
            pollster::block_on(RuntimeConfig::default().run(&generated.wasm)).expect("run");
        assert_eq!(result.value.as_deref(), Some("29"));
    });
}

#[test]
fn package_declaration_with_type_operators_links_from_a_consumer() {
    on_compiler_stack(|| {
        let modules = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: PACKAGE,
        }];
        let package =
            submilli_engine::compile_package("ops", ModulePath::from("lib"), &modules, &[])
                .expect("package");
        let mut rewritten = package.declaration.clone();
        rewrite_declaration(&mut rewritten);
        let serialized = serde_json::to_string(&rewritten).expect("serialize");
        let declaration: PackageDeclaration =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(declaration, rewritten);
        declaration.check_type_limits().expect("declaration limits");
        let rendered = declaration_types(&declaration);
        for spelled in [MAPPED, CONDITIONAL, INTERSECTION] {
            assert!(
                rendered.signatures.iter().any(|ty| ty == spelled),
                "no signature is {spelled}: {:?}",
                rendered.signatures
            );
        }
        assert_eq!(rendered.aliases, ["string & { brand: true }"]);

        // The consumer is checked against the placeholders, then compiled and
        // linked against the declaration that carries the type operators.
        let parsed = parse(CONSUMER);
        let stdlib = submilli_engine::runtime::stdlib_package_declarations();
        let (prelude, host, _) =
            submilli_engine::runtime::prelude::cached_runtime_package_declarations();
        let infer_refs: Vec<&PackageDeclaration> = prelude
            .iter()
            .chain(host)
            .chain(&stdlib)
            .chain([&package.declaration])
            .collect();
        let (mut ast, diagnostics) = submilli_engine::infer(CONSUMER, "main", &parsed, &infer_refs);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        rewrite_typed_ast(&mut ast);
        let mut dependencies = runtime_dependencies(&stdlib);
        dependencies.push(&declaration);
        let rules =
            submilli_engine::typechecker::rules::check_script(&ast, &dependencies).expect("rules");
        assert!(rules.is_empty(), "{rules:?}");
        let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0))
            .expect("desugar");
        let consumer = submilli_engine::codegen::codegen_with_type_info(
            CONSUMER,
            "consumer.ts",
            FileId(0),
            &ast,
            &dependencies,
        )
        .expect("codegen");
        let value = pollster::block_on(run_linked(
            &package.wasm,
            &declaration,
            &package.type_info,
            &consumer,
        ))
        .expect("run");
        assert_eq!(value.as_deref(), Some("16"));
    });
}

/// How the placeholders over `T` and `K` are spelled once replaced.
const MAPPED: &str = "{ [K in keyof T as K]?: T[K] }";
const CONDITIONAL: &str = "T extends (infer E)[] ? E : boolean";
const INTERSECTION: &str = "T & K";

/// Function parameter and return types, and alias bodies, as tsc spells
/// them.
#[derive(Default)]
struct SpelledTypes {
    signatures: Vec<String>,
    aliases: Vec<String>,
}

/// Replaces the placeholders in every signature, declaration and expression
/// type of `ast`.
fn rewrite_typed_ast(ast: &mut TypedAst) -> SpelledTypes {
    let mut rewritten = SpelledTypes::default();
    for function in &mut ast.functions {
        for param in &mut function.params {
            param.ty = rewrite(&param.ty);
            rewritten.signatures.push(param.ty.to_string());
        }
        function.return_type = rewrite(&function.return_type);
        rewritten.signatures.push(function.return_type.to_string());
    }
    for decl in &mut ast.types {
        if let TypedTypeDecl::Alias(alias) = decl {
            alias.ty = branded(&alias.name.name).unwrap_or_else(|| rewrite(&alias.ty));
            rewritten.aliases.push(alias.ty.to_string());
        }
    }
    for id in ast.expr_ids().unwrap() {
        let expr = ast.try_expr_mut(id).unwrap();
        expr.ty = rewrite(&expr.ty);
    }
    for ty in ast.authored_parameter_types.values_mut() {
        *ty = rewrite(ty);
    }
    rewritten
}

fn rewrite_declaration(declaration: &mut PackageDeclaration) {
    for symbol in declaration.values.values_mut() {
        if let ValueKind::Function { params, ret, .. } = &mut symbol.kind {
            for param in params {
                param.ty = rewrite(&param.ty);
            }
            *ret = rewrite(ret);
        }
    }
    for symbol in declaration.types.values_mut() {
        if let TypeKind::Alias { ty, .. } = &mut symbol.kind
            && let Some(primitive) = branded(&symbol.name)
        {
            *ty = primitive;
        }
    }
    for function in declaration.runtime_functions.values_mut() {
        for param in &mut function.params {
            *param = rewrite(param);
        }
        function.ret = rewrite(&function.ret);
    }
}

/// The declaration's function parameter and return types, and its alias
/// bodies, as tsc spells them.
fn declaration_types(declaration: &PackageDeclaration) -> SpelledTypes {
    let mut rendered = SpelledTypes::default();
    for symbol in declaration.values.values() {
        if let ValueKind::Function { params, ret, .. } = &symbol.kind {
            rendered
                .signatures
                .extend(params.iter().map(|param| param.ty.to_string()));
            rendered.signatures.push(ret.to_string());
        }
    }
    for symbol in declaration.types.values() {
        if let TypeKind::Alias { ty, .. } = &symbol.kind {
            rendered.aliases.push(ty.to_string());
        }
    }
    rendered
}

fn rewrite(ty: &Type) -> Type {
    match ty {
        Type::KeyOf(object) => {
            let object = rewrite(object);
            Type::Mapped {
                key: "K".into(),
                constraint: Box::new(Type::KeyOf(Box::new(object.clone()))),
                name: Some(Box::new(Type::TypeVar("K".into()))),
                template: Box::new(Type::IndexedAccess {
                    object: Box::new(object),
                    index: Box::new(Type::TypeVar("K".into())),
                }),
                optional: MappedModifier::Add,
                readonly: MappedModifier::Keep,
            }
        }
        Type::AliasRef { name, args, .. } if name == "Partial" && args.len() == 1 => {
            Type::Conditional {
                check: Box::new(rewrite(&args[0])),
                extends: Box::new(Type::Array(Box::new(Type::TypeVar("E".into())))),
                infers: vec!["E".into()],
                true_ty: Box::new(Type::TypeVar("E".into())),
                false_ty: Box::new(Type::Boolean),
            }
        }
        Type::IndexedAccess { object, index } => {
            Type::intersection(vec![rewrite(object), rewrite(index)])
        }
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ..
        } if let Some(primitive) = branded(name) => Type::Alias {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args.clone(),
            ty: Box::new(primitive),
        },
        _ => map_children_infallible(ty, rewrite),
    }
}

/// What a placeholder alias of a primitive becomes: the primitive
/// `& { brand: true }`.
fn branded(alias: &str) -> Option<Type> {
    let primitive = match alias {
        "Branded" => Type::String,
        "Score" => Type::Number,
        "Big" => Type::BigInt,
        "Flag" => Type::Boolean,
        _ => return None,
    };
    Some(Type::intersection(vec![
        primitive,
        Type::Object {
            more_indexes: Vec::new(),
            fields: BTreeMap::from([(
                "brand".to_string(),
                ObjectField::required(Type::BooleanLiteral(true)),
            )]),
            index: None,
        },
    ]))
}

fn parse(source: &str) -> submilli_engine::Ast {
    let mut asi = submilli_engine::Asi::new(source, FileId(0));
    let mut tokens = Vec::new();
    loop {
        let token = asi.next_token();
        let eof = matches!(token.kind, submilli_engine::TokenKind::Eof);
        tokens.push(token);
        if eof {
            break;
        }
    }
    let lexed = asi.finish().expect("lex");
    assert!(lexed.is_empty(), "{lexed:?}");
    let (ast, diagnostics) = submilli_engine::parse(source, tokens, FileId(0));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    submilli_engine::lower_patterns(ast).expect("lower")
}

fn runtime_dependencies(stdlib: &[PackageDeclaration]) -> Vec<&PackageDeclaration> {
    let (prelude, host, internal) =
        submilli_engine::runtime::prelude::cached_runtime_package_declarations();
    prelude
        .iter()
        .chain(host)
        .chain(internal)
        .chain(stdlib)
        .collect()
}

async fn run_linked(
    package_wasm: &[u8],
    declaration: &PackageDeclaration,
    package_types: &submilli_engine::TypeInfoTable,
    consumer: &submilli_engine::codegen::GeneratedModule,
) -> wasmtime::Result<Option<String>> {
    let config = RuntimeConfig::default();
    let engine = config.engine()?;
    let data = StoreData::with_vfs("typed-type-operators".into(), Vfs::tempdir()?);
    let mut store = config.store_async(&engine, data)?;
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(&engine);
    install_runtime_host_functions(&mut linker)?;
    install_runtime_store_bound(&mut linker, &mut store)?;
    let package_module = Module::new(&engine, package_wasm)?;
    install_package_modules_async(
        &mut linker,
        &mut store,
        &[LinkedPackageModule {
            module: &package_module,
            declaration,
            type_info: package_types,
        }],
    )
    .await?;
    store
        .data_mut()
        .install_type_info(consumer.type_info.clone());
    let module = Module::new(&engine, &consumer.wasm)?;
    let instance = linker.instantiate_async(&mut store, &module).await?;
    submilli_engine::dispatch_main_async(&mut store, &instance).await
}

fn on_compiler_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(COMPILER_STACK_BYTES)
        .spawn(test)
        .expect("compiler thread")
        .join()
        .expect("test thread");
}
