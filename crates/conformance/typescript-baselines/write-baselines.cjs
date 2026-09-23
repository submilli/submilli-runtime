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

const casesDir = path.join(__dirname, "..", "typescript");
const filter = process.argv[2] ?? "";

// A `// @name: value` compiler-option line. `is_option_line` in
// tests/typescript.rs must agree with it: the runner maps baseline lines back to
// the case by skipping the same lines.
const OPTION_LINE = /^\s*\/\/\s*@(\w+)\s*:\s*([^\r\n]*)/;

// The case options that change what `tsc` infers. Submilli is always strict, so
// every case is checked strictly; the port rewrites any `@strict: false`.
const BOOLEAN_OPTIONS = {
  strict: "strict",
  strictnullchecks: "strictNullChecks",
  noimplicitany: "noImplicitAny",
  exactoptionalpropertytypes: "exactOptionalPropertyTypes",
  nouncheckedindexedaccess: "noUncheckedIndexedAccess",
};

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

  const errors = errorLines(program, sourceFile);
  const errorsPath = `${base}.errors.txt`;
  if (errors.length) fs.writeFileSync(errorsPath, errors.join("\n") + "\n");
  else fs.rmSync(errorsPath, { force: true });
}

function compilerOptions(file) {
  const options = {
    strict: true,
    target: ts.ScriptTarget.ES2020,
    lib: ["lib.es2020.d.ts", "lib.dom.d.ts"],
    module: ts.ModuleKind.ES2020,
    noEmit: true,
  };
  const text = fs.readFileSync(file, "utf8");
  for (const [, name, value] of text.matchAll(new RegExp(OPTION_LINE.source, "gm"))) {
    const key = BOOLEAN_OPTIONS[name.toLowerCase()];
    if (key) options[key] = value.split(",")[0].trim().toLowerCase() === "true";
  }
  return options;
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
  const text = ts.getSourceTextOfNodeFromSourceFile(sourceFile, node).replace(/\r?\n/g, "");
  let type = ts.isExpressionWithTypeArgumentsInClassExtendsClause(node.parent)
    ? checker.getTypeAtLocation(node.parent)
    : undefined;
  if (!type || type.flags & ts.TypeFlags.Any) type = checker.getTypeAtLocation(node);
  const flags = ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.AllowUniqueESSymbolType;
  return [line, text, checker.typeToString(type, node.parent, flags)];
}

function errorLines(program, sourceFile) {
  const name = path.basename(sourceFile.fileName);
  return [...program.getSyntacticDiagnostics(sourceFile), ...program.getSemanticDiagnostics(sourceFile)]
    .filter((d) => d.category === ts.DiagnosticCategory.Error && d.file === sourceFile)
    .map((d) => {
      const { character } = sourceFile.getLineAndCharacterOfPosition(d.start);
      const message = ts.flattenDiagnosticMessageText(d.messageText, " ");
      return `${name}(${lineIndex(sourceFile, d.start) + 1},${character + 1}): error TS${d.code}: ${message}`;
    });
}

// The 0-based line `pos` is on, counted in `\n`s as the runner counts them.
// `tsc`'s own line map also breaks at U+2028, U+2029 and a lone `\r`.
function lineIndex(sourceFile, pos) {
  return sourceFile.text.slice(0, pos).split("\n").length - 1;
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
