// Ports and prunes every upstream case that belongs in the suite, writes its
// baselines and `.divergences`, and drops the ones left checking too little.
// ../typescript/README.md describes which cases belong. Writes every upstream case
// left out, and why, to ../typescript/EXCLUDED.md.
//
// Usage: node port-suite.cjs [--refresh] <TypeScript checkout> <typescript_case_errors>
// Runs `cargo test`, so it needs the workspace.
//
// A case already in the suite is left alone, so the cases ported whole before
// pruning existed keep their form. `--refresh` ports again each case pruning cut
// something from, so what it cut comes back once we support it.
const fs = require("fs");
const path = require("path");
const os = require("os");
const { execFile } = require("child_process");
const { promisify } = require("util");
const ts = require("typescript");
const { fileNames, splitCase, duplicateModules } = require("./case-files.cjs");
const { caseProgram, OPTION_LINE } = require("./tsc-case.cjs");

const run = promisify(execFile);
const casesDir = path.join(__dirname, "..", "typescript");
const CONCURRENCY = 8;

// Why an upstream case is left out. The detail says which feature, error or case.
const REASONS = {
  unsupported: "not supported",
  port: "the port changes what it checks",
  duplicate: "duplicate",
  thin: "checks too little",
  porter: "porter failure",
};

// Directories about a feature we exclude or haven't built, and the feature: what
// pruning leaves of their cases is only scaffolding. Each covers its subdirectories.
const EXCLUDED_DIRECTORIES = new Map([
  ["async", "async/await"],
  ["asyncGenerators", "async/await and generators"],
  ["classes/classStaticBlock", "static blocks"],
  ["classes/indexMemberDeclarations", "index signatures"],
  ["classes/staticIndexSignature", "index signatures"],
  ["decorators", "decorators"],
  ["dynamicImport", "dynamic imports"],
  ["es6/computedProperties", "computed property names"],
  ["es6/decorators", "decorators"],
  ["es6/Symbols", "`Symbol`"],
  ["es6/yieldExpressions", "generators"],
  ["esDecorators", "decorators"],
  ["generators", "generators"],
  ["internalModules", "user-declared namespaces"],
  ["parser/ecmascript2018/asyncGenerators", "async/await and generators"],
  ["parser/ecmascript2018/forAwait", "async/await"],
  ["parser/ecmascript5/ComputedPropertyNames", "computed property names"],
  ["parser/ecmascript5/IndexMemberDeclarations", "index signatures"],
  ["parser/ecmascript5/IndexSignatures", "index signatures"],
  ["parser/ecmascript5/Symbols", "`Symbol`"],
  ["parser/ecmascript6/ComputedPropertyNames", "computed property names"],
  ["parser/ecmascript6/Symbols", "`Symbol`"],
  ["Symbols", "`Symbol`"],
  ["expressions/binaryOperators/inOperator", "the `in` operator"],
  ["expressions/commaOperator", "the comma operator"],
  ["expressions/optionalChaining/delete", "`delete`"],
  ["expressions/optionalChaining/privateIdentifierChain", "private `#names`"],
  ["expressions/optionalChaining/taggedTemplateChain", "tagged templates"],
  ["expressions/typeSatisfaction", "`satisfies`"],
  ["expressions/unaryOperators/bitwiseNotOperator", "bitwise operators"],
  ["expressions/unaryOperators/deleteOperator", "`delete`"],
  ["statements/VariableStatements/usingDeclarations", "`using` declarations"],
  ["statements/for-await-ofStatements", "async/await"],
  ["statements/for-inStatements", "`for…in`"],
  ["statements/labeledStatements", "labeled statements"],
  ["statements/withStatements", "`with`"],
  ["types/any", "`any`"],
  ["types/asyncGenerators", "async/await and generators"],
  ["types/conditional", "conditional types"],
  ["types/contextualTypes/asyncFunctions", "async/await"],
  ["types/contextualTypes/commaOperator", "the comma operator"],
  ["types/contextualTypes/jsdoc", "JSDoc types in JavaScript"],
  ["types/forAwait", "async/await"],
  ["types/import", "`import()` types"],
  ["types/intersection", "intersection types"],
  ["types/mapped", "mapped types"],
  ["types/nonPrimitive", "the `object` type"],
  ["types/objectTypeLiteral/constructSignatures", "construct signatures"],
  ["types/objectTypeLiteral/indexSignatures", "index signatures"],
  ["types/thisType", "`this` types"],
  ["types/uniqueSymbol", "`Symbol`"],
]);

// Case names about such a feature, in directories about something else.
const EXCLUDED_NAMES = [
  [/Index(er|Signature)/, "index signatures"],
  [/Intersection/, "intersection types"],
  [/templateLiteral/, "template literal types"],
  [/stringMapping|intrinsic/, "string mapping types"],
  [/SymbolHasInstance/, "`Symbol`"],
  [/[dD]elete/, "`delete`"],
  [/ToAny/, "`any`"],
  [/Construct(or)?Signature/, "construct signatures"],
  [/extend\w+Interface/, "adding to a built-in interface"],
  [/InJs/, "JavaScript"],
  [/topLevelAwait/, "async/await"],
];

// Options that make a case JavaScript.
const JAVASCRIPT_OPTION = /^\s*\/\/\s*@(allowJs|checkJs)\s*:\s*true/im;

const JAVASCRIPT_FILE = /\.(js|jsx|mjs|cjs)$/i;
const JAVASCRIPT_FEATURE = "JavaScript";

// The files of a case that aren't TypeScript modules we can check, and what they
// are. The first that matches a file's name names it.
const UNSUPPORTED_FILES = [
  [/(^|\/)node_modules\//, "`node_modules` packages"],
  // Also one for another kind of file, such as `component.d.html.ts`.
  [/\.d\.([^/]+\.)?[mc]?ts$/i, "declaration files (`.d.ts`)"],
  [/\.tsx$/i, "JSX"],
  [/\.json$/i, "JSON modules"],
  [/\.[mc]ts$/i, "`.mts` and `.cts` files"],
  [/^(?!.*\.ts$)/, "files other than `.ts`"],
];

// A multi-file case's file with no `import` or `export` is a script, whose
// top-level declarations are global: every file sees them. Our files are modules,
// each seeing only what it imports, so a case whose files use what another
// script declares, or whose scripts declare the same name, checks something else.
const SHARED_SCOPE_FEATURE = "script files sharing one global scope";

// Two files of a case with one name: upstream checks both, where a module path
// can name only one.
const DUPLICATE_FILES_FEATURE = "two files with one name";

// A construct signature, `new (): T`. One in an object type is rejected, but one
// in an interface is read as a method named `new`, which pruning can't take out,
// and the pattern can't tell the two apart.
const CONSTRUCT_SIGNATURE = /(^|[{;,])\s*new\s*[<(]/m;
const CONSTRUCT_SIGNATURE_FEATURE = "construct signatures";

// An error the port causes wherever a class has a field with no initializer, since
// every case is made strict. It isn't a check the case makes, and both sides agree
// on it, so it doesn't make the case test the port.
const STRICT_FIELD_ERROR = "TS2564";

// "Subsequent variable declarations must have the same type": what a repeated `var`
// checks upstream.
const REDECLARED_TYPE_ERROR = "TS2403";

// How much of a case pruning must keep, when it cut something.
const MIN_CODE_LINES = 5;
const MIN_KEPT_FRACTION = 1 / 3;
// What a case must still check: `tsc` types compared plus lines `tsc` rejects. A
// pruned case checking less is mostly the scaffolding of what was cut; one pruning
// cut nothing from checks what upstream wrote it to, however little.
const MIN_CHECKS = 5;
const MIN_CHECKS_WHOLE = 1;

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--case-list") || args.includes("--apply-stage")) {
    return require("./reimport-cases.cjs").main(args);
  }
  const refresh = args[0] === "--refresh";
  const [typescript, caseErrors] = refresh ? args.slice(1) : args;
  if (!typescript || !caseErrors) {
    console.error("usage: node port-suite.cjs [--refresh] <TypeScript checkout> <typescript_case_errors>");
    process.exit(2);
  }
  const upstream = {
    conformance: path.join(typescript, "tests", "cases", "conformance"),
    baselines: path.join(typescript, "tests", "baselines", "reference"),
  };
  upstream.errorBaselines = fs.readdirSync(upstream.baselines).filter((f) => f.endsWith(".errors.txt"));
  await checkSuitePasses();
  const { candidates, excluded } = candidateCases(upstream.conformance, refresh);
  const results = await pool(candidates, (rel) => portCase(upstream, caseErrors, rel));
  excluded.push(...results.filter((r) => !r.kept));
  const kept = results.filter((r) => r.kept);
  const { unique, duplicates } = dropDuplicates(kept.map((r) => r.rel));
  excluded.push(...duplicates);
  let ported = [];
  if (unique.length) {
    const portedList = path.join(os.tmpdir(), `port-suite-${process.pid}.txt`);
    fs.writeFileSync(portedList, unique.join("\n"));
    try {
      await run("node", [path.join(__dirname, "write-baselines.cjs")], { maxBuffer: 1 << 26 });
      await updateDivergences(portedList);
      const causes = new Map(kept.map((r) => [r.rel, r.cause]));
      const minChecks = (rel) => (causes.get(rel) ? MIN_CHECKS : MIN_CHECKS_WHOLE);
      const thin = unique.map((rel) => ({ rel, checks: checksOf(rel) })).filter((c) => c.checks < minChecks(c.rel));
      for (const { rel, checks } of thin) {
        removeCase(rel);
        excluded.push(causes.get(rel) ?? exclusion(rel, REASONS.thin, `${checks} after the port`));
      }
      // Update mode removes the dropped cases' leftover files and listings.
      await updateDivergences(portedList);
      ported = unique.filter((rel) => !thin.some((t) => t.rel === rel));
    } finally {
      fs.rmSync(portedList, { force: true });
    }
  }
  writeExcluded(excluded);
  printReasons(excluded);
  console.log(`${ported.length} of ${candidates.length} candidate cases kept`);
}

function exclusion(rel, reason, detail) {
  return { rel, kept: false, reason, detail };
}

/** The upstream cases to port, and those left out before porting. A directory left
 * out is one entry, whatever it holds. */
function candidateCases(conformance, refresh) {
  const candidates = [];
  const excluded = [...EXCLUDED_DIRECTORIES].map(([dir, feature]) =>
    exclusion(`${dir}/`, REASONS.unsupported, feature),
  );
  // A declaration file (`.d.ts`) has no code to run, and isn't a case.
  const all = findCases(conformance)
    .filter((file) => !file.endsWith(".d.ts"))
    .map((file) => path.relative(conformance, file));
  for (const rel of all) {
    const existing = path.join(casesDir, rel);
    if (fs.existsSync(existing)) {
      if (!refresh || !wasPruned(existing)) continue;
      // Its explanations name lines of the case as it is.
      if (fs.existsSync(existing.replace(/\.ts$/, ".triage"))) console.log(`not refreshed, has a .triage: ${rel}`);
      else candidates.push(rel);
      continue;
    }
    if (excludedDirectory(rel)) continue;
    const reason = exclusionBeforePort(rel, fs.readFileSync(path.join(conformance, rel), "utf8"));
    if (reason) excluded.push(reason);
    else candidates.push(rel);
  }
  return { candidates, excluded };
}

function excludedDirectory(rel) {
  return [...EXCLUDED_DIRECTORIES.keys()].some((dir) => rel.startsWith(`${dir}/`));
}

function exclusionBeforePort(rel, source) {
  const named = EXCLUDED_NAMES.find(([pattern]) => pattern.test(path.basename(rel, ".ts")));
  if (named) return exclusion(rel, REASONS.unsupported, named[1]);
  if (isJavaScript(source)) return exclusion(rel, REASONS.unsupported, JAVASCRIPT_FEATURE);
  for (const name of fileNames(source)) {
    const kind = UNSUPPORTED_FILES.find(([pattern]) => pattern.test(name));
    if (kind) return exclusion(rel, REASONS.unsupported, kind[1]);
  }
  const text = source.replace(/\r\n?/g, "\n");
  const files = splitCase(text);
  const twice = duplicateModules(files ?? []);
  if (twice.length) return exclusion(rel, REASONS.unsupported, `${DUPLICATE_FILES_FEATURE}: \`${twice[0]}\``);
  if (files && sharesGlobalScope(text)) return exclusion(rel, REASONS.unsupported, SHARED_SCOPE_FEATURE);
  if (CONSTRUCT_SIGNATURE.test(source)) return exclusion(rel, REASONS.unsupported, CONSTRUCT_SIGNATURE_FEATURE);
  return null;
}

/** Whether an upstream case is JavaScript, by its options or a file's name. */
function isJavaScript(source) {
  return JAVASCRIPT_OPTION.test(source) || fileNames(source).some((name) => JAVASCRIPT_FILE.test(name));
}

/** Whether the files of the multi-file case `text` share declarations through
 * the global scope: a name in one file resolves to a script file's declaration in
 * another, or two script files declare the same name. */
function sharesGlobalScope(text) {
  // The name is only what `caseProgram` calls a single file: this case has several.
  const { program, sourceFiles } = caseProgram("case.ts", text);
  const checker = program.getTypeChecker();
  const scripts = new Set(sourceFiles.filter((f) => !ts.isExternalModule(f)));
  const declared = new Map(); // name → script file declaring it
  for (const script of scripts) {
    for (const name of topLevelNames(script)) {
      if (declared.has(name) && declared.get(name) !== script) return true;
      declared.set(name, script);
    }
  }
  return sourceFiles.some((sourceFile) => usesAnotherScript(sourceFile, scripts, checker));
}

/** Whether a name in `sourceFile` resolves to a declaration in another of `scripts`. */
function usesAnotherScript(sourceFile, scripts, checker) {
  let found = false;
  const visit = (node) => {
    if (found) return;
    if (ts.isIdentifier(node)) {
      const declarations = checker.getSymbolAtLocation(node)?.declarations ?? [];
      found = declarations.some((d) => d.getSourceFile() !== sourceFile && scripts.has(d.getSourceFile()));
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return found;
}

/** The names a file declares at its top level. */
function topLevelNames(sourceFile) {
  return sourceFile.statements.flatMap((statement) => {
    if (ts.isVariableStatement(statement)) {
      return statement.declarationList.declarations.filter((d) => ts.isIdentifier(d.name)).map((d) => d.name.text);
    }
    return statement.name && ts.isIdentifier(statement.name) ? [statement.name.text] : [];
  });
}

/** Whether pruning cut something from a case: `prune-case.cjs` marks what it blanks. */
function wasPruned(file) {
  return /\/\*pruned\*\/|\/\*\*\/[;{]/.test(fs.readFileSync(file, "utf8"));
}

async function portCase(upstream, caseErrors, rel) {
  const to = path.join(casesDir, rel);
  fs.mkdirSync(path.dirname(to), { recursive: true });
  const drop = (reason, detail) => {
    removeCase(rel);
    return exclusion(rel, reason, detail);
  };
  try {
    await run("node", [path.join(__dirname, "port-case.cjs"), path.join(upstream.conformance, rel), to]);
    const upstreamErrors = upstreamErrorCodes(upstream, rel);
    // The port renames a repeated `var`, so a check that repeats agree goes.
    if (upstreamErrors.has(`error ${REDECLARED_TYPE_ERROR}`)) {
      return drop(REASONS.port, `it renames the repeated \`var\` declarations whose types \`tsc\` checks (${REDECLARED_TYPE_ERROR})`);
    }
    // Pruning takes out each statement where `tsc` reports an error upstream doesn't.
    const expectedCodes = [...upstreamErrors, `error ${STRICT_FIELD_ERROR}`].map((code) => code.replace("error TS", ""));
    const { stdout } = await run("node", [path.join(__dirname, "prune-case.cjs"), caseErrors, to, expectedCodes.join(",")]);
    const summary = JSON.parse(stdout);
    if (summary.error) {
      const ours = summary.lacking[0];
      // An error about the case as a whole, such as a circular import, is on its first
      // line, an option line no statement holds: a feature we lack, not a porter bug.
      if (summary.lacking.length && summary.lacking.every((l) => OPTION_LINE.test(l.code))) {
        return drop(REASONS.unsupported, ours.message);
      }
      return drop(REASONS.porter, ours ? `${summary.error}; our first unsupported error: ${ours.message}` : summary.error);
    }
    if (summary.unprunable.length) return drop(REASONS.port, portCausedDetail(summary.unprunable));
    const { codeLinesBefore: before, codeLinesAfter: after } = summary;
    const pruned = summary.passes > 0;
    const cause = pruned ? pruningCause(rel, summary) : null;
    if (pruned && (after < MIN_CODE_LINES || after < before * MIN_KEPT_FRACTION)) return drop(cause.reason, cause.detail);
    return { rel, kept: true, cause: pruned ? cause : null };
  } catch (e) {
    return drop(REASONS.porter, e.message.split("\n")[0]);
  }
}

/** Why pruning cut what it did from a case: a feature we lack (`unsupportedCause`),
 * or, when nothing we lack was cut, the errors the port caused. */
function pruningCause(rel, summary) {
  if (summary.lacking.length || !summary.portCaused.length) return unsupportedCause(rel, summary.lacking);
  return exclusion(rel, REASONS.port, portCausedDetail(summary.portCaused.map((e) => e.code)));
}

function portCausedDetail(codes) {
  return `\`tsc\` then reports ${codes.join(", ")}`;
}

/** A case pruning cut for a feature we lack: our first error that lacked support,
 * and the code it was on. Later ones are often what the first left behind, and an
 * error on a line of only punctuation, such as a parser recovering at `}`, says
 * little. */
function unsupportedCause(rel, lacking) {
  if (!lacking.length) return exclusion(rel, REASONS.porter, "pruning cut code with no error of ours to cut");
  const top = lacking.find((l) => /\w/.test(l.code)) ?? lacking[0];
  const code = top.code.length > 80 ? `${top.code.slice(0, 77)}...` : top.code;
  // A code span's fence is longer than any run of backticks inside it.
  const fence = "`".repeat(Math.max(0, ...(code.match(/`+/g) ?? []).map((run) => run.length)) + 1);
  return exclusion(rel, REASONS.unsupported, `${top.message}, on ${fence} ${code} ${fence}`);
}

/** The `error TSn` codes of the upstream baselines for a case, under any options. */
function upstreamErrorCodes(upstream, rel) {
  const name = path.basename(rel, ".ts");
  const text = upstream.errorBaselines
    .filter((f) => f === `${name}.errors.txt` || f.startsWith(`${name}(`))
    .map((f) => fs.readFileSync(path.join(upstream.baselines, f), "utf8"))
    .join("\n");
  return errorCodes(text);
}

function errorCodes(text) {
  return new Set(text.match(/error TS\d+/g) ?? []);
}

function printReasons(excluded) {
  const reasons = new Map();
  for (const { reason } of excluded) reasons.set(reason, (reasons.get(reason) ?? 0) + 1);
  for (const [reason, count] of [...reasons].sort((a, b) => b[1] - a[1])) console.log(`left out ${count}: ${reason}`);
}

/** `ported` split into the cases to keep and those whose code, comments and options
 * aside, matches a case already in the suite or one kept before it. */
function dropDuplicates(ported) {
  const isPorted = new Set(ported);
  const seen = new Map(
    findCases(casesDir)
      .filter((f) => !isPorted.has(path.relative(casesDir, f)))
      .map((f) => [codeOf(f), path.relative(casesDir, f)]),
  );
  // A case with no code is no one's twin: it checks too little.
  seen.delete(codeOf(null));
  const unique = [];
  const duplicates = [];
  for (const rel of ported) {
    const code = codeOf(path.join(casesDir, rel));
    const twin = code === codeOf(null) ? undefined : seen.get(code);
    if (twin) {
      removeCase(rel);
      duplicates.push(exclusion(rel, REASONS.duplicate, `of \`${twin}\``));
      continue;
    }
    seen.set(code, rel);
    unique.push(rel);
  }
  return { unique, duplicates };
}

/** Writes ../typescript/EXCLUDED.md: every upstream case left out, and why. */
function writeExcluded(excluded) {
  // A control character, such as the NUL a scanner case is about, is escaped so
  // the file stays text.
  const cell = (text) =>
    text.replace(/\|/g, "\\|").replace(/[\u0000-\u0008\u000b-\u001f]/g, (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`);
  const rows = [...excluded]
    .sort((a, b) => a.rel.localeCompare(b.rel))
    .map(({ rel, reason, detail }) => `| \`${rel}\` | ${reason} | ${cell(detail)} |`);
  const counts = Object.values(REASONS)
    .map((reason) => [reason, excluded.filter((e) => e.reason === reason).length])
    .filter(([, n]) => n)
    .map(([reason, n]) => `${n} ${reason}`)
    .join(", ");
  const text = [
    "# Upstream cases left out",
    "",
    "Every upstream conformance case that isn't in the suite, and why. A directory (ending",
    "in `/`) is left out whole, with its subdirectories, except for any case in the suite.",
    "Written by `../typescript-baselines/port-suite.cjs`: change its lists or the porter,",
    "not this file. Declaration files (`.d.ts`) aren't cases, and `.tsx` files aren't read.",
    "",
    `${excluded.length} entries: ${counts}.`,
    "",
    "| Case | Reason | Detail |",
    "|:-----|:-------|:-------|",
    ...rows,
    "",
  ].join("\n");
  fs.writeFileSync(path.join(casesDir, "EXCLUDED.md"), text);
}

/** A case's code: no comments, options or what pruning left in place of a statement.
 * `null` gives the code of a case that is only the `main` the port adds. */
function codeOf(file) {
  return (file === null ? "function main(): void {}" : fs.readFileSync(file, "utf8"))
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && line !== ";" && line !== "{}")
    .join(" ");
}

/** Stops the run unless the suite passes as committed: update mode fails on what it
 * can't fix, such as an unexplained divergence, and failing halfway through would
 * leave a half-ported suite. */
async function checkSuitePasses() {
  try {
    await run("cargo", ["test", "--release", "-p", "conformance", "--test", "typescript"], { env: { ...process.env, SUBMILLI_TEST_NIGHTLY_ONLY: "1" }, maxBuffer: 1 << 26 });
  } catch (e) {
    console.error(`the suite must pass before porting; fix it first:\n${e.stdout?.slice(-4000) ?? ""}`);
    process.exit(1);
  }
}

/** Updates every case's `.divergences`, listing the unexplained divergences of the
 * cases named in the `portedList` file. */
async function updateDivergences(portedList) {
  try {
    await run("cargo", ["test", "--release", "-p", "conformance", "--test", "typescript"], {
      env: { ...process.env, SUBMILLI_TEST_NIGHTLY_ONLY: "1", UPDATE_TYPESCRIPT_EXPECTED: "1", TYPESCRIPT_PORTED_CASES: portedList },
      maxBuffer: 1 << 26,
    });
  } catch (e) {
    // The runner reports why a case failed on stdout.
    throw new Error(`the runner failed in update mode:\n${e.stdout?.slice(-4000) ?? ""}${e.stderr ?? ""}`);
  }
}

/** How many things a case checks: `tsc` types compared, plus lines `tsc` rejects
 * other than for the strict-mode field error. */
function checksOf(rel) {
  const base = path.join(casesDir, rel.slice(0, -".ts".length));
  const compared = Number(/types: (\d+) of/.exec(fs.readFileSync(`${base}.divergences`, "utf8"))?.[1] ?? 0);
  const errorsPath = `${base}.errors.txt`;
  const errors = fs.existsSync(errorsPath) ? fs.readFileSync(errorsPath, "utf8") : "";
  const rejected = new Set(
    [...errors.matchAll(/\((\d+),\d+\): error (TS\d+)/g)].filter((m) => m[2] !== STRICT_FIELD_ERROR).map((m) => m[1]),
  ).size;
  return compared + rejected;
}

function removeCase(rel) {
  const base = path.join(casesDir, rel.slice(0, -".ts".length));
  for (const suffix of [".ts", ".types", ".errors.txt", ".divergences", ".triage"]) fs.rmSync(base + suffix, { force: true });
}

async function pool(items, work) {
  const results = [];
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      results[i] = await work(items[i]);
    }
  };
  await Promise.all(Array.from({ length: CONCURRENCY }, worker));
  return results;
}

function findCases(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) return findCases(full);
      return entry.name.endsWith(".ts") ? [full] : [];
    })
    .sort();
}

module.exports = { exclusionBeforePort, isJavaScript, excludedDirectory, findCases, wasPruned, upstreamErrorCodes, codeOf, unsupportedCause, writeExcluded };

if (require.main === module) main().catch((e) => {
  console.error(e);
  process.exit(1);
});
