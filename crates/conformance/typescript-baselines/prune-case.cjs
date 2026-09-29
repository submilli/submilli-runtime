// Prunes a ported case down to what Submilli supports, keeping the rest: each pass
// blanks the statements holding our errors that lack support, and the uses `tsc`
// then rejects, until none are left. ../typescript/README.md#pruning has the rules.
//
// Usage: node prune-case.cjs <typescript_case_errors> <ported case.ts>
// Build the first with `cargo build --release -p conformance --example
// typescript_case_errors`; it lands in target/release/examples/. The case is
// rewritten in place, and a JSON summary printed.
const ts = require("typescript");
const fs = require("fs");
const { spawnSync } = require("child_process");
const { tscErrors } = require("./tsc-case.cjs");

const CHECK_TIMEOUT_MS = 20_000;
const MAX_PASSES = 40;
const MARKERS = ["/*pruned*/", "/**/"];
// The value `port-case.cjs` gives a binding that had none.
const PLACEHOLDER = "= null as unknown as (";

function main() {
  const [caseErrors, file] = process.argv.slice(2);
  if (!caseErrors || !file) {
    console.error("usage: node prune-case.cjs <typescript_case_errors> <ported case.ts>");
    process.exit(2);
  }
  console.log(JSON.stringify(prune(caseErrors, file)));
}

function prune(caseErrors, file) {
  const original = fs.readFileSync(file, "utf8");
  const originalTscErrors = new Set(tscErrors(file).map(errorKey));
  let text = original;
  let passes = 0;
  // What drove the pruning: each of our errors that lacked support, with the code it
  // was on, in the order they came up.
  const lacking = new Map();
  const finish = (error) => ({
    file,
    passes,
    error,
    codeLinesBefore: codeLines(original),
    codeLinesAfter: codeLines(text),
    lacking: [...lacking.values()],
  });
  for (; passes < MAX_PASSES; passes++) {
    const found = offsetsToPrune(caseErrors, file, text, originalTscErrors);
    if (found.error) return finish(found.error);
    for (const { line, message } of found.lacking) {
      const code = (text.split("\n")[line - 1] ?? "").trim();
      const key = `${message}\n${code}`;
      if (!lacking.has(key)) lacking.set(key, { message, code });
    }
    if (!found.offsets.length) return finish(null);
    const sourceFile = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
    const units = unitsToBlank(sourceFile, found.offsets);
    if (!units.length) return finish(`nothing to prune at offsets ${found.offsets.join(", ")}`);
    for (const unit of units) text = blank(text, unit.getStart(sourceFile), unit.end, needsStatement(unit));
    fs.writeFileSync(file, text);
  }
  return finish(`still pruning after ${MAX_PASSES} passes`);
}

/** Where each of our errors that lacks support is, and each `tsc` error pruning caused. */
function offsetsToPrune(caseErrors, file, text, originalTscErrors) {
  const ours = ourErrors(caseErrors, file);
  if (ours.error) return ours;
  const tsc = tscErrors(file);
  const tscRejects = new Set(tsc.map((e) => e.line));
  // An error about a feature we may lack is a type error where `tsc` rejects the
  // line too, and the feature we lack where it doesn't.
  const lacking = ours.errors.filter((e) => e.kind === "unsupported" || (e.kind === "unclear" && !tscRejects.has(e.line)));
  const offsets = lacking.map((e) => offsetOf(text, e.line, e.column));
  for (const e of tsc) {
    if (!originalTscErrors.has(errorKey(e))) offsets.push(e.start);
  }
  return { offsets, lacking };
}

function ourErrors(caseErrors, file) {
  const run = spawnSync(caseErrors, [file], { encoding: "utf8", timeout: CHECK_TIMEOUT_MS });
  if (run.error) return { error: `typescript_case_errors: ${run.error.message}` };
  if (run.status !== 0) return { error: `typescript_case_errors failed: ${run.stderr.slice(0, 200)}` };
  const errors = run.stdout
    .split("\n")
    .filter(Boolean)
    .map((row) => {
      const [line, column, kind, message] = row.split("\t");
      return { line: Number(line), column: Number(column), kind, message };
    });
  return { errors };
}

/** What tells a `tsc` error from the others: blanking keeps lines in place. */
function errorKey(e) {
  return `${e.line}:${e.code}:${e.message}`;
}

/** The offset of a 1-based line and UTF-16 column, counting lines by `\n` as we do. */
function offsetOf(text, line, column) {
  let start = 0;
  for (let l = 1; l < line; l++) start = text.indexOf("\n", start) + 1;
  return start + column - 1;
}

/** The statements to blank for errors at `offsets`, deduplicated, right to left so
 * blanking one leaves the offsets of the rest in place. An outer statement's blank
 * covers any inner one. */
function unitsToBlank(sourceFile, offsets) {
  const units = new Map();
  for (const offset of offsets) {
    // An error outside every statement, such as a missing `main` after a lexer
    // error stopped the parse, goes once what caused it is pruned.
    const unit = statementAt(sourceFile, offset);
    if (!unit) continue;
    let widened = unit;
    for (let previous = null; widened !== previous; ) {
      previous = widened;
      widened = widenForBody(sourceFile, widenForJumps(widened));
    }
    units.set(widened.pos, widened);
  }
  return [...units.values()].sort((a, b) => b.pos - a.pos);
}

/** The innermost statement containing `offset`, other than `main`. */
function statementAt(sourceFile, offset) {
  let found = null;
  const visit = (node) => {
    if (offset < node.getStart(sourceFile) || offset >= node.end) return;
    if (isPrunable(node)) found = node;
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return found;
}

function isPrunable(node) {
  if (!ts.isStatement(node) || ts.isBlock(node)) return false;
  return !(ts.isFunctionDeclaration(node) && node.name?.text === "main");
}

/** `unit`, or, when it holds a jump out of it, the statement holding its enclosing
 * function, else its top-level statement. A function's own jumps stay inside it. */
function widenForJumps(unit) {
  if (ts.isFunctionLike(unit)) return unit;
  const jumps = jumpsOut(unit);
  if (!jumps.length) return unit;
  if (jumps.some((j) => ts.isReturnStatement(j) || ts.isThrowStatement(j))) {
    const enclosingFunction = ancestor(unit, ts.isFunctionLike);
    return enclosingFunction ? (statementHolding(enclosingFunction) ?? unit) : topLevelStatement(unit);
  }
  // Only a `break` or `continue` leaves it: the loop, `switch` or label it leaves
  // goes, and is checked in turn.
  const targets = jumps.map(jumpTarget);
  return targets.reduce((outer, t) => (t.pos < outer.pos ? t : outer));
}

function topLevelStatement(unit) {
  let topLevel = unit;
  for (let node = unit.parent; node && !ts.isSourceFile(node); node = node.parent) {
    if (isPrunable(node)) topLevel = node;
  }
  return topLevel;
}

/** `unit`, or its enclosing statement when `unit` is a body too short to leave `{}` in. */
function widenForBody(sourceFile, unit) {
  const fitsBraces = firstLineLength(sourceFile.text.slice(unit.getStart(sourceFile), unit.end)) >= "{}".length;
  if (!needsStatement(unit) || fitsBraces) return unit;
  const outer = ancestor(unit, isPrunable);
  return outer ? widenForBody(sourceFile, outer) : unit;
}

function statementHolding(node) {
  return isPrunable(node) ? node : ancestor(node, isPrunable);
}

function ancestor(node, test) {
  for (let up = node.parent; up; up = up.parent) if (test(up)) return up;
  return undefined;
}

/** The jumps within `unit` that leave it. A nested function's jumps are its own. */
function jumpsOut(unit) {
  const jumps = [];
  const visit = (node) => {
    if (node !== unit && ts.isFunctionLike(node)) return;
    if (ts.isReturnStatement(node) || ts.isThrowStatement(node)) jumps.push(node);
    else if (ts.isBreakOrContinueStatement(node) && !isWithin(jumpTarget(node), unit)) jumps.push(node);
    ts.forEachChild(node, visit);
  };
  visit(unit);
  return jumps;
}

/** The statement a `break` or `continue` leaves: its label's, or else its loop or,
 * for a `break`, its `switch`. */
function jumpTarget(jump) {
  for (let node = jump.parent; node; node = node.parent) {
    if (jump.label) {
      if (ts.isLabeledStatement(node) && node.label.text === jump.label.text) return node;
    } else if (ts.isIterationStatement(node, false) || (ts.isSwitchStatement(node) && ts.isBreakStatement(jump))) {
      return node;
    }
  }
  return jump;
}

function isWithin(node, unit) {
  return node.pos >= unit.pos && node.end <= unit.end;
}

/** Whether blanking `node` must leave a statement behind: the body of an `if` or a
 * loop, or the last clause of a `switch`, can't be empty. */
function needsStatement(node) {
  const parent = node.parent;
  // Our parser skips a lone `;` in a `switch`, where only the last clause needs a body.
  if (ts.isCaseOrDefaultClause(parent)) return parent === parent.parent.clauses.at(-1);
  return !(ts.isBlock(parent) || ts.isSourceFile(parent) || ts.isModuleBlock(parent));
}

/** Spaces in place of `text[start..end)`, keeping its line breaks, led by a marker
 * where one fits, then `{}` where a statement is still needed and `;` elsewhere, so
 * the statements either side can't run together without the one between. */
function blank(text, start, end, statementRequired) {
  const gap = text.slice(start, end).replace(/[^\n]/g, " ");
  const body = statementRequired ? "{}" : ";";
  const marker = MARKERS.find((m) => m.length + body.length <= firstLineLength(gap)) ?? "";
  const head = marker + body;
  return text.slice(0, start) + head + gap.slice(head.length) + text.slice(end);
}

function firstLineLength(text) {
  const newline = text.indexOf("\n");
  return newline === -1 ? text.length : newline;
}

/** Lines with code on them: not only comments, markers or punctuation, and not what
 * the port adds (a placeholder value, or `main`). */
function codeLines(text) {
  return text.split("\n").filter((line) => {
    const code = line
      .replace(/\/\*.*?\*\//g, "")
      .replace(/\/\/.*$/, "")
      .trim();
    if (/^[{}()[\];,]*$/.test(code) || code === "function main(): void {}") return false;
    return !code.includes(PLACEHOLDER);
  }).length;
}

main();
