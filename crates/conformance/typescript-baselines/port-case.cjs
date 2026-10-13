// Ports a case from TypeScript's conformance suite to Submilli.
//
// The port is the mechanical one ../typescript/README.md describes, and keeps each
// line where it was, so `tsc`'s baselines and our diagnostics stay comparable line
// by line with the upstream test:
//
// - `var` becomes `let`. A `var` declared again in
//   the same scope, which upstream uses to check a type (`var x: T; var x = e;`),
//   would be a `let` redeclaration, so the repeat binds `x_2` instead; later reads
//   still name the first. (`port-suite.cjs` leaves out a case whose repeats are
//   what it checks: one where `tsc` reports that they differ.)
// - A typed binding with no value gets `null as unknown as (T)`, and a
//   `declare function` gets a body returning such a value.
// - A function declaration, class method or getter with no return type gets the one
//   `tsc` infers for it, and so do a class field and a parameter with a default value
//   that have no type, since Submilli requires them to be written. Where `tsc`
//   infers `any`, nothing is written.
// - `// @strict: false` and `// @strictNullChecks: false` become `true`, and `function main(): void {}`
//   is appended.
// - In a multi-file case (`case-files.cjs`), each step is made file by file, an
//   import naming another of the case's files names it by relative path, and no
//   `main` is appended: the files are the modules of a package, which has none.
//
// Usage: node port-case.cjs <upstream case.ts> <ported case.ts>
const ts = require("typescript");
const fs = require("fs");
const path = require("path");
const { splitCase, eachFile, moduleOfFileName, relativeSpecifier } = require("./case-files.cjs");
const { caseProgram, moduleOptions } = require("./tsc-case.cjs");

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
  // Without strict null checks tsc erases `null` and `undefined` from types,
  // conflating what Submilli keeps apart.
  text = text.replace(/^(\s*\/\/\s*@strictNullChecks\s*:\s*)false\b/im, "$1true");
  const files = splitCase(text);
  const multiFile = files !== null;
  text = eachFile(text, fileName, renameRedeclarations);
  text = eachFile(text, fileName, rewriteTokens);
  text = eachFile(text, fileName, (fileText, name) => fillDeclarations(fileText, name, multiFile));
  // The steps keep each file's name and lines, so `files` still names them.
  if (multiFile) text = eachFile(text, fileName, (fileText, name) => relativeImports(fileText, name, files));
  // Infer from the rewritten program, including its declaration placeholders.
  text = annotateInferredTypes(text, fileName);
  if (!multiFile && !/^\s*function\s+main\s*\(/m.test(text)) text += "\n\nfunction main(): void {}\n";
  return text;
}

/** Writes each import or re-export of another of the case's files as a relative
 * path with no extension, which is how Submilli's modules import each other.
 * Upstream often names one bare (`"m1"`), resolved by TypeScript's classic module
 * resolution, or with a `.js` extension. */
function relativeImports(text, fileName, files) {
  const sourceFile = parse(text, fileName);
  const from = moduleOfFileName(fileName);
  const modules = new Set(files.map((f) => f.module));
  const edits = [];
  for (const statement of sourceFile.statements) {
    const specifier = statement.moduleSpecifier;
    if (!(ts.isImportDeclaration(statement) || ts.isExportDeclaration(statement))) continue;
    if (!specifier || !ts.isStringLiteral(specifier)) continue;
    const to = caseModuleNamed(specifier.text, from, modules);
    if (to === undefined) continue;
    const quote = text[specifier.getStart(sourceFile)];
    edits.push({ start: specifier.getStart(sourceFile), end: specifier.end, text: `${quote}${relativeSpecifier(from, to)}${quote}` });
  }
  return applyEdits(text, edits);
}

/** The case module a specifier in module `from` names, if any: a relative path
 * from `from`'s directory, or else a bare name, with or without an extension. A
 * file name may also name a directory's `index`; `.`, `..` or a trailing `/`
 * names only the directory, as TypeScript resolves it. */
function caseModuleNamed(specifier, from, modules) {
  const bare = specifier.replace(/\.(ts|js)$/, "");
  const directory = /(^|\/)\.\.?$|\/$/.test(bare);
  const name = /^\.\.?(\/|$)/.test(bare)
    ? path.posix.normalize(path.posix.join(path.posix.dirname(from), bare))
    : bare.replace(/^\/+/, "");
  // Above the case's root, where `tsc` finds nothing either.
  if (name === ".." || name.startsWith("../")) return undefined;
  const module = name.replace(/\/$/, "").replace(/^\.$/, "");
  const candidates = directory ? [`${module}/index`] : [module, `${module}/index`];
  return candidates.map((m) => m.replace(/^\//, "")).find((m) => modules.has(m));
}

/** Renames each repeated `var` declaration in a scope `let` would share: the same
 * block, or a function body and its parameters. */
function renameRedeclarations(text, fileName) {
  const sourceFile = parse(text, fileName);
  const declared = new Map(); // scope node → name → times declared
  const edits = [];
  const declare = (scope, name) => {
    if (!declared.has(scope)) declared.set(scope, new Map());
    const names = declared.get(scope);
    const count = (names.get(name) ?? 0) + 1;
    names.set(name, count);
    return count;
  };
  const visit = (node) => {
    if (ts.isFunctionLike(node) && node.body && ts.isBlock(node.body)) {
      for (const p of node.parameters) if (ts.isIdentifier(p.name)) declare(node.body, p.name.text);
    }
    const isVar = ts.isVariableDeclarationList(node) && !(node.flags & (ts.NodeFlags.Let | ts.NodeFlags.Const));
    if (isVar) {
      // A statement's declarations share its block; a loop head's, the loop.
      const scope = ts.isVariableStatement(node.parent) ? node.parent.parent : node.parent;
      for (const decl of node.declarations) {
        if (!ts.isIdentifier(decl.name)) continue;
        const count = declare(scope, decl.name.text);
        if (count > 1) edits.push({ start: decl.name.end, end: decl.name.end, text: `_${count}` });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return applyEdits(text, edits);
}

/** Rewrite declaration keywords, leaving template text and property names alone. */
function rewriteTokens(text) {
  const sourceFile = parse(text, "case.ts");
  const edits = [];
  const visit = (node) => {
    if (ts.isVariableDeclarationList(node) && !(node.flags & (ts.NodeFlags.Let | ts.NodeFlags.Const))) {
      const start = node.getStart(sourceFile);
      if (text.slice(start, start + 3) === "var") edits.push({ start, end: start + 3, text: "let" });
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return applyEdits(text, edits);
}

/** Give each typed binding with no value, and each `declare function`, a placeholder. */
function fillDeclarations(text, fileName, multiFile) {
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
      // A value supplied here is not an ambient declaration another module can
      // import, except in a multi-file case, whose other files do import it.
      if (filled && !multiFile) removeModifier(node, ts.SyntaxKind.ExportKeyword, sourceFile, edits);
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

/** Write the type `tsc` infers where Submilli requires one and the case has none: the
 * return type of a function declaration, class method or getter, and the type of a class
 * field or of a parameter with a default value. */
function annotateInferredTypes(text, fileName) {
  const options = { strict: true, target: ts.ScriptTarget.ES2020, ...moduleOptions(text) };
  const { program, sourceFiles } = caseProgram(fileName, text, options);
  const checker = program.getTypeChecker();
  const annotated = new Map(sourceFiles.map((sourceFile) => [sourceFile.fileName, annotateFile(sourceFile, checker)]));
  return eachFile(text, fileName, (_, name) => annotated.get(name));
}

function annotateFile(sourceFile, checker) {
  const text = sourceFile.text;
  const edits = [];
  const spell = (type, node) => checker.typeToString(type, node, ts.TypeFormatFlags.NoTruncation);
  // A type that can't be written where it goes: `any`, a class expression's, or `this`.
  const unwritable = /\bany\b|\(Anonymous|\bthis\b/;
  // A type another file of the case declares and this one doesn't import, which
  // `tsc` writes as `import("…")`: nothing is written, as for a parameter.
  const imported = /\bimport\(/;
  const visit = (node) => {
    const binding =
      (ts.isPropertyDeclaration(node) && ts.isClassLike(node.parent)) || (ts.isParameter(node) && node.initializer);
    if (binding && !node.type && ts.isIdentifier(node.name)) {
      const type = checker.getTypeAtLocation(node);
      // An optional binding's type includes the `undefined` its `?` already allows.
      const members = node.questionToken && type.isUnion() ? type.types.filter((t) => !(t.flags & ts.TypeFlags.Undefined)) : [type];
      const written = members.map((t) => spell(t, node)).join(" | ");
      // After the name and any `?` or `!`.
      const at = (node.questionToken ?? node.exclamationToken ?? node.name).end;
      if (!unwritable.test(written) && !imported.test(written)) edits.push({ start: at, end: at, text: `: ${written}` });
    }
    const annotatable =
      (ts.isFunctionDeclaration(node) || ts.isMethodDeclaration(node) || ts.isGetAccessorDeclaration(node)) &&
      node.body &&
      !node.type &&
      !(!ts.isFunctionDeclaration(node) && ts.isObjectLiteralExpression(node.parent));
    if (annotatable) {
      const signature = checker.getSignatureFromDeclaration(node);
      const ret = signature && checker.getReturnTypeOfSignature(signature);
      const written = ret ? spell(ret, node) : "void";
      // `f() {` becomes `f(): T {`, not `f() : T {`.
      const end = node.body.getStart(sourceFile);
      let start = end;
      while (start > 0 && text[start - 1] === " ") start--;
      if (!imported.test(written)) edits.push({ start, end, text: `: ${written} ` });
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return applyEdits(text, edits);
}

function parse(text, fileName) {
  return ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true);
}

function applyEdits(text, edits) {
  // Right to left; edits at one position apply in reverse, so they read in the
  // order they were made.
  const sorted = edits.map((e, i) => ({ ...e, i })).sort((a, b) => b.start - a.start || b.i - a.i);
  let out = text;
  for (const e of sorted) out = out.slice(0, e.start) + e.text + out.slice(e.end);
  return out;
}

module.exports = { port, relativeImports };
if (require.main === module) main();
