const assert = require("node:assert/strict");
const { test } = require("node:test");
const { rangesToBlank, caseFileAt } = require("./prune-case.cjs");

/** The text each range to blank covers, for an error at the first occurrence of
 * each needle. */
function blanked(source, ...needles) {
  const errors = needles.map((needle) => ({ offset: source.indexOf(needle), file: 0 }));
  return rangesToBlank(caseFileAt("case.ts", source), errors).map((r) => source.slice(r.start, r.end));
}

test("an error in a statement blanks that statement", () => {
  assert.deepEqual(blanked("let a = 1;\nlet b = 2;\n", "b ="), ["let b = 2;"]);
});

test("an error on a token tsc skipped blanks only that token", () => {
  assert.deepEqual(blanked("let a = 1;\n\\\nlet b = 2;\n", "\\"), ["\\"]);
  assert.deepEqual(blanked("function f(): number {\n  return 1;\n})\n", ")\n"), [")"]);
  assert.deepEqual(blanked("{\n {\n  let a = 1;\n }\\\n}\n", "\\"), ["\\"]);
});

test("an error on a comment blanks the comment, but not a pruning marker", () => {
  assert.deepEqual(blanked("let a = 1;\n// @ts-expect-error\n;\n", "// @"), ["// @ts-expect-error"]);
  assert.deepEqual(blanked("class /**/;\n", "/**/"), ["class"]);
});

test("a comment's blank leaves no statement in its place", () => {
  const source = "function f(): void;\n// @ts-expect-error\nfunction f() {}\n";
  const [range] = rangesToBlank(caseFileAt("case.ts", source), [{ offset: source.indexOf("// @"), file: 0 }]);
  assert.equal(range.body, "");
});

test("an unterminated comment blanks its first line", () => {
  assert.deepEqual(blanked("let a = 1;\n/* open\n\nfunction main(): void {}\n", "/*"), ["/* open"]);
});

test("an error on a blank or at `main` blanks the statement before it", () => {
  assert.deepEqual(blanked("class /**/;\nfunction main(): void {}\n", ";"), ["class"]);
  assert.deepEqual(blanked("const\n\nfunction main(): void {}\n", "function"), ["const"]);
});

test("an empty body widens to its statement", () => {
  assert.deepEqual(blanked("let a = true\nif (a) ;\n", ";\n"), ["if (a) ;"]);
  assert.deepEqual(blanked("let a = true\nif (a) {} else ;\n", ";\n"), ["if (a) {} else ;"]);
});

test("an error after a blank isn't blamed on earlier statements", () => {
  assert.deepEqual(blanked("let a = 1;\n/**/;\nfunction main(): void {}\n", "function"), []);
});

test("an error outside every statement waits while another is in one", () => {
  assert.deepEqual(blanked("let a = 1;\n\\\nlet b = 2;\n", "\\", "b ="), ["let b = 2;"]);
});

test("two errors widening to the same statement blank it once", () => {
  const source = "class C {\n  x: number = 1;\n  m(): number {\n    return 1;\n  }\n}\n";
  assert.deepEqual(blanked(source, "x:", "return"), [source.slice(0, source.lastIndexOf("}") + 1)]);
});
