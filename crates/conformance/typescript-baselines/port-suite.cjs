// Ports and prunes every upstream case that belongs in the suite, writes its
// baselines and `.divergences`, and drops the ones left checking too little.
// ../typescript/README.md describes which cases belong.
//
// Usage: node port-suite.cjs <TypeScript checkout> <typescript_case_errors>
// Runs `cargo test`, so it needs the workspace.
//
// A case already in the suite is left alone, so the cases ported whole before
// pruning existed keep their form.
const ts = require("typescript");
const fs = require("fs");
const path = require("path");
const { execFile } = require("child_process");
const { promisify } = require("util");

const run = promisify(execFile);
const casesDir = path.join(__dirname, "..", "typescript");
const AREAS = ["controlFlow", "expressions", "statements", "types"];
const CONCURRENCY = 8;

// Directories about a feature we exclude or haven't built: what pruning leaves of
// their cases is only scaffolding.
const EXCLUDED_DIRECTORIES = new Set([
  "expressions/binaryOperators/inOperator",
  "expressions/commaOperator",
  "expressions/identifiers",
  "expressions/optionalChaining/delete",
  "expressions/optionalChaining/privateIdentifierChain",
  "expressions/optionalChaining/taggedTemplateChain",
  "expressions/superCalls",
  "expressions/superPropertyAccess",
  "expressions/thisKeyword",
  "expressions/typeSatisfaction",
  "expressions/unaryOperators/bitwiseNotOperator",
  "expressions/unaryOperators/deleteOperator",
  "expressions/unaryOperators/typeofOperator",
  "expressions/unaryOperators/voidOperator",
  "expressions/valuesAndReferences",
  "statements/VariableStatements/usingDeclarations",
  "statements/for-await-ofStatements",
  "statements/for-inStatements",
  "statements/labeledStatements",
  "statements/withStatements",
  "types/any",
  "types/asyncGenerators",
  "types/conditional",
  "types/contextualTypes/asyncFunctions",
  "types/contextualTypes/commaOperator",
  "types/contextualTypes/jsdoc",
  "types/forAwait",
  "types/import",
  "types/intersection",
  "types/mapped",
  "types/nonPrimitive",
  "types/objectTypeLiteral/constructSignatures",
  "types/objectTypeLiteral/indexSignatures",
  "types/primitives/enum",
  "types/primitives/undefined",
  "types/thisType",
  "types/uniqueSymbol",
  "types/witness",
]);

// Case names about such a feature, in directories about something else.
const EXCLUDED_NAMES =
  /Index(er|Signature)|Intersection|templateLiteral|stringMapping|intrinsic|SymbolHasInstance|[dD]elete|ToAny|Construct(or)?Signature|extend\w+Interface|TypeOfUndefined|InJs/;

// Options that make a case several files, or JavaScript: the port makes one
// TypeScript file.
const MULTI_FILE_OR_JS = /^\s*\/\/\s*@(filename|allowJs|checkJs)\s*:/im;

// A construct signature, `{ new (): T }`, which we read as a method named `new`
// rather than rejecting (SUB-1026), so pruning can't take it out.
const CONSTRUCT_SIGNATURE = /(^|[{;,])\s*new\s*[<(]/m;

// An error the port causes wherever a class has a field with no initializer, since
// every case is made strict. It isn't a check the case makes, and both sides agree
// on it, so it doesn't make the case test the port.
const STRICT_FIELD_ERROR = "TS2564";

// How much of a case pruning must keep.
const MIN_CODE_LINES = 5;
const MIN_KEPT_FRACTION = 1 / 3;
// What a case must still check: `tsc` types compared plus lines `tsc` rejects.
const MIN_CHECKS = 5;

async function main() {
  const [typescript, caseErrors] = process.argv.slice(2);
  if (!typescript || !caseErrors) {
    console.error("usage: node port-suite.cjs <TypeScript checkout> <typescript_case_errors>");
    process.exit(2);
  }
  const upstream = {
    conformance: path.join(typescript, "tests", "cases", "conformance"),
    baselines: path.join(typescript, "tests", "baselines", "reference"),
  };
  upstream.errorBaselines = fs.readdirSync(upstream.baselines).filter((f) => f.endsWith(".errors.txt"));
  const candidates = candidateCases(upstream.conformance);
  const results = await pool(candidates, (rel) => portCase(upstream, caseErrors, rel));
  printDropReasons(results);
  const ported = dropDuplicates(results.filter((r) => r.kept).map((r) => r.rel));
  if (!ported.length) {
    console.log(`none of ${candidates.length} candidate cases kept`);
    return;
  }
  await run("node", [path.join(__dirname, "write-baselines.cjs")], { maxBuffer: 1 << 26 });
  await updateDivergences();
  const thin = ported.filter(checksTooLittle);
  for (const rel of thin) removeCase(rel);
  // Update mode removes the dropped cases' leftover files.
  await updateDivergences();
  console.log(`${ported.length - thin.length} of ${candidates.length} candidate cases kept; ${thin.length} dropped as checking too little`);
}

function candidateCases(conformance) {
  return AREAS.flatMap((area) => findCases(path.join(conformance, area)))
    .map((file) => path.relative(conformance, file))
    .filter((rel) => {
      if (EXCLUDED_DIRECTORIES.has(path.dirname(rel))) return false;
      if (EXCLUDED_NAMES.test(path.basename(rel, ".ts"))) return false;
      const source = fs.readFileSync(path.join(conformance, rel), "utf8");
      if (MULTI_FILE_OR_JS.test(source) || CONSTRUCT_SIGNATURE.test(source)) return false;
      return !fs.existsSync(path.join(casesDir, rel));
    });
}

async function portCase(upstream, caseErrors, rel) {
  const to = path.join(casesDir, rel);
  fs.mkdirSync(path.dirname(to), { recursive: true });
  const drop = (reason) => {
    removeCase(rel);
    return { rel, kept: false, reason };
  };
  try {
    await run("node", [path.join(__dirname, "port-case.cjs"), path.join(upstream.conformance, rel), to]);
    if (leavesUndefined(to)) return drop("the port left `undefined`");
    if (portCausedErrors(upstream, rel, await tscErrorText(to))) return drop("the port caused a tsc error");
    const { stdout } = await run("node", [path.join(__dirname, "prune-case.cjs"), caseErrors, to]);
    const summary = JSON.parse(stdout);
    if (summary.error) return drop(summary.error);
    const { codeLinesBefore: before, codeLinesAfter: after } = summary;
    if (after < MIN_CODE_LINES || after < before * MIN_KEPT_FRACTION) return drop(`kept ${after} of ${before} code lines`);
    return { rel, kept: true };
  } catch (e) {
    return drop(`failed: ${e.message.split("\n")[0]}`);
  }
}

/** Whether `tsc` reports, on the ported case, an error the upstream baseline lacks:
 * the port caused it, as when `var` becoming `let` makes a repeated declaration a
 * redeclaration, `undefined` becoming `null` breaks an annotation, or strictness
 * inverts what a `@strict: false` case checks. Such a case tests the port. */
function portCausedErrors(upstream, rel, portedErrors) {
  const name = path.basename(rel, ".ts");
  const upstreamErrors = upstream.errorBaselines
    .filter((f) => f === `${name}.errors.txt` || f.startsWith(`${name}(`))
    .map((f) => fs.readFileSync(path.join(upstream.baselines, f), "utf8"))
    .join("\n");
  const codes = (text) => new Set(text.match(/error TS\d+/g) ?? []);
  const upstreamCodes = codes(upstreamErrors);
  return [...codes(portedErrors)].some((code) => code !== `error ${STRICT_FIELD_ERROR}` && !upstreamCodes.has(code));
}

/** Whether the port left an `undefined` its token scan missed, as it can after a
 * template literal with substitutions. */
function leavesUndefined(file) {
  const sourceFile = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true);
  const visit = (node) => (ts.isIdentifier(node) && node.text === "undefined") || ts.forEachChild(node, visit) === true || undefined;
  return ts.forEachChild(sourceFile, visit) === true;
}

/** `tsc`'s errors on a case, one `error TSn: message` per line. */
async function tscErrorText(file) {
  const { stdout } = await run("node", [path.join(__dirname, "tsc-case.cjs"), file], { maxBuffer: 1 << 26 });
  return JSON.parse(stdout)
    .map((e) => `error TS${e.code}: ${e.message}`)
    .join("\n");
}

function printDropReasons(results) {
  const reasons = new Map();
  for (const { kept, reason } of results) {
    if (kept) continue;
    const kind = reason.replace(/\d+/g, "N");
    reasons.set(kind, (reasons.get(kind) ?? 0) + 1);
  }
  for (const [reason, count] of [...reasons].sort((a, b) => b[1] - a[1])) console.log(`dropped ${count}: ${reason}`);
}

/** `ported` without the cases whose code, comments and options aside, matches a case
 * already in the suite or one kept before it: `undefined` becoming `null` makes some
 * upstream cases twins. */
function dropDuplicates(ported) {
  const isPorted = new Set(ported);
  const seen = new Set(findCases(casesDir).filter((f) => !isPorted.has(path.relative(casesDir, f))).map(codeOf));
  return ported.filter((rel) => {
    const code = codeOf(path.join(casesDir, rel));
    if (seen.has(code)) {
      removeCase(rel);
      return false;
    }
    seen.add(code);
    return true;
  });
}

/** A case's code: no comments, options or what pruning left in place of a statement. */
function codeOf(file) {
  return fs
    .readFileSync(file, "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && line !== ";" && line !== "{}")
    .join(" ");
}

async function updateDivergences() {
  await run("cargo", ["test", "--release", "-p", "conformance", "--test", "typescript"], {
    env: { ...process.env, UPDATE_TYPESCRIPT_EXPECTED: "1" },
    maxBuffer: 1 << 26,
  });
}

/** Whether a case checks fewer than `MIN_CHECKS` things: `tsc` types compared, plus
 * lines `tsc` rejects other than for the strict-mode field error. */
function checksTooLittle(rel) {
  const base = path.join(casesDir, rel.slice(0, -".ts".length));
  const compared = Number(/types: (\d+) of/.exec(fs.readFileSync(`${base}.divergences`, "utf8"))?.[1] ?? 0);
  const errorsPath = `${base}.errors.txt`;
  const errors = fs.existsSync(errorsPath) ? fs.readFileSync(errorsPath, "utf8") : "";
  const rejected = new Set(
    [...errors.matchAll(/\((\d+),\d+\): error (TS\d+)/g)].filter((m) => m[2] !== STRICT_FIELD_ERROR).map((m) => m[1]),
  ).size;
  return compared + rejected < MIN_CHECKS;
}

function removeCase(rel) {
  const base = path.join(casesDir, rel.slice(0, -".ts".length));
  for (const suffix of [".ts", ".types", ".errors.txt", ".divergences"]) fs.rmSync(base + suffix, { force: true });
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

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
