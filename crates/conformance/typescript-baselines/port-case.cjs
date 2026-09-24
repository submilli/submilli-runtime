// Ports a case from TypeScript's conformance suite to Submilli.
//
// The port is the mechanical one ../typescript/README.md describes, and keeps each
// line where it was, so `tsc`'s baselines and our diagnostics stay comparable line
// by line with the upstream test:
//
// - `var` becomes `let`, and `undefined` becomes `null`.
// - A typed binding with no value gets `null as unknown as (T)`, and a
//   `declare function` gets a body returning such a value.
// - A function declaration or class method with no return type gets the one `tsc`
//   infers for it, since Submilli requires it to be written.
// - `// @strict: false` becomes `// @strict: true`, and `function main(): void {}`
//   is appended.
//
// Usage: node port-case.cjs <upstream case.ts> <ported case.ts>
const ts = require("typescript");
const fs = require("fs");

const PLACEHOLDER = (ty) => `null as unknown as (${ty})`;

function main() {
  const [from, to] = process.argv.slice(2);
  if (!from || !to) {
    console.error("usage: node port-case.cjs <upstream case.ts> <ported case.ts>");
    process.exit(2);
  }
  fs.writeFileSync(to, port(fs.readFileSync(from, "utf8"), from));
}

function port(source, fileName) {
  let text = source.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
  text = text.replace(/^(\s*\/\/\s*@strict\s*:\s*)false\b/im, "$1true");
  text = rewriteTokens(text);
  text = fillDeclarations(text, fileName);
  // Return types are inferred from the program as rewritten so far, so they are
  // spelled with `null` and see the values the placeholders supply.
  text = annotateReturnTypes(text, fileName);
  if (!/^\s*function\s+main\s*\(/m.test(text)) text += "\n\nfunction main(): void {}\n";
  return text;
}

/**
 * `var` → `let` and `undefined` → `null`, as tokens, so strings and comments are left
 * alone. The scanner has no parser to tell a template's `}` from a block's, so words in
 * a template's text after its first substitution are rewritten too.
 */
function rewriteTokens(text) {
  const scanner = ts.createScanner(ts.ScriptTarget.Latest, false, ts.LanguageVariant.Standard, text);
  const edits = [];
  for (let token = scanner.scan(); token !== ts.SyntaxKind.EndOfFileToken; token = scanner.scan()) {
    if (token === ts.SyntaxKind.VarKeyword) edits.push(replace(scanner, "let"));
    else if (token === ts.SyntaxKind.Identifier && scanner.getTokenText() === "undefined") {
      edits.push(replace(scanner, "null"));
    } else if (token === ts.SyntaxKind.UndefinedKeyword) edits.push(replace(scanner, "null"));
  }
  return applyEdits(text, edits);
}

function replace(scanner, replacement) {
  return { start: scanner.getTokenStart(), end: scanner.getTokenEnd(), text: replacement };
}

/** Give each typed binding with no value, and each `declare function`, a placeholder. */
function fillDeclarations(text, fileName) {
  const sourceFile = parse(text, fileName);
  const edits = [];
  const visit = (node) => {
    if (ts.isVariableStatement(node)) {
      removeDeclare(node, sourceFile, edits);
      let filled = false;
      for (const decl of node.declarationList.declarations) {
        if (decl.type && !decl.initializer && ts.isIdentifier(decl.name)) {
          const ty = decl.type.getText(sourceFile);
          edits.push({ start: decl.type.end, end: decl.type.end, text: ` = ${PLACEHOLDER(ty)}` });
          filled = true;
        }
      }
      if (filled && text[node.end - 1] !== ";") edits.push({ start: node.end, end: node.end, text: ";" });
      // A value supplied here is not an ambient declaration another module can import.
      if (filled) removeModifier(node, ts.SyntaxKind.ExportKeyword, sourceFile, edits);
    } else if (ts.isFunctionDeclaration(node) && hasDeclare(node)) {
      removeDeclare(node, sourceFile, edits);
      const ret = node.type ? node.type.getText(sourceFile) : "void";
      // A type predicate's function returns the boolean that proves it.
      const valueTy = node.type && ts.isTypePredicateNode(node.type) ? "boolean" : ret;
      const body = ret === "void" ? "{ }" : `{ return ${PLACEHOLDER(valueTy)}; }`;
      const end = node.end;
      const hasSemicolon = text[end - 1] === ";";
      const bodyStart = hasSemicolon ? end - 1 : end;
      const typeEnd = node.type ? "" : ": void";
      edits.push({ start: bodyStart, end, text: `${typeEnd} ${body}` });
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return applyEdits(text, edits);
}

function hasDeclare(node) {
  return (ts.getModifiers(node) ?? []).some((m) => m.kind === ts.SyntaxKind.DeclareKeyword);
}

function removeDeclare(node, sourceFile, edits) {
  removeModifier(node, ts.SyntaxKind.DeclareKeyword, sourceFile, edits);
}

function removeModifier(node, kind, sourceFile, edits) {
  for (const m of ts.getModifiers(node) ?? []) {
    if (m.kind !== kind) continue;
    // The modifier and the space after it.
    const start = m.getStart(sourceFile);
    let end = m.end;
    while (sourceFile.text[end] === " ") end++;
    edits.push({ start, end, text: "" });
  }
}

/** Write the return type `tsc` infers on every function declaration and class method lacking one. */
function annotateReturnTypes(text, fileName) {
  const program = ts.createProgram([fileName], { strict: true, target: ts.ScriptTarget.ES2020 }, host(text, fileName));
  const checker = program.getTypeChecker();
  const sourceFile = program.getSourceFile(fileName);
  const edits = [];
  const visit = (node) => {
    const annotatable =
      (ts.isFunctionDeclaration(node) || ts.isMethodDeclaration(node)) &&
      node.body &&
      !node.type &&
      !(ts.isMethodDeclaration(node) && ts.isObjectLiteralExpression(node.parent));
    if (annotatable) {
      const signature = checker.getSignatureFromDeclaration(node);
      const ret = signature && checker.getReturnTypeOfSignature(signature);
      const written = ret ? checker.typeToString(ret, node, ts.TypeFormatFlags.NoTruncation) : "void";
      // `f() {` becomes `f(): T {`, not `f() : T {`.
      const end = node.body.getStart(sourceFile);
      let start = end;
      while (start > 0 && text[start - 1] === " ") start--;
      edits.push({ start, end, text: `: ${written.replace(/\bundefined\b/g, "null")} ` });
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return applyEdits(text, edits);
}

function parse(text, fileName) {
  return ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true);
}

/** A compiler host that serves `text` for `fileName` and the default libraries from disk. */
function host(text, fileName) {
  const base = ts.createCompilerHost({});
  return {
    ...base,
    getSourceFile(name, languageVersion) {
      if (name === fileName) return ts.createSourceFile(name, text, languageVersion, true);
      return base.getSourceFile(name, languageVersion);
    },
    fileExists: (name) => name === fileName || base.fileExists(name),
    readFile: (name) => (name === fileName ? text : base.readFile(name)),
  };
}

function applyEdits(text, edits) {
  // Right to left; edits at one position apply in reverse, so they read in the
  // order they were made.
  const sorted = edits.map((e, i) => ({ ...e, i })).sort((a, b) => b.start - a.start || b.i - a.i);
  let out = text;
  for (const e of sorted) out = out.slice(0, e.start) + e.text + out.slice(e.end);
  return out;
}

main();
