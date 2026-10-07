//! End-to-end snapshots of rendered diagnostics across B1–B5.
//! Each test drives a small .subm program through the full pipeline and
//! renders every emitted diagnostic; this file is the integration guarantee.

use interpreter::{
    Asi, FileId, Severity, Sources, Token, TokenKind, check, diagnostics, infer, parse,
};

const FILENAME: &str = "test.subm";

fn render_all(source: &str) -> String {
    let mut asi = Asi::new(source, FileId(0));
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let tok = asi.next_token();
        let is_eof = matches!(tok.kind, TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let mut diags = asi.into_diagnostics();
    let (ast, parse_diags) = parse(source, tokens, FileId(0));
    diags.extend(parse_diags);
    // Skip later phases if lex/parse already failed — running infer on
    // a partial AST would just produce cascading noise.
    if !diags.iter().any(|d| d.severity == Severity::Error) {
        let (prelude_defs, host_defs, _) =
            interpreter::runtime::prelude::cached_runtime_package_declarations();
        let mut package_refs = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        package_refs.extend(prelude_defs.iter());
        package_refs.extend(host_defs.iter());
        let (ta, infer_diags) = infer(source, "main", &ast, &package_refs);
        diags.extend(infer_diags);
        diags.extend(check(&ta).unwrap());
    }

    let (sources, _) = Sources::single(FILENAME, source).unwrap();
    let mut out = String::new();
    for diag in &diags {
        out.push_str(&diagnostics::render(diag, &sources));
        out.push_str("---\n");
    }
    out
}

/// Sources that intentionally omit `main` would otherwise carry the
/// "missing main" diagnostic on every test; prefix with a no-op
/// `main` so each snapshot focuses on the diagnostic under test.
fn render_inside_main(body: &str) -> String {
    let source = format!("function main(): void {{\n{body}\n}}\n");
    render_all(&source)
}

#[test]
fn b1_field_not_found_on_object() {
    insta::assert_snapshot!(render_inside_main(
        "  let p: { x: number; y: number } = { x: 1, y: 2 };\n  let z = p.z;"
    ));
}

#[test]
fn b1_call_non_callable_lifts_number_interface() {
    insta::assert_snapshot!(render_inside_main("  let n: number = 42;\n  let r = n(1);"));
}

#[test]
fn b1_index_into_non_array() {
    insta::assert_snapshot!(render_inside_main("  let n: number = 42;\n  let r = n[0];"));
}

#[test]
fn b1_dynamic_object_index() {
    insta::assert_snapshot!(render_inside_main(
        "  let o = { a: 1 };\n  let k = \"a\";\n  let r = o[k];"
    ));
}

#[test]
fn b2_call_arity_mismatch_lifts_anon_signature() {
    let source = "\
function add(a: number, b: number): number { return a + b; }
function main(): void {
  add(1);
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn b2_generic_function_arg_unify_mismatch_lifts_signature_and_diff() {
    let source = "\
function pick<T>(a: T, b: T): T { return a; }
function main(): void {
  const first: { x: number; y: number } = { x: 1, y: 2 };
  let p = pick(first, { x: 1, z: 3 });
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn b2_method_arity_mismatch_lifts_method_signature() {
    insta::assert_snapshot!(render_inside_main(
        "  let xs: number[] = [1, 2, 3];\n  xs.push();"
    ));
}

#[test]
fn b2_generic_method_type_arg_arity_lifts_method_signature() {
    insta::assert_snapshot!(render_inside_main(
        "  let xs: number[] = [1, 2, 3];\n  let ys = xs.map<string, number>((x: number) => x.toString());"
    ));
}

#[test]
fn b2_intrinsic_arity_lifts_signature() {
    insta::assert_snapshot!(render_inside_main("  parseInt();"));
}

#[test]
fn b2_number_coercion_type_mismatch_lifts_signature() {
    insta::assert_snapshot!(render_inside_main("  let x = Number(42);"));
}

#[test]
fn b3_unresolved_identifier_suggests_close_local() {
    insta::assert_snapshot!(render_inside_main("  let pi: number = 3;\n  let q = pi2;"));
}

#[test]
fn b3_unresolved_identifier_no_suggestion_when_no_close_match() {
    insta::assert_snapshot!(render_inside_main(
        "  let pi: number = 3;\n  let q = absolutely_unrelated_name;"
    ));
}

#[test]
fn b3_unknown_type_suggests_close_builtin() {
    insta::assert_snapshot!(render_inside_main("  let n: numbre = 1;"));
}

#[test]
fn b4_missing_main_help() {
    // Single space so the source has any non-empty token to anchor a
    // line on; the diagnostic span is (0, 0).
    insta::assert_snapshot!(render_all(" \n"));
}

#[test]
fn b4_let_missing_initializer_help() {
    insta::assert_snapshot!(render_inside_main("  let x: number;"));
}

#[test]
fn b4_const_missing_initializer_help() {
    insta::assert_snapshot!(render_inside_main("  const x: number;"));
}

#[test]
fn b4_missing_return_type_help() {
    insta::assert_snapshot!(render_all("function main() { }\n"));
}

#[test]
fn b4_param_missing_annotation_help() {
    insta::assert_snapshot!(render_all(
        "function f(a): number { return a; }\nfunction main(): void { }\n"
    ));
}

#[test]
fn b4_const_reassign_local_help() {
    insta::assert_snapshot!(render_inside_main("  const k = 1;\n  k = 2;"));
}

#[test]
fn b4_const_reassign_global_help() {
    insta::assert_snapshot!(render_all(
        "const k: number = 1;\nfunction main(): void { k = 2; }\n"
    ));
}

#[test]
fn b4_function_reassign_help() {
    insta::assert_snapshot!(render_all(
        "function f(): void { }\nfunction main(): void { f = 1; }\n"
    ));
}

#[test]
fn b4_return_outside_function_help() {
    insta::assert_snapshot!(render_all("function main(): void { }\nreturn 1;\n"));
}

#[test]
fn b4_cannot_infer_type_param_function_help() {
    let source = "\
function pickReturn<T>(): T { return null; }
function main(): void {
  let r = pickReturn();
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn b4_unknown_escape_help() {
    insta::assert_snapshot!(render_inside_main("  let s = \"hi\\q\";"));
}

#[test]
fn b5_object_field_assign_value_mismatch_diff() {
    let source = "\
function main(): void {
  let p: { name: string; age: number } = { name: \"a\", age: 1 };
  p.name = 42;
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn b5_function_type_assign_diff() {
    insta::assert_snapshot!(render_inside_main(
        "  const g = (x: string) => true;\n  let f: (a: number, b: number) => boolean = g;"
    ));
}

#[test]
fn b5_object_boundary_diff_in_call() {
    let source = "\
function takes(p: { x: number; y: number }): void { }
function main(): void {
  takes({ x: 1, z: 3 });
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn b5_fresh_object_literal_excess_field_help() {
    let source = "\
function main(): void {
  const p: { firstName: string; age: number } = { firstNmae: \"Ada\", age: 37 };
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_param_name_mismatch_warns() {
    let source = "\
/**
 * Sum two numbers.
 * @param wrong The first operand.
 * @param b The second operand.
 * @returns The sum.
 */
function add(a: number, b: number): number { return a + b; }
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_returns_on_void_warns() {
    let source = "\
/**
 * Does nothing.
 * @returns Nothing.
 */
function noop(): void { }
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_missing_returns_warns() {
    let source = "\
/**
 * Squares a number.
 * @param x The input.
 */
function square(x: number): number { return x * x; }
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_duplicate_param_warns() {
    let source = "\
/**
 * @param a First.
 * @param a Again.
 * @returns r
 */
function f(a: number): number { return a; }
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_unknown_tag_warns() {
    let source = "\
/**
 * Doc with typo.
 * @parm a First.
 * @returns r
 */
function f(a: number): number { return a; }
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn doc_surfaces_in_call_arity_lift() {
    let source = "\
/**
 * Sum two numbers.
 * @param a First operand.
 * @param b Second operand.
 * @returns The sum.
 */
function add(a: number, b: number): number { return a + b; }
function main(): void {
  let _ = add(1);
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn prelude_doc_surfaces_in_array_push_lift() {
    insta::assert_snapshot!(render_inside_main(
        "  let xs: number[] = [1, 2, 3];\n  xs.push();"
    ));
}

#[test]
fn multi_catch_duplicate_arm() {
    let source = "\
class ParseError extends Error {
  constructor(m: string) { super(m); }
}
function main(): void {
  try {
    throw new ParseError(\"x\");
  } catch (e: ParseError) {
  } catch (e: ParseError) {
  }
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn multi_catch_unreachable_arm() {
    let source = "\
class Base extends Error {
  constructor(m: string) { super(m); }
}
class Derived extends Base {
  constructor(m: string) { super(m); }
}
function main(): void {
  try {
    throw new Derived(\"x\");
  } catch (e: Base) {
  } catch (e: Derived) {
  }
}
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn nullable_template_interpolation_offers_fix_trio() {
    let source = "\
function fmt(node: { title: string; completedAt: string | null }): string {
  return `${node.title} (Completed: ${node.completedAt})`;
}
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn nullable_template_interpolation_complex_expr_parenthesized() {
    let source = "\
function fmt(a: string | null, b: string | null): string {
  return `got: ${a ?? b}`;
}
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

#[test]
fn null_tostring_offers_fix_trio() {
    insta::assert_snapshot!(render_inside_main(
        "  const n = null;\n  let s = n.toString();"
    ));
}

#[test]
fn doc_property_with_param_warns() {
    let source = "\
interface Box {
  /**
   * The boxed value.
   * @param wrong This shouldn't be here.
   */
  value: number;
}
function main(): void { }
";
    insta::assert_snapshot!(render_all(source));
}

/// A closely matched mismatch inside an object literal argument is reported
/// at each field, as tsc does, and an error elsewhere in the argument (a
/// callback's body) does not hide it.
#[test]
fn b5_close_match_reported_at_each_field() {
    let source = "\
class Box<A> {
  constructor(public v: A) {}
}
function pair<T>(o: { a: T | Box<number>; b: T | Box<number> }, c: T): T {
  return c;
}
function run<T>(o: { a: T | Box<number>; cb: () => void }, c: T): T {
  return c;
}
function main(): void {
  pair({ a: new Box(true), b: new Box(\"s\") }, 1);
  run({ a: new Box(true), cb: () => { const n: number = \"x\"; } }, 1);
}
";
    insta::assert_snapshot!(render_all(source));
}
