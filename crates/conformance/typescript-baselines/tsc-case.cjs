// How `tsc` sees a ported case: the options it is checked with, how its lines are
// counted, and the errors it gets. Shared by the baseline writer, the pruner and
// the suite porter, so all three judge a case by the same `tsc`.
const ts = require("typescript");
const fs = require("fs");

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

// The 0-based line `pos` is on, counted in `\n`s as the runner counts them.
// `tsc`'s own line map also breaks at U+2028, U+2029 and a lone `\r`.
function lineIndex(sourceFile, pos) {
  return sourceFile.text.slice(0, pos).split("\n").length - 1;
}

/** `tsc`'s errors on the case: where each starts, its 1-based line and column, code
 * and message. */
function tscErrors(file) {
  const program = ts.createProgram([file], compilerOptions(file));
  const sourceFile = program.getSourceFile(file);
  return [...program.getSyntacticDiagnostics(sourceFile), ...program.getSemanticDiagnostics(sourceFile)]
    .filter((d) => d.category === ts.DiagnosticCategory.Error && d.file === sourceFile)
    .map((d) => ({
      start: d.start,
      line: lineIndex(sourceFile, d.start) + 1,
      column: sourceFile.getLineAndCharacterOfPosition(d.start).character + 1,
      code: d.code,
      message: ts.flattenDiagnosticMessageText(d.messageText, " "),
    }));
}

module.exports = { OPTION_LINE, compilerOptions, lineIndex, tscErrors };

// `node tsc-case.cjs <case.ts>` prints its errors as JSON, for callers that check
// many cases at once in child processes.
if (require.main === module) console.log(JSON.stringify(tscErrors(process.argv[2])));
