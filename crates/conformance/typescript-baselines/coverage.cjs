// Writes ../COVERAGE.md: for each feature Submilli supports, whether every
// upstream test about it has been dealt with, and what the suites check of it.
//
// Usage: node coverage.cjs <TypeScript checkout>
// The checkout is the one `port-suite.cjs` ports from. Runs `cargo test`, so it
// needs the workspace. COVERAGE_SAMPLE=<text> also prints the checks counted for
// each feature whose name contains <text>, to check a detector against what it
// matched.
//
// A feature is done when every single-file upstream TypeScript test that uses it
// is in the suite or excluded with a reason, and every test262 area it names has
// been ported (each area records what it skipped, and why). TypeScript's authors
// decided how much testing a feature needs; the suite is done with it when it has
// taken all of that.
//
// The lines checked are what the suite exercises of a feature, shown so that a
// feature pruning nearly removed stands out. A TypeScript check is a `tsc` type
// the runner compared, or a line whose errors it compared (TYPESCRIPT_CHECKS_OUT
// lists them). A check counts for a feature when the feature's syntax starts on
// the check's line; when it reads a name the feature declares, within the
// feature's syntax (a `catch` variable, a loop variable, a parameter); when it is
// inside a feature whose every line it shapes (`try`); or, for a type feature,
// when the type `tsc` gave the checked expression has it.
const ts = require("typescript");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync } = require("child_process");

const conformanceDir = path.join(__dirname, "..");
const workspace = path.join(conformanceDir, "..", "..");
const outputFile = path.join(conformanceDir, "COVERAGE.md");

// A value the port supplies for a binding with none (`null as unknown as (T)`):
// the port's scaffolding, not something the case checks.
const PORT_VALUE = /^null as unknown( as |$)/;

const K = ts.SyntaxKind;
const is = (...kinds) => (node) => kinds.includes(node.kind);
const binary = (...operators) => (node) =>
  node.kind === K.BinaryExpression && operators.includes(node.operatorToken.kind);
const prefix = (...operators) => (node) =>
  node.kind === K.PrefixUnaryExpression && operators.includes(node.operator);
const modifier = (kind) => (node) => ts.canHaveModifiers(node) && (ts.getModifiers(node) ?? []).some((m) => m.kind === kind);
const inFunction = (node) => {
  for (let p = node.parent; p; p = p.parent) if (ts.isFunctionLike(p)) return true;
  return false;
};
const insideClass = (node) => {
  for (let p = node.parent; p; p = p.parent) {
    if (ts.isClassLike(p)) return true;
    if (ts.isFunctionDeclaration(p) || ts.isFunctionExpression(p)) return false;
  }
  return false;
};
const isNull = (node) => node.kind === K.NullKeyword;
const isTypeofString = (node) => ts.isTypeOfExpression(node);
// The names a binding name declares: an identifier, or every name in a pattern.
const boundNames = (name) => {
  if (!name) return [];
  if (ts.isIdentifier(name)) return [name.text];
  return name.elements.flatMap((e) => (ts.isOmittedExpression(e) ? [] : boundNames(e.name)));
};
const parametersOf = (test) => (node) => ts.isFunctionLike(node) ? (node.parameters ?? []).filter(test).flatMap((p) => boundNames(p.name)) : [];
const isStringEnum = (n) => ts.isEnumDeclaration(n) && n.members.some((m) => m.initializer && ts.isStringLiteral(m.initializer));
const isNumericEnum = (n) => ts.isEnumDeclaration(n) && !isStringEnum(n);
// An enum is used by name and by member, anywhere its declaration is in scope.
const enumNames = (n) => [n.name.text, ...n.members.map((m) => `${n.name.text}.${m.name.getText()}`)];
const isInstanceMethod = (n) => ts.isMethodDeclaration(n) && ts.isClassLike(n.parent) && !modifier(K.StaticKeyword)(n);
const isGenericType = (n) => (ts.isTypeAliasDeclaration(n) || ts.isInterfaceDeclaration(n)) && (n.typeParameters?.length ?? 0) > 0;
const isGenericFunction = (n) => ts.isFunctionLike(n) && (n.typeParameters?.length ?? 0) > 0;
const declaredName = (test) => (n) => (test(n) && n.name && ts.isIdentifier(n.name) ? [n.name.text] : []);
const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const typeWord = (word) => new RegExp(`(^|[^\\w$."'])${escapeRegExp(word)}($|[^\\w$"'])`);

// Every feature the spec's feature matrix marks supported that the compiler
// accepts, grouped as the matrix groups them. `node` detects its syntax; `type`
// detects it in a type `tsc` printed. `test262` names the suite's areas for a
// runtime feature. `none` says why no upstream test can cover it.
const FEATURES = [
  ["Types", [
    { name: "`number`", spec: "1.1", type: typeWord("number"), node: is(K.NumberKeyword) },
    { name: "`string`", spec: "1.1", type: typeWord("string"), node: is(K.StringKeyword) },
    { name: "`boolean`", spec: "1.1", type: typeWord("boolean"), node: is(K.BooleanKeyword) },
    { name: "`null`", spec: "1.1", type: typeWord("null"), node: isNull },
    { name: "`void`", spec: "1.1", type: typeWord("void"), node: is(K.VoidKeyword) },
    { name: "`never`", spec: "1.1", type: typeWord("never"), node: is(K.NeverKeyword) },
    { name: "`bigint`", spec: "1.1", type: typeWord("bigint"), node: is(K.BigIntKeyword, K.BigIntLiteral), test262: ["BigInt"] },
    { name: "Radix literals (`0x`/`0b`/`0o`)", spec: "1.1", node: (n) => (n.kind === K.NumericLiteral || n.kind === K.BigIntLiteral) && /^0[xbo]/i.test(n.getText()) },
    { name: "`unknown`", spec: "2.11", type: typeWord("unknown"), node: is(K.UnknownKeyword) },
    { name: "Object types (`{ x: T }`)", spec: "1.2", type: /^\{ /, node: is(K.TypeLiteral, K.ObjectLiteralExpression) },
    { name: "Optional properties (`a?: T`)", spec: "1.2", node: (n) => ts.isPropertySignature(n) && !!n.questionToken },
    { name: "Object literal shorthand (`{ x }`)", spec: "1.2", node: is(K.ShorthandPropertyAssignment) },
    { name: "Arrays (`T[]`)", spec: "1.2", type: /\[\]/, node: is(K.ArrayType, K.ArrayLiteralExpression), test262: ["Array"] },
    { name: "Tuples (`[T, U]`)", spec: "1.2", type: /^\[|[(:,|] \[/, node: is(K.TupleType) },
    { name: "`readonly` arrays and tuples", spec: "1.2", type: /\breadonly /, node: (n) => ts.isTypeOperatorNode(n) && n.operator === K.ReadonlyKeyword },
    { name: "Union types (`A | B`)", spec: "1.2", type: / \| /, node: is(K.UnionType) },
    { name: "Type aliases (`type X = …`)", spec: "1.2", node: is(K.TypeAliasDeclaration), binds: declaredName(ts.isTypeAliasDeclaration), bindsInParent: true, declaresType: true },
    { name: "Literal types (`\"foo\"`, `42`, `true`)", spec: "1.2", type: /"[^"]*"|(^|[^\w.])\d+($|[^\w.])|\btrue\b|\bfalse\b/, node: (n) => ts.isLiteralTypeNode(n) && n.literal.kind !== K.NullKeyword },
  ]],
  ["Declarations", [
    { name: "`let`", spec: "1.3", node: (n) => ts.isVariableDeclarationList(n) && (n.flags & ts.NodeFlags.Let) !== 0 },
    { name: "`const`", spec: "1.3", node: (n) => ts.isVariableDeclarationList(n) && (n.flags & ts.NodeFlags.Const) !== 0 },
    { name: "Type inference on `let`/`const`", spec: "1.3", node: (n) => ts.isVariableDeclaration(n) && !n.type && !!n.initializer },
  ]],
  ["Functions", [
    { name: "Function declarations", spec: "1.4", node: (n) => ts.isFunctionDeclaration(n) && !inFunction(n), binds: declaredName((n) => ts.isFunctionDeclaration(n) && !inFunction(n)), bindsInParent: true },
    { name: "Nested function declarations", spec: "1.4", node: (n) => ts.isFunctionDeclaration(n) && inFunction(n), binds: declaredName((n) => ts.isFunctionDeclaration(n) && inFunction(n)), bindsInParent: true },
    { name: "Arrow functions", spec: "1.4", node: is(K.ArrowFunction) },
    { name: "Closures (function expressions and arrows in a function)", spec: "1.4", node: (n) => (ts.isArrowFunction(n) || ts.isFunctionExpression(n)) && inFunction(n) },
    { name: "Default parameters (`x = val`)", spec: "1.4", node: (n) => ts.isParameter(n) && !!n.initializer, binds: parametersOf((p) => !!p.initializer) },
    { name: "Rest parameters (`...args`)", spec: "2.9", node: (n) => ts.isParameter(n) && !!n.dotDotDotToken, binds: parametersOf((p) => !!p.dotDotDotToken) },
    { name: "Trailing commas", spec: "1.4", node: (n) => (ts.isCallExpression(n) || ts.isFunctionLike(n)) && ((n.arguments ?? n.parameters)?.hasTrailingComma ?? false) },
  ]],
  ["Control flow", [
    { name: "`if` / `else`", spec: "1.5", node: is(K.IfStatement) },
    { name: "`while`", spec: "1.5", node: is(K.WhileStatement) },
    { name: "`do…while`", spec: "1.5", node: is(K.DoStatement) },
    { name: "`for` (C-style)", spec: "1.5", node: is(K.ForStatement) },
    { name: "`for…of`", spec: "1.5", node: is(K.ForOfStatement), binds: (n) => ts.isForOfStatement(n) && ts.isVariableDeclarationList(n.initializer) ? n.initializer.declarations.flatMap((d) => boundNames(d.name)) : [] },
    { name: "`switch`", spec: "1.5", node: is(K.SwitchStatement, K.CaseClause) },
    { name: "`break` / `continue`", spec: "1.5", node: is(K.BreakStatement, K.ContinueStatement) },
  ]],
  ["Operators", [
    { name: "Arithmetic (`+ - * / % **`)", spec: "1.6", node: binary(K.PlusToken, K.MinusToken, K.AsteriskToken, K.SlashToken, K.PercentToken, K.AsteriskAsteriskToken) },
    { name: "Strict equality (`===`, `!==`)", spec: "1.6", node: binary(K.EqualsEqualsEqualsToken, K.ExclamationEqualsEqualsToken) },
    { name: "Loose equality (`==`, `!=`)", spec: "1.6", node: binary(K.EqualsEqualsToken, K.ExclamationEqualsToken) },
    { name: "Comparison (`< > <= >=`)", spec: "1.6", node: binary(K.LessThanToken, K.GreaterThanToken, K.LessThanEqualsToken, K.GreaterThanEqualsToken) },
    { name: "Logical (`&&`, `||`, `!`)", spec: "1.6", node: (n) => binary(K.AmpersandAmpersandToken, K.BarBarToken)(n) || prefix(K.ExclamationToken)(n) },
    { name: "Unary arithmetic (`+x`, `-x`)", spec: "1.6", node: prefix(K.PlusToken, K.MinusToken) },
    { name: "Ternary (`? :`)", spec: "1.6", node: is(K.ConditionalExpression) },
    { name: "Assignment (`=`, `+=`, …)", spec: "1.6", node: (n) => ts.isBinaryExpression(n) && n.operatorToken.kind >= K.FirstAssignment && n.operatorToken.kind <= K.LastAssignment },
    { name: "Postfix `++` / `--`", spec: "1.6", node: is(K.PostfixUnaryExpression) },
    { name: "`typeof x === \"T\"` narrowing", spec: "1.6", node: (n) => binary(K.EqualsEqualsEqualsToken, K.ExclamationEqualsEqualsToken, K.EqualsEqualsToken, K.ExclamationEqualsToken)(n) && (isTypeofString(n.left) || isTypeofString(n.right)) },
    { name: "`x === null` narrowing", spec: "1.6", node: (n) => binary(K.EqualsEqualsEqualsToken, K.ExclamationEqualsEqualsToken, K.EqualsEqualsToken, K.ExclamationEqualsToken)(n) && (isNull(n.left) || isNull(n.right)) },
    { name: "`Array.isArray(x)` narrowing", spec: "1.6", node: (n) => ts.isCallExpression(n) && n.expression.getText() === "Array.isArray" },
    { name: "`instanceof`", spec: "2.2", node: binary(K.InstanceOfKeyword) },
    { name: "`in` operator", spec: "1.6", node: binary(K.InKeyword) },
    { name: "Nullish coalescing (`??`)", spec: "1.6", node: binary(K.QuestionQuestionToken, K.QuestionQuestionEqualsToken) },
    { name: "Optional chaining (`?.`)", spec: "1.6", node: (n) => !!n.questionDotToken },
    { name: "Spread in array and object literals", spec: "2.5", node: is(K.SpreadElement, K.SpreadAssignment) },
    { name: "`as` casts", spec: "2.11", node: (n) => ts.isAsExpression(n) && !PORT_VALUE.test(n.getText()) },
    { name: "Type predicates (`x is T`)", spec: "2.11", node: is(K.TypePredicate) },
    { name: "Non-null assertion (`x!`)", spec: "2.11", node: is(K.NonNullExpression) },
  ]],
  ["Error handling", [
    { name: "`try` / `catch` / `finally`", spec: "1.8", node: is(K.TryStatement), range: is(K.TryStatement) },
    { name: "`throw`", spec: "1.8", node: is(K.ThrowStatement) },
    { name: "`catch` binding", spec: "1.8", node: (n) => ts.isCatchClause(n) && !!n.variableDeclaration, binds: (n) => ts.isCatchClause(n) ? boundNames(n.variableDeclaration?.name) : [] },
    { name: "Typed subclass `catch` (`catch (e: MyError)`)", spec: "1.8", none: "a Submilli extension: TypeScript allows only `unknown` or `any` on a `catch` variable" },
    { name: "Multiple `catch` clauses", spec: "1.8", none: "a Submilli extension: TypeScript allows one `catch` per `try`" },
    { name: "Custom error classes", spec: "1.8", node: (n) => ts.isClassLike(n) && (n.heritageClauses ?? []).some((h) => h.types.some((t) => /Error$/.test(t.expression.getText()))) },
    { name: "Built-in error classes", spec: "1.8", node: (n) => ts.isNewExpression(n) && /^(Range|Type|Syntax)?Error$/.test(n.expression.getText()), test262: ["Error", "NativeErrors"] },
  ]],
  ["Classes", [
    { name: "Class declarations", spec: "2.2", node: is(K.ClassDeclaration, K.ClassExpression), binds: declaredName(ts.isClassDeclaration), bindsInParent: true, declaresType: true },
    { name: "`constructor`", spec: "2.2", node: is(K.Constructor) },
    { name: "Instance methods", spec: "2.2", node: (n) => isInstanceMethod(n), binds: declaredName(isInstanceMethod), bindsInClassScope: true, member: true },
    { name: "Instance properties", spec: "2.2", node: (n) => ts.isPropertyDeclaration(n) && !modifier(K.StaticKeyword)(n) },
    { name: "Field initializers", spec: "2.2", node: (n) => ts.isPropertyDeclaration(n) && !!n.initializer && !modifier(K.StaticKeyword)(n) },
    { name: "Parameter properties", spec: "2.2", node: (n) => ts.isParameter(n) && ts.isConstructorDeclaration(n.parent) && ts.getModifiers(n)?.length > 0, binds: (n) => ts.isConstructorDeclaration(n) ? n.parameters.filter((p) => ts.getModifiers(p)?.length > 0).flatMap((p) => boundNames(p.name)) : [] },
    { name: "Single inheritance (`extends`)", spec: "2.2", node: (n) => ts.isHeritageClause(n) && n.token === K.ExtendsKeyword && ts.isClassLike(n.parent) },
    { name: "Generic classes", spec: "2.2", node: (n) => ts.isClassLike(n) && (n.typeParameters?.length ?? 0) > 0 },
    { name: "`public` / `private` / `readonly`", spec: "2.2", node: is(K.PublicKeyword, K.PrivateKeyword, K.ReadonlyKeyword) },
    { name: "`static` methods", spec: "2.2", node: (n) => ts.isMethodDeclaration(n) && modifier(K.StaticKeyword)(n), binds: declaredName((n) => ts.isMethodDeclaration(n) && modifier(K.StaticKeyword)(n)), bindsInClassScope: true, member: true },
    { name: "`static` fields", spec: "2.2", node: (n) => ts.isPropertyDeclaration(n) && modifier(K.StaticKeyword)(n) },
    { name: "Getters / setters", spec: "2.2", node: is(K.GetAccessor, K.SetAccessor), binds: declaredName((n) => ts.isGetAccessor(n) || ts.isSetAccessor(n)), bindsInClassScope: true, member: true },
    { name: "`this` in class methods", spec: "2.2", node: (n) => n.kind === K.ThisKeyword && insideClass(n) },
  ]],
  ["Interfaces", [
    { name: "Interface declarations", spec: "2.3", node: is(K.InterfaceDeclaration), binds: declaredName(ts.isInterfaceDeclaration), bindsInParent: true, declaresType: true },
    { name: "`implements`", spec: "2.3", node: (n) => ts.isHeritageClause(n) && n.token === K.ImplementsKeyword },
  ]],
  ["Generics", [
    { name: "Generic functions", spec: "2.1", node: (n) => isGenericFunction(n), binds: declaredName((n) => isGenericFunction(n) && ts.isFunctionDeclaration(n)), bindsInParent: true },
    { name: "Generic types", spec: "2.1", node: (n) => isGenericType(n), binds: declaredName(isGenericType), bindsInParent: true, declaresType: true },
    { name: "Explicit type arguments", spec: "2.1", node: (n) => (ts.isCallExpression(n) || ts.isNewExpression(n)) && (n.typeArguments?.length ?? 0) > 0 },
  ]],
  ["Enums", [
    { name: "Numeric enums", spec: "1.2", node: (n) => isNumericEnum(n), binds: (n) => (isNumericEnum(n) ? enumNames(n) : []), bindsInParent: true },
    { name: "String enums", spec: "1.2", node: (n) => isStringEnum(n), binds: (n) => (isStringEnum(n) ? enumNames(n) : []), bindsInParent: true },
  ]],
  ["Modules", [
    { name: "Imports of host tools", spec: "1.10", none: "Submilli-specific: `submilli:*` packages have no upstream counterpart" },
    { name: "`export` on top-level declarations (ignored)", spec: "1.10", node: modifier(K.ExportKeyword) },
  ]],
  ["Destructuring", [
    { name: "Object destructuring", spec: "1.6", node: is(K.ObjectBindingPattern), binds: (n) => ts.isObjectBindingPattern(n) ? boundNames(n) : [] },
    { name: "Array destructuring", spec: "1.6", node: is(K.ArrayBindingPattern), binds: (n) => ts.isArrayBindingPattern(n) ? boundNames(n) : [] },
    { name: "Rest in destructuring", spec: "2.6", node: (n) => ts.isBindingElement(n) && !!n.dotDotDotToken, binds: (n) => ts.isBindingElement(n) && n.dotDotDotToken ? boundNames(n.name) : [] },
  ]],
  ["Strings", [
    { name: "String literals", spec: "1.7", node: is(K.StringLiteral) },
    { name: "Template literals", spec: "1.7", node: is(K.TemplateExpression, K.NoSubstitutionTemplateLiteral) },
    { name: "String methods", spec: "1.7", test262: ["String"] },
  ]],
  ["Built-ins", [
    { name: "`Map`", spec: "2.7", node: (n) => ts.isNewExpression(n) && n.expression.getText() === "Map", test262: ["Map"] },
    { name: "`Set`", spec: "2.7", node: (n) => ts.isNewExpression(n) && n.expression.getText() === "Set", test262: ["Set"] },
    { name: "`Uint8Array`", spec: "1.2", test262: ["Uint8Array"] },
    { name: "`TextEncoder` / `TextDecoder`", spec: "1.2", none: "WHATWG Encoding, not ECMAScript: test262 has no tests for it" },
    { name: "`Boolean`", spec: "1.6", test262: ["Boolean"] },
    { name: "`Number` and numeric globals", spec: "1.6", test262: ["Number", "parseInt", "parseFloat", "isNaN", "isFinite", "NaN", "Infinity"] },
    { name: "URI functions", spec: "1.6", test262: ["encodeURIComponent", "encodeURI", "decodeURIComponent", "decodeURI"] },
    { name: "`Object` statics", spec: "1.6", test262: ["Object"] },
    { name: "`JSON`", spec: "1.6", test262: ["JSON"] },
    { name: "`Math`", spec: "1.10", test262: ["Math"] },
    { name: "`RegExp`", spec: "1.7", test262: ["RegExp"] },
    { name: "`Temporal`", spec: "1.7", test262: ["Temporal"] },
    { name: "`PermissionDeniedError`", spec: "1.8", none: "Submilli-specific: capability denials have no upstream counterpart" },
  ]],
];

const sample = process.env.COVERAGE_SAMPLE;

// Options that make an upstream case several files, or JavaScript, as in
// `port-suite.cjs`: the port makes one TypeScript file, so these can't be ported.
const MULTI_FILE_OR_JS = /^\s*\/\/\s*@(filename|allowJs|checkJs)\s*:/im;

// Rows of the spec's feature matrix marked for Phase 1 or 2 that the compiler
// rejects today, so they aren't supported yet and have no row above.
const NOT_YET_SUPPORTED = [
  "Generic constraints (`<T extends X>`)",
  "Type parameters on instance methods",
  "User-declared namespaces",
  "Interface `extends`",
  "Optional parameters (`x?`)",
  "Intersection types (`A & B`)",
  "Bitwise operators",
  "Utility types (`Partial`, `Pick`, …)",
];

function main() {
  const checkout = process.argv[2];
  if (!checkout) {
    console.error("usage: node coverage.cjs <TypeScript checkout>");
    process.exit(2);
  }
  const features = FEATURES.flatMap(([group, list]) => list.map((f) => ({ ...f, group, cases: new Set(), sites: new Set(), upstream: new Set() })));
  const upstream = upstreamCases(path.join(checkout, "tests", "cases", "conformance"), features);
  const byCase = groupBy(typescriptChecks(), (c) => c.case);
  for (const [rel, caseChecks] of byCase) {
    const source = fs.readFileSync(path.join(conformanceDir, rel), "utf8");
    const { onLine, scopes } = featureSyntax(source, features);
    for (const check of caseChecks) {
      if (check.kind === "type" && PORT_VALUE.test(check.text)) continue;
      const hit = new Set(onLine.get(check.line) ?? []);
      for (const scope of scopes) {
        const within = check.line >= scope.start && check.line <= scope.end;
        if (within && (!scope.names || (check.kind === "type" && namesMatch(scope, check)))) hit.add(scope.feature);
      }
      if (check.kind === "type") for (const f of features) if (f.type?.test(check.tsc)) hit.add(f);
      for (const f of hit) {
        f.cases.add(rel);
        f.sites.add(`${rel}:${check.line}`);
        if (sample && f.name.includes(sample)) console.log(`${f.name}\t${rel}:${check.line}\t${check.kind}\t${check.text}\t${check.tsc}`);
      }
    }
  }
  const inSuite = new Set(walk(path.join(conformanceDir, "typescript")).filter((f) => f.endsWith(".ts")).map((f) => path.relative(path.join(conformanceDir, "typescript"), f)));
  fs.writeFileSync(outputFile, render(features, test262Areas(), { upstream, inSuite }));
  console.log(`wrote ${path.relative(workspace, outputFile)}`);
}

// Every single-file upstream TypeScript case, recording in each feature the
// cases that use it. Returns how many there are.
function upstreamCases(conformance, features) {
  let count = 0;
  for (const file of walk(conformance)) {
    if (!file.endsWith(".ts") || file.endsWith(".d.ts")) continue;
    const source = fs.readFileSync(file, "utf8");
    if (MULTI_FILE_OR_JS.test(source)) continue;
    count++;
    const rel = path.relative(conformance, file);
    let tree;
    try {
      tree = ts.createSourceFile("case.ts", source, ts.ScriptTarget.Latest, true);
    } catch {
      continue; // syntax this `typescript` version can't parse
    }
    const visit = (node) => {
      for (const f of features) if (f.node?.(node)) f.upstream.add(rel);
      ts.forEachChild(node, visit);
    };
    visit(tree);
  }
  return count;
}

// Every check the TypeScript runner makes, from a run of it.
function typescriptChecks() {
  const out = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "coverage-")), "checks.tsv");
  execFileSync("cargo", ["test", "--release", "-p", "conformance", "--test", "typescript"], {
    cwd: workspace,
    env: { ...process.env, TYPESCRIPT_CHECKS_OUT: out },
    stdio: ["ignore", "ignore", "inherit"],
  });
  return fs.readFileSync(out, "utf8").split("\n").filter(Boolean).map((row) => {
    const [rel, line, kind, text, tsc] = row.split("\t");
    return { case: rel, line: Number(line), kind, text, tsc };
  });
}

// Whether a check reads a name a scope declares: the name itself, a member of it
// or a call of it; for a member, an access to it; for a type, the type `tsc`
// printed naming it.
function namesMatch(scope, check) {
  const { feature, names } = scope;
  return names.some((name) => {
    const word = escapeRegExp(name);
    if (feature.member) return new RegExp(`\\.${word}($|[^\\w$])`).test(check.text);
    if (check.text === name || new RegExp(`^${word}[.(<]`).test(check.text)) return true;
    return feature.declaresType && typeWord(name).test(check.tsc);
  });
}

// The features whose syntax starts on each line of `source`, and the line spans
// where a check counts for a feature because it is inside it (`names` absent) or
// reads a name it declares.
function featureSyntax(source, features) {
  const file = ts.createSourceFile("case.ts", source, ts.ScriptTarget.Latest, true);
  const lineOf = (pos) => file.getLineAndCharacterOfPosition(pos).line + 1;
  const onLine = new Map();
  const scopes = [];
  const visit = (node) => {
    const start = lineOf(node.getStart(file));
    const end = lineOf(node.getEnd());
    for (const f of features) {
      if (f.node?.(node)) {
        if (!onLine.has(start)) onLine.set(start, new Set());
        onLine.get(start).add(f);
      }
      if (f.range?.(node)) scopes.push({ feature: f, start, end });
      const names = f.binds?.(node) ?? [];
      if (names.length) {
        const scope = f.bindsInClassScope ? node.parent.parent : f.bindsInParent ? node.parent : node;
        scopes.push({ feature: f, start: lineOf(scope.getStart(file)), end: lineOf(scope.getEnd()), names });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return { onLine, scopes };
}

// Each test262 area: its cases and assertions.
function test262Areas() {
  const root = path.join(conformanceDir, "cases");
  const areas = new Map();
  for (const area of fs.readdirSync(root)) {
    const files = walk(path.join(root, area)).filter((f) => f.endsWith(".ts"));
    const assertions = files.reduce((n, f) => n + (fs.readFileSync(f, "utf8").match(/\bassert\w*\(/g) ?? []).length, 0);
    areas.set(area, { cases: files.length, assertions });
  }
  return areas;
}

function render(features, test262, { upstream, inSuite }) {
  const lines = [
    "# Conformance coverage",
    "",
    "Generated by `typescript-baselines/coverage.cjs`; don't edit by hand.",
    "",
    "Every feature Submilli supports, and whether the suites have dealt with every",
    "upstream test about it. A feature is **done** when every single-file upstream",
    "TypeScript test that uses it is in the suite or excluded with a reason, and every",
    "test262 area it names has been ported; **open** otherwise. **None upstream** means",
    "neither suite has a test that uses it, and **n/a** that none can, with the reason.",
    "",
    "Excluded tests aren't recorded yet (plan step 2), so every upstream test outside",
    "the suite counts as not yet dealt with.",
    "",
    "*Lines checked* is what the TypeScript suite exercises of a feature: lines where a",
    "`tsc` type or error is compared and the feature is involved, by its syntax on the",
    "line, a name it declares, or the type `tsc` printed. It is approximate, and",
    "`COVERAGE_SAMPLE=<feature>` prints what it counted. It shows a feature that",
    "pruning nearly removed; it doesn't decide whether a feature is done.",
    "",
    `Upstream: ${upstream} single-file TypeScript tests. Suite: ${inSuite.size} TypeScript cases, ${[...test262.values()].reduce((n, a) => n + a.cases, 0)} test262 cases.`,
    "",
  ];
  const statuses = new Map();
  for (const [group] of FEATURES) {
    lines.push(
      `## ${group}`,
      "",
      "| Feature | Spec | Status | Upstream tests | In the suite | Not yet dealt with | Lines checked | test262 cases | test262 checks | Note |",
      "|:--|:--|:--|--:|--:|--:|--:|--:|--:|:--|",
    );
    for (const f of features.filter((x) => x.group === group)) {
      const areas = (f.test262 ?? []).map((a) => [a, test262.get(a)]);
      const ported = areas.filter(([, a]) => a).map(([, a]) => a);
      const missing = areas.filter(([, a]) => !a).map(([name]) => `\`${name}\``);
      const brought = [...f.upstream].filter((rel) => inSuite.has(rel)).length;
      const open = f.upstream.size - brought;
      const status = statusOf(f, open, missing.length, ported.length);
      statuses.set(status, (statuses.get(status) ?? 0) + 1);
      const ts = (v) => (f.node ? String(v) : "—");
      const t262 = (key) => (f.test262 ? String(ported.reduce((n, a) => n + a[key], 0)) : "—");
      const note = f.none ?? (missing.length ? `no test262 area for ${missing.join(", ")}` : "");
      lines.push(`| ${f.name} | §${f.spec} | ${status} | ${ts(f.upstream.size)} | ${ts(brought)} | ${ts(open)} | ${ts(f.sites.size)} | ${t262("cases")} | ${t262("assertions")} | ${note} |`);
    }
    lines.push("");
  }
  lines.push(
    "## Not listed",
    "",
    "The spec's feature matrix marks these for Phase 1 or 2, or defers them, but the",
    "compiler rejects them today, so they aren't supported yet:",
    "",
    ...NOT_YET_SUPPORTED.map((name) => `- ${name}`),
    "",
  );
  const summary = ["done", "open", "none upstream", "n/a"].map((s) => `${statuses.get(s) ?? 0} ${s}`).join(", ");
  lines.splice(lines.indexOf("") + 1, 0, `**${features.length} features: ${summary}.**`, "");
  return lines.join("\n");
}

function statusOf(feature, openUpstream, missingAreas, portedAreas) {
  if (feature.none) return "n/a";
  if (openUpstream > 0 || missingAreas > 0) return "open";
  return feature.upstream.size + portedAreas > 0 ? "done" : "none upstream";
}

function groupBy(items, key) {
  const groups = new Map();
  for (const item of items) {
    const k = key(item);
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(item);
  }
  return groups;
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
}

if (require.main === module) main();
module.exports = { FEATURES };
