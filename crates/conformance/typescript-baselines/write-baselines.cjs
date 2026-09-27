// Writes the `tsc` baselines the TypeScript conformance runner compares against.
//
// For every ported case under ../typescript it writes, next to the case:
//
// - `<case>.types`: the type `tsc` infers at each expression, in the format of
//   TypeScript's own test harness (src/harness/typeWriter.ts): each source line,
//   then a `>text : type` entry for every expression and binding starting on it.
// - `<case>.errors.txt`: one `file(line,col): error TSn: message` line per error
//   `tsc` reports, when there are any.
//
// The baselines come from running `tsc` on the *ported* case, not from
// TypeScript's recorded baselines for the original: the port gives an unassigned
// variable a value, and that changes what `tsc` infers from it.
//
// Usage: npm ci && node write-baselines.cjs [path substring]
const ts = require("typescript");
const fs = require("fs");
const path = require("path");
const { OPTION_LINE, compilerOptions, lineIndex, tscErrors } = require("./tsc-case.cjs");

const casesDir = path.join(__dirname, "..", "typescript");
const filter = process.argv[2] ?? "";

for (const file of findCases(casesDir)) {
  if (!file.includes(filter)) continue;
  writeBaselines(file);
}

function writeBaselines(file) {
  const program = ts.createProgram([file], compilerOptions(file));
  const checker = program.getTypeChecker();
  const sourceFile = program.getSourceFile(file);
  const base = file.slice(0, -".ts".length);

  fs.writeFileSync(`${base}.types`, typesBaseline(sourceFile, checker));

  const errors = errorLines(file);
  const errorsPath = `${base}.errors.txt`;
  if (errors.length) fs.writeFileSync(errorsPath, errors.join("\n") + "\n");
  else fs.rmSync(errorsPath, { force: true });
}

function typesBaseline(sourceFile, checker) {
  const entries = new Map(); // line -> [text, type][]
  const visit = (node) => {
    if (ts.isExpressionNode(node) || ts.isIdentifier(node) || ts.isDeclarationName(node)) {
      const entry = typeEntry(node, sourceFile, checker);
      if (entry) {
        const [line, ...rest] = entry;
        if (!entries.has(line)) entries.set(line, []);
        entries.get(line).push(rest);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);

  const out = [`=== ${path.basename(sourceFile.fileName)} ===`];
  sourceFile.text.split("\n").forEach((rawLine, i) => {
    const line = rawLine.replace(/\r$/, "");
    // The harness leaves out the `// @option:` lines; so does this.
    if (OPTION_LINE.test(line)) return;
    out.push(line);
    for (const [text, type] of entries.get(i) ?? []) {
      out.push(`>${text} : ${type}`);
      out.push(`>${" ".repeat(text.length)} : ${"^".repeat(type.length)}`);
    }
    if (entries.has(i)) out.push("");
  });
  return out.join("\n") + "\n";
}

// Mirrors the harness's `writeTypeOrSymbol`: a node that is part of a type, or an
// identifier that names only a type, has no value type to report.
function typeEntry(node, sourceFile, checker) {
  if (ts.isPartOfTypeNode(node)) return undefined;
  if (
    ts.isIdentifier(node) &&
    !(ts.getMeaningFromDeclaration(node.parent) & ts.SemanticMeaning.Value) &&
    !(ts.isEntityNameExpression(node.parent) || ts.isPropertyAccessExpression(node.parent)) &&
    !ts.isExpressionNode(node) &&
    !ts.isDeclarationName(node)
  ) {
    return undefined;
  }
  const line = lineIndex(sourceFile, ts.skipTrivia(sourceFile.text, node.pos));
  // A line break becomes a space, where TypeScript's harness drops it, so that
  // `a` ⏎ `>= 1` reads `a >= 1` as the runner reads our side.
  const text = ts.getSourceTextOfNodeFromSourceFile(sourceFile, node).replace(/\r?\n/g, " ");
  let type = ts.isExpressionWithTypeArgumentsInClassExtendsClause(node.parent)
    ? checker.getTypeAtLocation(node.parent)
    : undefined;
  if (!type || type.flags & ts.TypeFlags.Any) type = checker.getTypeAtLocation(node);
  const flags = ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.AllowUniqueESSymbolType;
  // A module's type names its file by absolute path; make it relative to the case,
  // so the baseline doesn't carry the path of the checkout that wrote it.
  const checkoutPrefix = `import("${path.dirname(sourceFile.fileName)}/`;
  const written = checker.typeToString(type, node.parent, flags).replaceAll(checkoutPrefix, 'import("./');
  return [line, text, written];
}

function errorLines(file) {
  const name = path.basename(file);
  return tscErrors(file).map((e) => `${name}(${e.line},${e.column}): error TS${e.code}: ${e.message}`);
}

function findCases(dir) {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) return findCases(full);
      return entry.name.endsWith(".ts") ? [full] : [];
    })
    .sort();
}
