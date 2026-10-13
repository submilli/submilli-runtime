const assert = require("node:assert/strict");
const { test } = require("node:test");
const { splitCase, eachFile, relativeSpecifier } = require("./case-files.cjs");
const { relativeImports } = require("./port-case.cjs");
const { rangesToBlank, caseFileAt } = require("./prune-case.cjs");
const { exclusionBeforePort } = require("./port-suite.cjs");

const CASE = "// @strict: true\n// @filename: a.ts\nexport let x = 1;\n// @Filename: /dir/b.ts\nimport { x } from \"a\";\nlet y = x;\n";

test("a case with fewer than two files is not split", () => {
  assert.equal(splitCase("// @filename: a.ts\nlet x = 1;\n"), null);
  assert.equal(splitCase("let x = 1;\n"), null);
});

test("each file keeps its own lines in place and blanks the others", () => {
  const [a, b] = splitCase(CASE);
  assert.equal(a.module, "a");
  assert.equal(b.module, "dir/b");
  assert.equal(b.fileName, "/__case__/dir/b.ts");
  assert.equal(a.text.length, CASE.length);
  assert.equal(b.text.length, CASE.length);
  assert.ok(a.text.startsWith("// @strict: true\n// @filename: a.ts\nexport let x = 1;\n "));
  assert.ok(b.text.endsWith("// @Filename: /dir/b.ts\nimport { x } from \"a\";\nlet y = x;\n"));
  assert.match(b.text.split("\n")[2], /^ +$/);
});

test("a step applied to each file keeps each file's lines", () => {
  const stepped = eachFile(CASE, "case.ts", (text, name) => text.replace(/let/g, name.includes("b.ts") ? "const" : "var"));
  assert.match(stepped, /export var x = 1;/);
  assert.match(stepped, /const y = x;/);
});

test("an import of another file of the case names it by relative path", () => {
  assert.equal(relativeSpecifier("a", "b"), "./b");
  assert.equal(relativeSpecifier("dir/b", "a"), "../a");
  const files = splitCase(CASE);
  const ported = relativeImports(files[1].text, files[1].fileName, files);
  assert.match(ported, /import \{ x \} from "\.\.\/a";/);
});

test("a case's files are pruned where they are in the case", () => {
  const source = "// @filename: a.ts\nexport let x = 1;\n// @filename: b.ts\nimport { x } from \"./a\";\nlet y = x;\n";
  const ranges = rangesToBlank(caseFileAt("case.ts", source), [{ offset: source.indexOf("let y"), file: 1 }]);
  assert.deepEqual(ranges.map((r) => source.slice(r.start, r.end)), ["let y = x;"]);
});

test("a multi-file case is ported unless a file is one we can't check", () => {
  const ts = "// @filename: a.ts\nexport let x = 1;\n// @filename: b.ts\nimport { x } from \"./a\";\n";
  assert.equal(exclusionBeforePort("multi.ts", ts), null);
  assert.equal(exclusionBeforePort("multi.ts", "// @filename: a.d.ts\nexport let x: number;\n// @filename: b.ts\nimport { x } from \"./a\";\n").detail, "declaration files (`.d.ts`)");
  assert.equal(exclusionBeforePort("multi.ts", "// @allowJs: true\nlet x = 1;\n").detail, "JavaScript");
  const twice = "// @filename: a.ts\nexport let x = 1;\n// @filename: ./a.ts\nexport let y = 1;\n// @filename: b.ts\nimport { x } from \"./a\";\n";
  assert.equal(exclusionBeforePort("multi.ts", twice).detail, "two files with one name: `a`");
});

test("a multi-file case whose scripts share the global scope is left out", () => {
  const shared = "// @filename: a.ts\nlet x = 1;\n// @filename: b.ts\nlet y = x;\n";
  assert.equal(exclusionBeforePort("multi.ts", shared).detail, "script files sharing one global scope");
  const separate = "// @filename: a.ts\nlet x = 1;\n// @filename: b.ts\nlet y = 2;\n";
  assert.equal(exclusionBeforePort("multi.ts", separate), null);
});

test("a statement a file leaves unfinished is pruned in that file only", () => {
  const unterminated = "// @filename: a.ts\nexport const x = 1;\nlet t = `abc\n// @filename: b.ts\nimport { x } from \"./a\";\nconst y = x;\n";
  const [range] = rangesToBlank(caseFileAt("case.ts", unterminated), [{ offset: unterminated.indexOf("let t"), file: 0 }]);
  assert.equal(unterminated.slice(range.start, range.end), "let t = `abc\n");
  const unclosed = "// @filename: a.ts\nexport function f(): void {\n  const k = 1;\n// @filename: b.ts\nimport { x } from \"./a\";\nconst z = 1;\n";
  const [atEnd] = rangesToBlank(caseFileAt("case.ts", unclosed), [{ offset: unclosed.length, file: 0 }]);
  assert.equal(unclosed.slice(atEnd.start, atEnd.end), "export function f(): void {\n  const k = 1;");
});

test("an import of a directory names its index", () => {
  const source = "// @filename: dir/index.ts\nexport let x = 1;\n// @filename: b.ts\nimport { x } from \"./dir\";\n";
  const files = splitCase(source);
  assert.match(relativeImports(files[1].text, files[1].fileName, files), /from "\.\/dir\/index"/);
});
