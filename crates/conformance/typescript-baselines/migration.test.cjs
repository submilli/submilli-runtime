const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { port } = require("./port-case.cjs");
const { parseCaseList } = require("./case-list.cjs");
const { stageCases, applyStage } = require("./reimport-cases.cjs");
const { excludedDirectory, exclusionBeforePort } = require("./port-suite.cjs");
const { affectedSyntax } = require("./undefined-migration.cjs");
const { trimSourcePadding } = require("./source-padding.cjs");

test("staged padding cleanup preserves multiline string and template values", () => {
  const source = '/*pruned*/;   \nconst text = `left   \n${undefined} middle   \nright`;  \nconst continued = "a\\\n  b";   \n';
  const clean = trimSourcePadding(source);
  assert.equal(clean, '/*pruned*/;\nconst text = `left   \n${undefined} middle   \nright`;\nconst continued = "a\\\n  b";\n');
});

test("inventory includes implicit undefined syntax as well as literal undefined", () => {
  const reasons = affectedSyntax("interface I { a?: string; m?(): void } function f(p = 1, q?: number) {} type Pair = [number, string?]; const {a = 1} = {}; void 0;");
  for (const reason of ["optional-property", "optional-method", "parameter-default", "optional-parameter", "optional-tuple", "destructuring-default", "void-expression"]) {
    assert.ok(reasons.has(reason), reason);
  }
});

test("port keeps undefined values, type annotations, inferred returns and template text", () => {
  const source = 'var x: string | undefined = undefined;\nfunction missing() { return undefined; }\nclass Box { value = undefined; }\nconst text = `${x} undefined var ${undefined}`;\n';
  const actual = port(source, path.join(os.tmpdir(), "undefined-port.ts"));
  assert.match(actual, /let x: string \| undefined = undefined/);
  assert.match(actual, /function missing\(\): undefined/);
  assert.match(actual, /value: undefined = undefined/);
  assert.ok(actual.includes('`${x} undefined var ${undefined}`'));
  assert.equal(actual.split("\n").length, source.split("\n").length + 3);
});

test("port keeps null checks strict so tsc never erases null or undefined", () => {
  const source = "// @strict: false\n// @strictNullChecks: false\nvar x: number | undefined = 1;\n";
  const actual = port(source, path.join(os.tmpdir(), "strict-null-port.ts"));
  assert.match(actual, /\/\/ @strict: true/);
  assert.match(actual, /\/\/ @strictNullChecks: true/);
});

test("undefined and void cases are eligible without weakening unrelated exclusions", () => {
  assert.equal(excludedDirectory("types/primitives/undefined/directReferenceToUndefined.ts"), false);
  assert.equal(excludedDirectory("expressions/unaryOperators/voidOperator/void.ts"), false);
  assert.equal(exclusionBeforePort("typeGuardTypeOfUndefined.ts", "const x = undefined;"), null);
  assert.equal(excludedDirectory("types/any/assignment.ts"), true);
  assert.notEqual(exclusionBeforePort("multi.ts", "// @filename: a.ts"), null);
});

test("case lists reject escapes and are exact, sorted and deduplicated", () => {
  assert.deepEqual(parseCaseList("# selected\nz.ts\na/b.ts\nz.ts\n"), ["a/b.ts", "z.ts"]);
  for (const bad of ["", "../case.ts", "/case.ts", "a//b.ts", "a/./b.ts", "a\\b.ts", "case.d.ts", "case.js"]) {
    assert.throws(() => parseCaseList(bad));
  }
});

test("staging includes whole and triaged cases; applying preserves triage and unselected adaptations", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "undefined-reimport-test-"));
  try {
    const upstream = path.join(tmp, "upstream");
    const sources = path.join(upstream, "tests/cases/conformance");
    const cases = path.join(tmp, "cases");
    fs.mkdirSync(sources, { recursive: true });
    fs.mkdirSync(cases);
    fs.writeFileSync(path.join(sources, "whole.ts"), "var x = undefined;   \nconst text = `space   `;\n");
    fs.writeFileSync(path.join(sources, "adapted.ts"), "var y = undefined;\n");
    fs.writeFileSync(path.join(cases, "whole.ts"), "let x = null;\n");
    fs.writeFileSync(path.join(cases, "whole.triage"), "line 1 type: artifact original explanation\n");
    fs.writeFileSync(path.join(cases, "adapted.ts"), "// Adapted by hand\nlet y = 1;\n");
    const stage = path.join(tmp, "stage");
    stageCases(upstream, ["whole.ts", "adapted.ts"], stage, undefined, cases);
    assert.equal(fs.readFileSync(path.join(cases, "whole.ts"), "utf8"), "let x = null;\n");
    assert.equal(fs.readFileSync(path.join(stage, "original/whole.triage"), "utf8"), "line 1 type: artifact original explanation\n");
    applyStage(stage, ["whole.ts"], cases);
    assert.match(fs.readFileSync(path.join(cases, "whole.ts"), "utf8"), /let x = undefined/);
    const types = fs.readFileSync(path.join(cases, "whole.types"), "utf8");
    assert.doesNotMatch(types, /[ \t]+$/m);
    assert.ok(types.includes('"space   "'));
    assert.equal(fs.readFileSync(path.join(cases, "whole.triage"), "utf8"), "line 1 type: artifact original explanation\n");
    assert.equal(fs.readFileSync(path.join(cases, "adapted.ts"), "utf8"), "// Adapted by hand\nlet y = 1;\n");
    assert.throws(() => applyStage(stage, ["whole.ts"], cases), /original changed/);
    assert.throws(() => applyStage(stage, ["unknown.ts"], cases), /not staged/);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
